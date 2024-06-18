use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
};

use rustfft::{FftNum, FftPlanner};
use tracing::instrument;

pub trait Float: FftNum + num_traits::Float {}
impl Float for f32 {}
impl Float for f64 {}

#[derive(Clone)]
pub struct SpectrogramGenerator<T: Float> {
    planner: Arc<Mutex<FftPlanner<T>>>,
    haans: Arc<RwLock<HashMap<usize, Arc<Vec<f32>>>>>,
}

impl<T: Float> Default for SpectrogramGenerator<T> {
    fn default() -> Self {
        Self {
            planner: Arc::new(Mutex::new(FftPlanner::new())),
            haans: Default::default(),
        }
    }
}

impl<T: Float> SpectrogramGenerator<T> {
    pub fn new_with_planner(planner: FftPlanner<T>) -> Self {
        Self {
            planner: Arc::new(Mutex::new(planner)),
            ..Default::default()
        }
    }

    #[instrument(skip(self, samples), level = "trace")]
    pub fn run(&self, samples: &[f32], config: &SpectrogramConfig) -> Vec<Vec<T>> {
        let mut planner_guard = self.planner.lock().unwrap();
        let fft = planner_guard.plan_fft_forward(config.fft_len);
        drop(planner_guard);
        let hann = self.get_hann(config.fft_len);
        let hann_slice = hann.as_slice();

        let spectrogram = samples
            .windows(config.fft_len)
            .step_by(config.fft_len - config.overlap)
            .map(|window| {
                window
                    .into_iter()
                    .zip(hann_slice)
                    .map(|(sample, hann)| sample * hann)
                    .map(|scaled| {
                        num_complex::Complex::new(T::from_f32(scaled).unwrap(), T::zero())
                    })
                    .collect::<Vec<_>>()
            })
            .map(|mut window| {
                fft.process(window.as_mut_slice());
                window
            })
            .map(|complex| {
                complex
                    .into_iter()
                    // half the the fft is mirrored due to complex inputs
                    .take(config.fft_len / 2)
                    .map(|val| val.norm_sqr().sqrt())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        spectrogram
    }

    fn get_hann(&self, size: usize) -> Arc<Vec<f32>> {
        let read = self.haans.read().unwrap();

        match read.contains_key(&size) {
            true => read.get(&size).unwrap().to_owned(),
            false => {
                drop(read);
                self.generate_hann(size)
            }
        }
    }

    #[instrument(skip(self), level = "trace")]
    fn generate_hann(&self, size: usize) -> Arc<Vec<f32>> {
        let hann = generate_hanning_window(size);
        let hann = Arc::new(hann);
        let mut write = self.haans.write().unwrap();
        write.insert(size, hann.clone());
        hann
    }
}

#[derive(Debug)]
pub struct SpectrogramConfig {
    pub fft_len: usize,
    pub overlap: usize,
}

impl Default for SpectrogramConfig {
    fn default() -> Self {
        Self {
            fft_len: 80,
            overlap: 8,
            // samplerate: 48_000,
        }
    }
}

fn generate_hanning_window(size: usize) -> Vec<f32> {
    let mut out = vec![0.0; size];

    for i in 0..size {
        out[i] = 0.5 * (1.0 - (std::f32::consts::TAU * (i as f32 / size as f32)).cos());
    }

    out
}
