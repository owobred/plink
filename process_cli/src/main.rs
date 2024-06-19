use clap::{arg, Parser};
use process::SpectrogramConfig;
use rubato::Resampler;
use std::{fmt::Debug, path::PathBuf, sync::Arc};
use symphonia::core::{
    audio::AudioBuffer, formats::FormatOptions, io::MediaSourceStream, meta::MetadataOptions,
    probe::Hint,
};
use tracing::{debug, info, instrument, trace, warn};

const TARGET_SAMPLERATE_HZ: usize = 30_000;
const SPECTROGRAM_CONFIG: &SpectrogramConfig = &process::SpectrogramConfig {
    fft_len: 1280,
    overlap: 320,
};
const DATE_FORMAT: &[time::format_description::BorrowedFormatItem<'static>] =
    time::macros::format_description!("[day]/[month]/[year]");

#[derive(Debug, clap::Parser)]
enum Command {
    /// Upload a single song to the database
    Upload {
        /// The path to this song's audio file
        path: PathBuf,
        /// The title of this song, including any artists
        #[arg(long, short)]
        title: String,
        /// This song's `singer_id`
        #[arg(long, short)]
        singer_id: usize,
        #[arg(long, short)]
        db: String,
        // TODO: figure out how to make clap parse the date
        /// The date this song was sung at, in `dd/mm/yyyy` format
        #[arg(long, short)]
        sung_at: Option<String>,
    },
    /// Upload many songs to the database
    UploadBulk {
        /// The directory to look through
        directory: PathBuf,
        /// The shell script to use to parse filenames
        /// should be able to be substituted into `sh {shell_script} {file_path}`
        ///
        /// The script should return a json dictionary
        /// On success, it should be in the form
        /// ```json
        /// {
        ///     "success": true,
        ///     "title": String,
        ///     "day": usize,
        ///     "month": usize,
        ///     "year": usize,
        ///     "singer": usize,
        /// }
        /// ```
        /// On failure it should instead be
        /// ```json
        /// {
        ///     "success": false,
        ///     "error": String,
        /// }
        /// ```
        #[arg(long, short)]
        shell_script: String,
        /// The url to connect to the database
        #[arg(long, short)]
        db: String,
        /// The number of songs to upload simultaneously
        #[arg(long, short, default_value_t = 64)]
        max_concurrency: usize,
    },
    /// See if a song matches any in the database
    Discover {
        /// The file to load
        path: PathBuf,
        /// The url to connect to the database
        #[arg(long, short)]
        db: String,
        /// The maximum distance to look for matching samples
        #[arg(long, short, default_value_t = 200.0)]
        max_distance: f64,
        /// The maximum number of matching samples to look for
        #[arg(long, short, default_value_t = 40)]
        results_per: usize,
        /// The number of samples to attempt to match simultaneously
        #[arg(long, short, default_value_t = 200)]
        max_concurrency: usize,
        /// Make the program output a json dictionary with the results
        #[arg(long, short, action = clap::ArgAction::SetTrue)]
        json: bool,
        /// How many potential matches should be included in the results?
        #[arg(long, short, default_value_t = 10)]
        n_matches: usize,
    },
}

#[tokio::main]
async fn main() {
    {
        use tracing_subscriber::prelude::*;

        tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(std::io::stderr)
                    .with_filter(tracing_subscriber::EnvFilter::from_default_env()),
            )
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
                sung_at.map(|date| time::Date::parse(&date, DATE_FORMAT).unwrap()),
            )
            .await
        }
        Command::UploadBulk {
            directory,
            shell_script,
            db,
            max_concurrency,
        } => upload_bulk(directory, &shell_script, &db, max_concurrency).await,
        Command::Discover {
            path,
            db,
            max_distance,
            results_per,
            max_concurrency,
            json,
            n_matches,
        } => {
            discover_song(
                &path,
                &db,
                max_distance,
                results_per,
                max_concurrency,
                json,
                n_matches,
            )
            .await
        }
    };
}

async fn upload_song(
    file: PathBuf,
    title: &str,
    singer_id: usize,
    db_url: &str,
    sung_at: Option<time::Date>,
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
            local_path: Some(file.to_str().unwrap().to_string()),
        },
        SPECTROGRAM_CONFIG,
    )
    .await;
    let elapsed = start.elapsed();
    info!(?elapsed, "completed insert");
}

