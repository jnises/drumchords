use enum_iterator::IntoEnumIterator;
use enum_map::{enum_map, EnumMap};
use hound::WavReader;
use strum_macros::Display;

#[derive(Copy, Clone, enum_map::Enum, Display, IntoEnumIterator, PartialEq)]
#[repr(u8)]
pub enum Sample {
    Hihat,
    Snare,
    Cowbell,
}

#[derive(Clone)]
pub struct Bank {
    samples: EnumMap<Sample, Vec<f32>>,
}

fn sample_to_vec(data: &[u8]) -> Vec<f32> {
    WavReader::new(data)
        .unwrap()
        .into_samples::<i16>()
        .map(|s| s.unwrap() as f32 / i16::MAX as f32)
        .collect()
}

impl Bank {
    pub fn new() -> Self {
        Bank {
            samples: enum_map! {
                Sample::Hihat => {
                    sample_to_vec(include_bytes!("../../samples/hihat.wav"))
                },
                Sample::Snare => {
                    sample_to_vec(include_bytes!("../../samples/snare.wav"))
                },
                Sample::Cowbell => {
                    sample_to_vec(include_bytes!("../../samples/cowbell.wav"))
                },
            },
        }
    }

    pub fn get_sound(&self, idx: Sample) -> &[f32] {
        &self.samples[idx]
    }
}
