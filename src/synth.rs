use std::sync::Arc;

use crossbeam::{atomic::AtomicCell, channel};
use fixedbitset::FixedBitSet;
use hound::WavReader;
use num::Integer;
use wmidi::MidiMessage;

const HIHAT: &[u8] = include_bytes!("../samples/hihat.wav");
const SNARE: &[u8] = include_bytes!("../samples/snare.wav");

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

pub const PATTERN_LENGTH: u64 = 64;

pub struct Feedback {
    // TODO can we use some doublebuffered thing if we want things larger than can be atomic?
    pub patterns: [AtomicCell<u64>; 2],
    pub beat: AtomicCell<u64>,
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
    samples: [Vec<f32>; 2],
    bpm: u64,
    channels: [Option<Sample>; 2],
}

impl Synth {
    pub fn new(midi_events: MidiChannel) -> Self {
        let hihat = WavReader::new(HIHAT)
            .unwrap()
            .into_samples::<i16>()
            .map(|s| s.unwrap() as f32 / i16::MAX as f32)
            .collect();
        let snare = WavReader::new(SNARE)
            .unwrap()
            .into_samples::<i16>()
            .map(|s| s.unwrap() as f32 / i16::MAX as f32)
            .collect();
        Self {
            clock: 0,
            midi_events,
            notes_held: FixedBitSet::with_capacity(127),
            params: Arc::new(Params { gain: 1f32.into() }),
            feedback: Arc::new(Feedback {
                patterns: [0.into(), 0.into()],
                beat: 0.into(),
            }),
            samples: [hihat, snare],
            // not your normal bpm
            bpm: 120 * 16,
            channels: [None, None],
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

        // TODO only do this on demand?
        let mut patterns = [0; 2];
        let held = &self.notes_held;
        for (pattid, pattern) in patterns.iter_mut().enumerate() {
            for beat in 0..PATTERN_LENGTH {
                let mut a = false;
                for n in (pattid * 12)..((pattid + 1) * 12) {
                    if held[n] {
                        let nmod = n as u64 % 12;
                        // update this depending on the pattern length
                        let div = match nmod {
                            0 => 64,
                            2 => 32,
                            4 => 16,
                            5 => 8,
                            7 => 4,
                            9 => 2,
                            11 => 1,
                            _ => break,
                        };
                        let c = beat / div & 1 == 0;
                        a = a != c;
                    }
                }
                if a {
                    *pattern |= 1 << (PATTERN_LENGTH - 1 - beat);
                }
            }
        }
        for pattern in patterns.iter_mut() {
            let rot = *pattern >> 1 | *pattern << (PATTERN_LENGTH - 1);
            *pattern &= !rot;
        }

        for (&src, dst) in patterns.iter().zip(self.feedback.patterns.iter()) {
            dst.store(src);
        }

        // produce sound
        let frames_per_beat = sample_rate as u64 * 60 / self.bpm;
        let gain = self.params.gain.load();
        for frame in output.chunks_exact_mut(channels) {
            let (beat, beat_frame) = self.clock.div_mod_floor(&frames_per_beat);
            if beat_frame == 0 {
                let beatmod = beat % PATTERN_LENGTH;
                self.feedback.beat.store(beatmod);
                for (chanid, channel) in self.channels.iter_mut().enumerate() {
                    // TODO zip instead
                    let p = patterns[chanid];
                    if p >> (PATTERN_LENGTH - 1) - beatmod & 1 != 0 {
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
                    if let Some(&v) = self.samples[ci].get(time_sample as usize) {
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
