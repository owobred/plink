use clap::{arg, command, value_parser, Parser};
use std::{fmt::Debug, path::PathBuf};
use symphonia::core::{
    audio::{AudioBuffer, Signal},
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};
use tracing::{debug, info, instrument, trace, warn};

#[derive(Debug, clap::Parser)]
enum Command {
    Upload {
        path: PathBuf,
        #[arg[long, long]]
        title: String,
        #[arg[short, long]]
        singer_id: usize,
        #[arg[short, long]]
        db: String,
        // TODO: figure out how to make clap parse the date
        #[arg[short, long]]
        sung_at: String,
    },
    Discover {
        path: PathBuf,
        #[arg[long, long]]
        db: String,
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
                time::Date::parse(
                    &sung_at,
                    time::macros::format_description!("[day]/[month]/[year]"),
                )
                .unwrap(),
            )
            .await
        }
        Command::Discover { path, db } => discover_song(&path, &db).await,
    };
}

async fn upload_song(
    file: PathBuf,
    title: &str,
    singer_id: usize,
    db_url: &str,
    sung_at: time::Date,
) {
    let spectrogram_config = process::SpectrogramConfig {
        fft_len: 1280,
        overlap: 320,
    };

    let start = std::time::Instant::now();
    let spectrogram = handle_file(&file, &spectrogram_config);
    let elapsed = start.elapsed();
    info!(?elapsed, "completed parse");

    // debug_to_image(&spectrogram);
    let start = std::time::Instant::now();
    persist_to_db(
        db_url,
        spectrogram,
        &database::models::SongMetadata {
            title: title.to_string(),
            singer_id: singer_id as u16,
            date_first_sung: sung_at,
            local_path: file.to_str().unwrap().to_string(),
        },
        &spectrogram_config,
    )
    .await;
    let elapsed = start.elapsed();
    info!(?elapsed, "completed insert");
}

async fn discover_song(path: &PathBuf, db_url: &str) {
    let spectrogram_config = process::SpectrogramConfig {
        fft_len: 1280,
        overlap: 320,
    };

    let spectrogram = handle_file(path, &spectrogram_config);

    let db = database::Database::connect(db_url).await.expect("failed to connect to db");
    debug!("querying database");
    let closest = db.find_similar_to(spectrogram[10000].clone(), 100.0, 50).await.unwrap();

    println!("{closest:?}");
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
    let spect_gen: process::SpectrogramGenerator<f32> = process::SpectrogramGenerator::default();
    let start = std::time::Instant::now();
    let spectrogram = spect_gen.run(&first_channel, &spectrogram_config);
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
    url: &str,
    spectrogram: Vec<Vec<f32>>,
    song_metadata: &database::models::SongMetadata,
    spectrogram_config: &process::SpectrogramConfig,
) {
    let database = database::Database::connect(url)
        .await
        .expect("failed to connect to database");

    let song_id = database
        .insert_new_song(
            spectrogram,
            song_metadata,
            44_100,
            spectrogram_config.fft_len,
            spectrogram_config.overlap,
        )
        .await
        .expect("failed to insert song");

    info!(song_id, metadata=?song_metadata, spec_cofig=?spectrogram_config, "inserted song")
}
