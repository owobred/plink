use clap::{arg, command, value_parser};
use std::path::PathBuf;
use symphonia::core::{
    audio::{AudioBuffer, Signal},
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};
use tracing::{debug, info, instrument, trace, warn};

fn main() {
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
        .get_matches();

    let file = matches.get_one::<PathBuf>("file").unwrap();
    let start = std::time::Instant::now();
    handle_file(file);
    let elapsed = start.elapsed();
    info!(?elapsed, "completed parse")
}

#[instrument(level = "trace")]
fn handle_file(filename: &PathBuf) {
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
        // let inner: symphonia::core::audio::AudioBuffer<f32> = decoded.make_equivalent();
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

    // let pchannel = &channels[0][100_000..1_000_000];
    let pchannel = &channels[0];
    let spect_gen: process::SpectrogramGenerator<f32> = process::SpectrogramGenerator::default();
    let start = std::time::Instant::now();
    let spectrogram = spect_gen.run(
        &pchannel,
        &process::SpectrogramConfig {
            fft_len: 640,
            overlap: 320,
        },
    );
    let elapsed = start.elapsed();
    println!("spectrogram took {elapsed:?}");
    let max_value = *spectrogram
        .iter()
        .map(|row| {
            row.into_iter()
                .max_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Greater))
                .unwrap()
        })
        .max_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Greater))
        .unwrap();
    let (width, height) = (spectrogram.len(), spectrogram[0].len() / 2);
    dbg!((width, height));
    dbg!(max_value);
    let mut canvas: image::ImageBuffer<image::Rgb<u8>, Vec<u8>> =
        image::ImageBuffer::new(height as u32, width as u32);
    canvas
        .rows_mut()
        .zip(spectrogram.into_iter())
        .for_each(|(row, spect_row)| {
            row.zip(spect_row.into_iter()).for_each(|(canvas, value)| {
                *canvas = image::Rgb([(value
                     * u8::MAX as f32) as u8; 3])
            })
        });
    let get_rotated_idiot = image::imageops::rotate270(&canvas);
    get_rotated_idiot.save("spect.png").unwrap();

    todo!()
}