async fn upload_bulk(directory: PathBuf, executable: &str, db: &str, max_concurrency: usize) {
    let db = database::Database::connect(db)
        .await
        .expect("failed to connect to database");

    let mut handles = Vec::new();
    let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrency));

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
            let full_file_path = file
                .path()
                .canonicalize()
                .expect("failed to normalize path")
                .to_str()
                .unwrap()
                .to_string();

            tokio::task::spawn(async move {
                let _guard = semaphore
                    .acquire()
                    .await
                    .expect("faile to acquire semaphore");
                let already_saved = db.song_already_saved(&full_file_path).await.expect("failed to query db");

                if already_saved {
                    warn!(path=full_file_path, "skipping file as path is already in database");
                    return;
                }

                let command_output = tokio::process::Command::new("sh")
                    .arg(shell_script)
                    .arg(file.file_name())
                    .stdout(std::process::Stdio::piped())
                    .spawn()
                    .expect("failed to spawn subprocess")
                    .wait_with_output()
                    .await
                    .expect("failed to get command output");
                let command_result: ParseResult =
                    serde_json::from_slice(command_output.stdout.trim_ascii_end())
                        .expect("failed to parse command output");

                let metadata = match command_result {
                    ParseResult::Parsed {
                        title,
                        date,
                        singer_id,
                    } => {
                        let date = date.map(|date| {
                            time::Date::parse(
                                &format!("{:02}/{:02}/{}", date.day, date.month, date.year),
                                DATE_FORMAT,
                            )
                            .expect("failed to parse date somehow")
                        });
                        debug!(title, ?date, "got song metadata");
                        database::models::SongMetadata {
                            title,
                            singer_id: singer_id as i16,
                            date_first_sung: date,
                            local_path: Some(full_file_path),
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

async fn discover_song(
    path: &PathBuf,
    db_url: &str,
    max_distance: f64,
    results_per_query: usize,
    max_concurrency: usize,
    output_json: bool,
    n_matches: usize,
) {
    info!("generating spectrogram");
    let start = std::time::Instant::now();
    let spectrogram = handle_file(path, SPECTROGRAM_CONFIG);
    let spectrogram_time = start.elapsed();

    let db = database::Database::connect(db_url)
        .await
        .expect("failed to connect to db");

    let mut hashmap = std::collections::HashMap::new();
    info!("querying database");

    let (send, mut recv) = tokio::sync::mpsc::unbounded_channel();
    let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrency));

    let start = std::time::Instant::now();
    for sample in spectrogram {
        let db = db.clone();
        let send = send.clone();
        let semaphore = semaphore.clone();

        tokio::task::spawn(async move {
            let _guard = semaphore
                .acquire()
                .await
                .expect("failed to aquire semaphore");
            let result = db
                .find_similar_to(sample, max_distance, results_per_query as i64)
                .await
                .expect("failed to query database");
            send.send(result).expect("failed to send to mpsc");
        });
    }

    drop(send);

    while let Some(result) = recv.recv().await {
        let n = result.len();
        for (index, (song_id, _sample_id, _distance)) in result.into_iter().enumerate() {
            if !hashmap.contains_key(&song_id) {
                hashmap.insert(song_id, 0);
            }
            *hashmap.get_mut(&song_id).unwrap() += n - index;
        }
    }
    let query_time = start.elapsed();

    let mut top = hashmap.into_iter().collect::<Vec<_>>();
    top.sort_by_key(|(_, v)| *v);
    top.reverse();

    let singers = db.get_singers().await.expect("failed to fetch from db");

    let mut result = DiscoverResult {
        entries: Vec::with_capacity(n_matches),
        timings: DiscoverTimings {
            spectrogram: spectrogram_time,
            query: query_time,
        },
    };

    for (song_id, score) in &top[..n_matches] {
        let song_info = db
            .get_song(*song_id)
            .await
            .expect("database error")
            .unwrap();
        let singer_id = song_info.metadata.singer_id;

        result.entries.push(DiscoverEntry {
            song: song_info.into(),
            singer_name: singers.get(&singer_id).unwrap().name.clone(),
            score: *score,
        })
    }

    info!(timings=?result.timings, "completed");
    info!("top {n_matches} matches");
    for (index, entry) in result.entries.iter().enumerate() {
        info!(
            "{: >3}: {} [id={}]: score={}",
            index + 1,
            entry.song.title,
            entry.song.id,
            entry.score
        );
    }

    if output_json {
        println!(
            "{}",
            serde_json::to_string(&result).expect("failed to serialize json")
        )
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
        date: Option<ParsedDate>,
        singer_id: usize,
    },
    Error {
        error: String,
    },
}

#[derive(Debug, serde::Deserialize)]
struct ParsedDate {
    day: usize,
    month: usize,
    year: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DiscoverResult {
    entries: Vec<DiscoverEntry>,
    timings: DiscoverTimings,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DiscoverEntry {
    song: Song,
    singer_name: String,
    score: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
struct Song {
    id: i64,
    title: String,
    date_sung: Option<time::Date>,
    file_path: Option<String>,
}

impl From<database::models::Song> for Song {
    fn from(value: database::models::Song) -> Self {
        Self {
            id: value.id,
            title: value.metadata.title,
            date_sung: value.metadata.date_first_sung,
            file_path: value.metadata.local_path,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct DiscoverTimings {
    spectrogram: std::time::Duration,
    query: std::time::Duration,
}
