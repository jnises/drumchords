use enum_map::{enum_map, EnumMap};
use hound::WavReader;
use strum_macros::Display;
use enum_iterator::IntoEnumIterator;

#[derive(Copy, Clone, enum_map::Enum, Display, IntoEnumIterator, PartialEq)]
#[repr(u8)]
pub enum Sample {
    Hihat,
    Snare,
}

#[derive(Clone)]
pub struct Bank {
    samples: EnumMap<Sample, Vec<f32>>,
}

impl Bank {
    pub fn new() -> Self {
        Bank {
            samples: enum_map! {
                Sample::Hihat => {
                    WavReader::new(include_bytes!("../../samples/hihat.wav") as &[u8])
                    .unwrap()
                    .into_samples::<i16>()
                    .map(|s| s.unwrap() as f32 / i16::MAX as f32)
                    .collect()
                },
                Sample::Snare => {
                    WavReader::new(include_bytes!("../../samples/snare.wav") as &[u8])
                    .unwrap()
                    .into_samples::<i16>()
                    .map(|s| s.unwrap() as f32 / i16::MAX as f32)
                    .collect()
                },
            },
        }
    }

    pub fn get_sound(&self, idx: Sample) -> &[f32] {
        &self.samples[idx]
    }
}
