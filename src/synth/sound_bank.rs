use enum_map::{enum_map, EnumMap};
use hound::WavReader;
use rubato::Resampler;
use strum_macros::Display;

#[derive(Copy, Clone, enum_map::Enum, Display, enum_iterator::Sequence, PartialEq)]
#[repr(u8)]
pub enum Sample {
    HihatClosed,
    HihatOpen,
    Snare,
    Cowbell,
    Kick,
}

#[derive(Clone)]
pub struct Bank {
    samples: EnumMap<Sample, Vec<f32>>,
    sample_rate: u32,
}

fn sample_to_vec(data: &[u8], sample_rate: u32) -> Vec<f32> {
    let wav = WavReader::new(data).unwrap();
    let in_sample_rate = wav.spec().sample_rate as f64;
    let num_samples = wav.len() as usize;
    assert!(wav.spec().channels == 1);
    let buf: Vec<f32> = wav
        .into_samples::<i16>()
        .map(|s| s.unwrap() as f32 / i16::MAX as f32)
        .collect();
    // TODO use fft resampler instead? how to avoid it changing the timing?
    let mut resampler = rubato::SincFixedIn::new(
        sample_rate as f64 / in_sample_rate,
        1.0,
        rubato::InterpolationParameters {
            sinc_len: 256,
            f_cutoff: 0.95,
            oversampling_factor: 128,
            interpolation: rubato::InterpolationType::Cubic,
            window: rubato::WindowFunction::Blackman,
        },
        num_samples,
        1, //< channels
    )
    .unwrap();
    let out = resampler.process(&[buf], None).unwrap();
    debug_assert!(out.len() == 1);
    out.into_iter().next().unwrap()
}

impl Bank {
    pub fn new(sample_rate: u32) -> Self {
        Bank {
            samples: enum_map! {
                Sample::HihatClosed => {
                    sample_to_vec(include_bytes!("../../samples/hihat_closed.wav"), sample_rate)
                },
                Sample::HihatOpen => {
                    sample_to_vec(include_bytes!("../../samples/hihat_open.wav"), sample_rate)
                },
                Sample::Snare => {
                    sample_to_vec(include_bytes!("../../samples/snare.wav"), sample_rate)
                },
                Sample::Cowbell => {
                    sample_to_vec(include_bytes!("../../samples/cowbell.wav"), sample_rate)
                },
                Sample::Kick => {
                    sample_to_vec(include_bytes!("../../samples/kick.wav"), sample_rate)
                },
            },
            sample_rate,
        }
    }

    pub fn get_sound(&self, idx: Sample) -> &[f32] {
        &self.samples[idx]
    }

    pub fn get_sample_rate(&self) -> u32 {
        self.sample_rate
    }
}
