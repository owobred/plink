use clap::{arg, command, value_parser, Parser};
use process::SpectrogramConfig;
use rubato::Resampler;
use std::{fmt::Debug, path::PathBuf, sync::Arc};
use symphonia::core::{
    audio::{AudioBuffer, Signal},
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};
use tracing::{debug, info, instrument, trace, warn};

const TARGET_SAMPLERATE_HZ: usize = 40_000;
const SPECTROGRAM_CONFIG: &SpectrogramConfig = &process::SpectrogramConfig {
    fft_len: 1280,
    overlap: 320,
};
const DATE_FORMAT: &[time::format_description::BorrowedFormatItem<'static>] =
    time::macros::format_description!("[day]/[month]/[year]");

#[derive(Debug, clap::Parser)]
enum Command {
    Upload {
        path: PathBuf,
        #[arg(long, short)]
        title: String,
        #[arg(long, short)]
        singer_id: usize,
        #[arg(long, short)]
        db: String,
        // TODO: figure out how to make clap parse the date
        #[arg(long, short)]
        sung_at: String,
    },
    UploadBulk {
        directory: PathBuf,
        #[arg(long, short)]
        shell_script: String,
        #[arg(long, short)]
        db: String,
    },
    Discover {
        path: PathBuf,
        #[arg(long, short)]
        db: String,
        #[arg(long, short, default_value_t = 200.0)]
        max_distance: f64,
        #[arg(long, short, default_value_t = 40)]
        results_per: usize,
    },
}

#[tokio::main]
async fn main() {
    {
        use tracing_subscriber::prelude::*;

        tracing_subscriber::registry()
            .with(tracing_subscriber::fmt::layer())
            .with(tracing_subscriber::EnvFilter::from_default_env())
            .init()
    }

    match Command::parse() {
        Command::Upload {
            path,
            title,
            singer_id,
            db,
            sung_at,
        } => {
            upload_song(
                path,
                &title,
                singer_id,
                &db,
                time::Date::parse(&sung_at, DATE_FORMAT).unwrap(),
            )
            .await
        }
        Command::UploadBulk {
            directory,
            shell_script,
            db,
        } => upload_bulk(directory, &shell_script, &db).await,
        Command::Discover {
            path,
            db,
            max_distance,
            results_per,
        } => discover_song(&path, &db, max_distance, results_per).await,
    };
}

async fn upload_song(
    file: PathBuf,
    title: &str,
    singer_id: usize,
    db_url: &str,
    sung_at: time::Date,
) {
    let db = database::Database::connect(db_url)
        .await
        .expect("failed to connect to db");

    let start = std::time::Instant::now();
    let spectrogram = handle_file(&file, SPECTROGRAM_CONFIG);
    let elapsed = start.elapsed();
    info!(?elapsed, "completed parse");

    // debug_to_image(&spectrogram);
    let start = std::time::Instant::now();
    persist_to_db(
        db,
        spectrogram,
        &database::models::SongMetadata {
            title: title.to_string(),
            singer_id: singer_id as i16,
            date_first_sung: sung_at,
            local_path: file.to_str().unwrap().to_string(),
        },
        SPECTROGRAM_CONFIG,
    )
    .await;
    let elapsed = start.elapsed();
    info!(?elapsed, "completed insert");
}

async fn upload_bulk(directory: PathBuf, executable: &str, db: &str) {
    let db = database::Database::connect(db)
        .await
        .expect("failed to connect to database");

    let mut handles = Vec::new();
    let semaphore = Arc::new(tokio::sync::Semaphore::new(6));

    for dir in std::fs::read_dir(directory).expect("failed to read directory") {
        let file = match dir {
            Ok(file) => file,
            Err(error) => {
                warn!(?error, "failed to iterate file");
                continue;
            }
        };

        if !file.file_type().expect("failed to get file type").is_file() {
            debug!(?file, "skipping as not a file");
            continue;
        }

        let task: tokio::task::JoinHandle<()> = {
            let semaphore = semaphore.clone();
            let db = db.clone();
            let shell_script = executable.to_string();

            tokio::task::spawn(async move {
                let _guard = semaphore
                    .acquire()
                    .await
                    .expect("faile to acquire semaphore");
                let command_output = tokio::process::Command::new("sh")
                    .arg(shell_script)
                    .arg(file.file_name())
                    .stdout(std::process::Stdio::piped())
                    .spawn()
                    .expect("failed to spawn subprocess")
                    .wait_with_output()
                    .await
                    .expect("failed to get command output");
                println!("{:?}", String::from_utf8_lossy(&command_output.stdout));
                let command_result: ParseResult =
                    serde_json::from_slice(command_output.stdout.trim_ascii_end())
                        .expect("failed to parse command output");

                let metadata = match command_result {
                    ParseResult::Parsed {
                        title,
                        day,
                        month,
                        year,
                        singer_id,
                    } => {
                        let date =
                            time::Date::parse(&format!("{day:02}/{month:02}/{year}"), DATE_FORMAT)
                                .expect("failed to parse date somehow");
                        debug!(title, ?date, "got song metadata");
                        database::models::SongMetadata {
                            title,
                            singer_id: singer_id as i16,
                            date_first_sung: date,
                            local_path: file
                                .path()
                                .canonicalize()
                                .expect("failed to normalize path")
                                .to_str()
                                .unwrap()
                                .to_string(),
                        }
                    }
                    ParseResult::Error { error } => {
                        warn!(?error, "failed to parse filename");
                        return;
                    }
                };

                let spectrogram = handle_file(&file.path(), SPECTROGRAM_CONFIG);
                persist_to_db(db, spectrogram, &metadata, SPECTROGRAM_CONFIG).await;
            })
        };

        handles.push(task);
    }

    let join = futures::future::join_all(handles.into_iter()).await;

    let ok = join.iter().filter(|r| r.is_ok()).count();
    let err = join.iter().filter(|r| r.is_err()).count();

    info!(ok, err, "upload finished");
}

async fn discover_song(path: &PathBuf, db_url: &str, max_distance: f64, results_per_query: usize) {
    info!("generating spectrogram");
    let spectrogram = handle_file(path, SPECTROGRAM_CONFIG);

    let db = database::Database::connect(db_url)
        .await
        .expect("failed to connect to db");

    let mut hashmap = std::collections::HashMap::new();
    info!("querying database");

    for sample in &spectrogram {
        let closest = db
            .find_similar_to(sample.to_owned(), max_distance, results_per_query as i64)
            .await
            .unwrap();
        let n = closest.len();

        for (index, (song_id, _sample_id, _distance)) in closest.into_iter().enumerate() {
            if !hashmap.contains_key(&song_id) {
                hashmap.insert(song_id, 0);
            }
            // TODO: some kind of avg distance vs number of occurances would be nice
            *hashmap.get_mut(&song_id).unwrap() += n - index;
        }
    }

    let mut top = hashmap.into_iter().collect::<Vec<_>>();
    top.sort_by_key(|(_, v)| *v);
    top.reverse();

    for (song_id, score) in &top[..10] {
        let song_info = db
            .get_song(*song_id)
            .await
            .expect("database error")
            .unwrap();
        info!("{} [id={song_id}]: score={score}", song_info.metadata.title);
    }
}

#[instrument(level = "trace")]
fn handle_file(
    filename: &PathBuf,
    spectrogram_config: &process::SpectrogramConfig,
) -> Vec<Vec<f32>> {
    debug!("opening file");
    let registry = symphonia::default::get_codecs();
    let probe = symphonia::default::get_probe();
    let file = std::fs::File::open(filename).unwrap();
    let stream = MediaSourceStream::new(
        Box::new(file),
        symphonia::core::io::MediaSourceStreamOptions::default(),
    );
    let mut format = probe
        .format(
            &Hint::new(),
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .unwrap();

    let metadata = format.metadata.get();
    debug!(?metadata, "read song");
    let tracks = format.format.tracks();
    if tracks.len() != 1 {
        warn!(?tracks, "song had multiple tracks, using only default");
    }
    let track = format.format.default_track().unwrap();
    let mut decoder = registry
        .make(
            &track.codec_params,
            &symphonia::core::codecs::DecoderOptions::default(),
        )
        .unwrap();
    info!(params=?track.codec_params, "read codec params");
    let samplerate = track.codec_params.sample_rate.unwrap();
    let track_id = track.id;

    let mut channels: Vec<Vec<f32>> = Vec::new();

    while let Ok(packet) = format.format.next_packet() {
        if packet.track_id() != track_id {
            continue;
        }

        let decoded = decoder.decode(&packet).unwrap();
        let mut converted: AudioBuffer<f32> =
            AudioBuffer::new(decoded.frames() as u64, decoded.spec().to_owned());
        decoded.convert(&mut converted);
        let planes = converted.planes();
        let planes_slice = planes.planes();
        if channels.len() != planes_slice.len() {
            trace!("resizing channels due to size mismatch");
            channels.resize_with(planes_slice.len(), || Vec::new());
        }
        channels
            .iter_mut()
            .zip(planes_slice)
            .for_each(|(d, v)| d.extend(*v));
    }

    // TODO: maybe do something for each channel idk?
    let first_channel = &channels[0];

    debug!("resampling audio");
    let mut resampler = rubato::FftFixedIn::new(
        samplerate as usize,
        TARGET_SAMPLERATE_HZ,
        first_channel.len(),
        640,
        1,
    )
    .unwrap();
    let resampled = resampler
        .process(&[first_channel], None)
        .unwrap()
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    debug!("generating spectrogram");
    let spect_gen: process::SpectrogramGenerator<f32> = process::SpectrogramGenerator::default();
    let start = std::time::Instant::now();
    let spectrogram = spect_gen.run(&resampled, &spectrogram_config);
    let elapsed = start.elapsed();
    debug!(?elapsed, "spectrogram generated");
    spectrogram
}

fn debug_to_image(spectrogram: &Vec<Vec<f32>>) {
    let (width, height) = (spectrogram.len(), spectrogram[0].len());
    let mut canvas: image::ImageBuffer<image::Rgb<u8>, Vec<u8>> =
        image::ImageBuffer::new(height as u32, width as u32);
    canvas
        .rows_mut()
        .zip(spectrogram.into_iter())
        .for_each(|(row, spect_row)| {
            row.zip(spect_row.into_iter()).for_each(|(canvas, value)| {
                *canvas = image::Rgb([(value * u8::MAX as f32) as u8; 3])
            })
        });
    let get_rotated_idiot = image::imageops::rotate270(&canvas);
    get_rotated_idiot.save("spect.png").unwrap();
}

#[instrument(skip_all, level = "trace")]
async fn persist_to_db(
    db: database::Database,
    spectrogram: Vec<Vec<f32>>,
    song_metadata: &database::models::SongMetadata,
    spectrogram_config: &process::SpectrogramConfig,
) -> i64 {
    let song_id = db
        .insert_new_song(
            spectrogram,
            song_metadata,
            TARGET_SAMPLERATE_HZ,
            spectrogram_config.fft_len,
            spectrogram_config.overlap,
        )
        .await
        .expect("failed to insert song");

    info!(song_id, metadata=?song_metadata, spec_cofig=?spectrogram_config, "inserted song");

    song_id
}

#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum ParseResult {
    Parsed {
        title: String,
        day: usize,
        month: usize,
        year: usize,
        singer_id: usize,
    },
    Error {
        error: String,
    },
}
