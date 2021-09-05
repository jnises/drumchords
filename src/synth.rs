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
    playing: ArrayVec<Sample, 8>,
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
            bpm: 120,
            playing: ArrayVec::new(),
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
                wmidi::MidiMessage::NoteOn(_, note, velocity) => {
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

            }
            // TODO make sample player
            
            let mut value = 0f32;
            for i in 0.. {
                if i >= self.playing.len() {
                    break
                }
                let start_clock = self.playing[i].start_clock;
                asdf
            }
            let mut samplei = 0;
            while samplei < self.playing.len() {

            }
            let beat_frame = self.clock % frames_per_beat;
            if beat_frame == 0 {

            }
            let time = self.clock as f64 / sample_rate as f64;
            // TODO on the frame where a beat starts. 
            let beat = time * self.bpm as f64;
            let time_samples = self.clock - pressed;
            let mut value = self
                .cowbell
                .get(time_samples as usize)
                .copied()
                .unwrap_or(0f32);
            value *= gain;
            // fade in to avoid pop
            // value *= (time * 1000.).min(1.);
            // fade out
            if let Some(released) = released {
                let released_time = (self.clock - released) as f32 / sample_rate as f32;
                value *= (1. - released_time * 1000.).max(0.);
            }
            // TODO also avoid popping when switching between notes
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
