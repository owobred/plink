use clap::{arg, command, value_parser};
use std::{fmt::Debug, path::PathBuf};
use symphonia::core::{
    audio::{AudioBuffer, Signal},
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};
use tracing::{debug, info, instrument, trace, warn};

#[tokio::main]
async fn main() {
    {
        use tracing_subscriber::prelude::*;

        tracing_subscriber::registry()
            .with(tracing_subscriber::fmt::layer())
            .with(tracing_subscriber::EnvFilter::from_default_env())
            .init()
    }

    let matches = command!()
        .arg(
            arg!([file] "The file name to process")
                .required(true)
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(arg!(-t --title <TITLE> "The title of the song"))
        .arg(arg!(-d --database <DATABASE> "URL to the postgres database"))
        .get_matches();

    let file = matches.get_one::<PathBuf>("file").unwrap();
    let start = std::time::Instant::now();
    let spectrogram = handle_file(file);
    let elapsed = start.elapsed();
    info!(?elapsed, "completed parse");
    let start = std::time::Instant::now();
    persist_to_db(&matches.get_one::<String>("database").unwrap(), spectrogram).await;
    let elapsed = start.elapsed();
    info!(?elapsed, "completed insert");
}

#[instrument(level = "trace")]
fn handle_file(filename: &PathBuf) -> Vec<Vec<f32>> {
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
    let spectrogram = spect_gen.run(
        &first_channel,
        &process::SpectrogramConfig {
            fft_len: 1280,
            overlap: 640 + 320,
        },
    );
    let elapsed = start.elapsed();
    debug!(?elapsed, "spectrogram generated");
    spectrogram
}

#[instrument(skip_all, level = "trace")]
async fn persist_to_db(url: &str, spectrogram: Vec<Vec<f32>>) {
    let database = database::Database::connect(url).await.expect("failed to connect to database");

    database.insert_sectrogram_for_song(1, spectrogram, 44100, 1280, 640 + 320).await.expect("failed to insert spectrogram");
}