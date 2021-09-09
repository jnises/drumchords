use std::sync::Arc;

use crossbeam::{atomic::AtomicCell, channel};
use fixedbitset::FixedBitSet;
use hound::WavReader;
use num::Integer;
use wmidi::MidiMessage;

const HIHAT: &[u8] = include_bytes!("../samples/hihat.wav");
//const SNARE: &[u8] = include_bytes!("../samples/snare.wav");
// TOOD should it be 11?
const NUM_CHANNELS: usize = 11;
pub const PATTERN_LENGTH: u64 = 32;

type MidiChannel = channel::Receiver<MidiMessage<'static>>;

#[derive(Clone)]
struct NoteEvent {
    note: wmidi::Note,
    velocity: wmidi::U7,
    pressed: u64,
    released: Option<u64>,
}

// TODO handle params using messages instead?
pub struct Params {
    pub gain: AtomicCell<f32>,
}

pub struct Feedback {
    // TODO can we use some doublebuffered thing if we want things larger than can be atomic?
    pub patterns: [AtomicCell<u32>; NUM_CHANNELS],
}

#[derive(Clone)]
struct Sample {
    start_clock: u64,
}

#[derive(Clone)]
pub struct Synth {
    clock: u64,
    midi_events: MidiChannel,

    notes_held: FixedBitSet,
    params: Arc<Params>,
    feedback: Arc<Feedback>,
    samples: [Vec<f32>; 1],
    bpm: u64,
    channels: [Option<Sample>; NUM_CHANNELS],
}

impl Synth {
    pub fn new(midi_events: MidiChannel) -> Self {
        let hihat = WavReader::new(HIHAT)
            .unwrap()
            .into_samples::<i16>()
            .map(|s| s.unwrap() as f32 / i16::MAX as f32)
            .collect();
        // let snare = WavReader::new(SNARE)
        //     .unwrap()
        //     .into_samples::<i16>()
        //     .map(|s| s.unwrap() as f32 / i16::MAX as f32)
        //     .collect();
        Self {
            clock: 0,
            midi_events,
            notes_held: FixedBitSet::with_capacity(128), // TODO Note max?
            params: Arc::new(Params { gain: 1f32.into() }),
            feedback: Arc::new(Feedback {
                patterns: Default::default(),
            }),
            samples: [hihat],//, snare],
            // not your normal bpm
            bpm: 120 * 4,
            channels: Default::default(),
        }
    }

    pub fn get_params(&self) -> Arc<Params> {
        self.params.clone()
    }

    pub fn get_feedback(&self) -> Arc<Feedback> {
        self.feedback.clone()
    }
}

pub trait SynthPlayer {
    fn play(&mut self, sample_rate: u32, channels: usize, output: &mut [f32]);
}

impl SynthPlayer for Synth {
    fn play(&mut self, sample_rate: u32, channels: usize, output: &mut [f32]) {
        // pump midi messages
        for message in self.midi_events.try_iter() {
            match message {
                wmidi::MidiMessage::NoteOn(_, note, _) => {
                    self.notes_held.put(note as usize);
                }
                wmidi::MidiMessage::NoteOff(_, note, _) => {
                    self.notes_held.set(note as usize, false);
                }
                _ => {}
            }
        }

        // produce sound
        let held = &self.notes_held;
        let frames_per_beat = sample_rate as u64 * 60 / self.bpm;
        let gain = self.params.gain.load();
        for frame in output.chunks_exact_mut(channels) {
            let (beat, beat_frame) = self.clock.div_mod_floor(&frames_per_beat);
            if beat_frame == 0 {
                for (chanid, (channel, feedback_pattern)) in self
                    .channels
                    .iter_mut()
                    .zip(self.feedback.patterns.iter())
                    .enumerate()
                {
                    let mut pattern = 0u64;
                    for b in 0..=PATTERN_LENGTH {
                        let mut a = false;
                        for n in chanid * 12..(chanid + 1) * 12 {
                            if held[n] {
                                let nmod = n as u64 % 12;
                                // TODO use different divisors. n2, fib?
                                let div = nmod + 1;
                                let c = (beat - 1 + b) / div & 1 == 0;
                                a = a != c;
                            }
                        }
                        if a {
                            pattern |= 1 << (PATTERN_LENGTH - b);
                        }
                    }
                    let xorpattern = (pattern ^ (pattern >> 1)) as u32;
                    feedback_pattern.store(xorpattern);

                    if xorpattern >> PATTERN_LENGTH - 1 & 1 != 0 {
                        *channel = Some(Sample {
                            start_clock: self.clock,
                        });
                    }
                }
            }

            let mut value = 0f32;
            for (ci, c) in self.channels.iter_mut().enumerate() {
                if let Some(Sample { start_clock }) = *c {
                    let time_sample = self.clock - start_clock;
                    if let Some(&v) = self.samples[0].get(time_sample as usize) {
                        value += v;
                    } else {
                        *c = None;
                    }
                }
            }
            value *= gain;
            for sample in frame.iter_mut() {
                *sample = value;
            }
            self.clock += 1;
        }
    }
}

#[cfg(test)]
mod test {
    use super::{Synth, SynthPlayer};
    use crossbeam::channel;

    #[test]
    fn silence() {
        let (_tx, rx) = channel::bounded(1);
        let mut synth = Synth::new(rx);
        let mut data = [0f32; 512];
        synth.play(48000, 2, &mut data);
        assert_eq!([0f32; 512], data);
    }
}
