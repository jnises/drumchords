use std::{collections::HashSet, f32::consts::PI, sync::Arc};

use arrayvec::ArrayVec;
use crossbeam::{atomic::AtomicCell, channel};
use fixedbitset::FixedBitSet;
use hound::{WavReader, WavSamples};
use num::Integer;
use wmidi::MidiMessage;

const COWBELL: &[u8] = include_bytes!("../samples/cowbell.wav");

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
    cowbell: Vec<f32>,
    bpm: u64,
    channels: [Option<Sample>; 2],
}

impl Synth {
    pub fn new(midi_events: MidiChannel) -> Self {
        let cowbell = WavReader::new(COWBELL)
            .unwrap()
            .into_samples::<i16>()
            .map(|s| s.unwrap() as f32 / i16::MAX as f32)
            .collect();
        Self {
            clock: 0,
            midi_events,
            notes_held: FixedBitSet::with_capacity(127),
            params: Arc::new(Params { gain: 1f32.into() }),
            cowbell,
            // not your normal bpm
            bpm: 240,
            channels: [None, None],
        }
    }

    pub fn get_params(&self) -> Arc<Params> {
        self.params.clone()
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
        let frames_per_beat = sample_rate as u64 * 60 / self.bpm;
        let gain = self.params.gain.load();
        for frame in output.chunks_exact_mut(channels) {
            let (beat, beat_frame) = self.clock.div_mod_floor(&frames_per_beat);
            if beat_frame == 0 {
                for (chanid, channel) in self.channels.iter_mut().enumerate() {
                    let held = &self.notes_held;
                    let f = |b| {
                        let mut a = false;
                        for n in (chanid * 12)..((chanid + 1) * 12) {
                            if held[n] {
                                let c = b & n as u64 != 0;
                                a = a != c;
                            }
                        }
                        a
                    };
                    let beatmod = beat % 24;
                    let prevbeat = (beat + 23) % 24;
                    if f(beatmod) && !f(prevbeat) {
                        *channel = Some(Sample {
                            start_clock: self.clock,
                        });
                    }
                }
            }

            let mut value = 0f32;
            for c in self.channels.iter_mut() {
                if let Some(Sample { start_clock }) = *c {
                    let time_sample = self.clock - start_clock;
                    if let Some(&v) = self.cowbell.get(time_sample as usize) {
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
