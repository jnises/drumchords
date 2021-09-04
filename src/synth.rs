use std::{f32::consts::PI, sync::Arc};

use crossbeam::{atomic::AtomicCell, channel};
use hound::{WavReader, WavSamples};
use wmidi::MidiMessage;
const COWBELL: &[u8] = include_bytes!("../samples/cowbell.wav");

// super simple synth
// TODO make interesting

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
pub struct Synth {
    clock: u64,
    midi_events: MidiChannel,

    note_event: Option<NoteEvent>,
    params: Arc<Params>,
    cowbell: Vec<f32>,
}

impl Synth {
    pub fn new(midi_events: MidiChannel) -> Self {
        let cowbell = WavReader::new(COWBELL).unwrap().into_samples::<i16>().map(|s| s.unwrap() as f32 / i16::MAX as f32).collect();
        Self {
            clock: 0,
            midi_events,
            note_event: None,
            params: Arc::new(Params {
                gain: 1f32.into(),
            }),
            cowbell,
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
                    self.note_event = Some(NoteEvent {
                        note,
                        velocity,
                        pressed: self.clock,
                        released: None,
                    });
                }
                wmidi::MidiMessage::NoteOff(_, note, _) => {
                    if let Some(NoteEvent {
                        note: held_note,
                        ref mut released,
                        ..
                    }) = self.note_event
                    {
                        if note == held_note {
                            *released = Some(self.clock);
                        }
                    }
                }
                _ => {}
            }
        }

        // produce sound
        if let Some(NoteEvent {
            note,
            velocity,
            pressed,
            released,
        }) = self.note_event
        {
            let gain = self.params.gain.load();
            let norm_vel = (u8::from(velocity) - u8::from(wmidi::U7::MIN)) as f32
                / (u8::from(wmidi::U7::MAX) - u8::from(wmidi::U7::MIN)) as f32;
            let gain = gain * norm_vel;
            for frame in output.chunks_exact_mut(channels) {
                let time_samples = self.clock - pressed;
                let mut value = self.cowbell.get(time_samples as usize).copied().unwrap_or(0f32);
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
        } else {
            output.fill(0f32);
            self.clock += (output.len() / channels) as u64;
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
