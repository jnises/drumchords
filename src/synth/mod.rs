mod midi_writer;
use midi_writer::MidiWriter;
use std::{
    convert::{TryFrom, TryInto},
    sync::Arc,
};

use anyhow::Result;
use crossbeam::{atomic::AtomicCell, channel};
use hound::WavReader;
use midly::{self, num::u7, MetaMessage, TrackEvent, TrackEventKind};
use num::Integer;
use wmidi;

const HIHAT: &[u8] = include_bytes!("../../samples/hihat.wav");
//const SNARE: &[u8] = include_bytes!("../samples/snare.wav");
const NUM_CHANNELS: usize = 11;
pub const PATTERN_LENGTH: u64 = 32;
const NUM_NOTES: usize = 128;
pub const NOTES_PER_CHANNEL: u64 = 12;

type MidiChannel = channel::Receiver<wmidi::MidiMessage<'static>>;

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
    pub locked: [AtomicCell<u16>; NUM_CHANNELS],
    pub bpm: AtomicCell<u32>,
}

#[derive(Default)]
pub struct ChannelFeedback {
    pub pattern: AtomicCell<u32>,
}

pub struct Feedback {
    pub channels: [ChannelFeedback; NUM_CHANNELS],
}

impl Feedback {
    fn new() -> Self {
        Self {
            channels: Default::default(),
        }
    }
}

#[derive(Clone)]
struct Sample {
    start_clock: u64,
}

pub struct Config {
    pub params: Params,
    pub feedback: Feedback,
    pub selected: [AtomicCell<u16>; NUM_CHANNELS],
}

impl Config {
    fn get_beat(&self, channel: usize, beat: u64) -> bool {
        // TODO don't load here. make copy of config to use to generate a pattern or midi?
        let selected = self.selected[channel].load();
        let locked = self.params.locked[channel].load();
        let triggered = selected | locked;
        let f = |b| {
            let mut a = false;
            for n in 0..NOTES_PER_CHANNEL {
                if triggered & 1 << n != 0 {
                    // TODO use different divisors. n2, fib?
                    let div = n + 1;
                    let c = b / div & 1 == 0;
                    a = a != c;
                }
            }
            a
        };
        f(beat) != f(beat.wrapping_sub(1))
    }

    // TODO run this on a web worker to not block the main thread
    pub fn generate_midi(&self) -> Result<Vec<u8>> {
        // TODO proper tempo
        let ticks_per_beat = 4;
        let mut smf = midly::Smf::new(midly::Header::new(
            midly::Format::SingleTrack,
            midly::Timing::Metrical(ticks_per_beat.into()),
        ));
        let mut track = vec![];
        let us_per_beat = (60 * 1_000_000 / self.params.bpm.load() * 4).try_into()?;
        track.push(TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Meta(MetaMessage::Tempo(us_per_beat)),
        });
        {
            let mut writer = MidiWriter::new(&mut track);
            // TODO some other length
            for b in 0..1024 {
                for c in 0..NUM_CHANNELS {
                    if self.get_beat(c, b) {
                        let key = match c {
                            // c4
                            0 => 60,
                            1 => 62,
                            2 => 64,
                            3 => 65,
                            4 => 67,
                            5 => 69,
                            6 => 71,
                            // c5
                            7 => 72,
                            8 => 74,
                            9 => 76,
                            10 => 77,
                            _ => panic!("unexpected channel {}", c)
                        }.into();
                        writer.add_event(midi_writer::Event {
                            tick: b,
                            kind: TrackEventKind::Midi {
                                channel: 0.into(),
                                message: midly::MidiMessage::NoteOn {
                                    vel: 127.into(),
                                    key,
                                },
                            },
                        });
                        // TODO some other note length?
                        writer.add_event(midi_writer::Event {
                            tick: b + 1,
                            kind: TrackEventKind::Midi {
                                channel: 0.into(),
                                message: midly::MidiMessage::NoteOff {
                                    vel: 127.into(),
                                    key: u8::try_from(c)?.try_into()?,
                                },
                            },
                        });
                    }
                }
            }
            writer.flush();
        }
        track.push(TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
        });
        smf.tracks.push(track);
        let mut buf = Vec::new();
        smf.write(&mut buf).unwrap();
        Ok(buf)
    }
}

#[derive(Clone)]
pub struct Synth {
    samples: [Vec<f32>; 1],

    clock: u64,
    midi_events: MidiChannel,

    config: Arc<Config>,
    playing: [Option<Sample>; NUM_CHANNELS],
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
            samples: [hihat], //, snare],
            clock: 0,
            midi_events,
            config: Arc::new(Config {
                params: Params {
                    gain: 1f32.into(),
                    locked: Default::default(),
                    // not your normal bpm
                    // TODO do it normal instead. but what to call it?
                    bpm: (120 * 4).into(),
                },
                feedback: Feedback::new(),
                selected: Default::default(),
            }),
            playing: Default::default(),
        }
    }

    pub fn get_config(&self) -> Arc<Config> {
        self.config.clone()
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
                    let (quot, rem) = (note as usize).div_mod_floor(&(NOTES_PER_CHANNEL as usize));
                    self.config.selected[quot].fetch_xor(1 << rem);
                }
                wmidi::MidiMessage::NoteOff(_, note, _) => {
                    let (quot, rem) = (note as usize).div_mod_floor(&(NOTES_PER_CHANNEL as usize));
                    self.config.selected[quot].fetch_and(!(1 << rem));
                }
                _ => {}
            }
        }

        // produce sound
        let frames_per_beat = sample_rate * 60 / self.config.params.bpm.load();
        let gain = self.config.params.gain.load();
        for frame in output.chunks_exact_mut(channels) {
            let (beat, beat_frame) = self.clock.div_mod_floor(&frames_per_beat.into());
            if beat_frame == 0 {
                for channel in 0..NUM_CHANNELS {
                    let mut pattern = 0u32;
                    // TODO static assert?
                    debug_assert!(PATTERN_LENGTH <= 32);
                    for b in 0..PATTERN_LENGTH {
                        if self.config.get_beat(channel, beat + b) {
                            pattern |= 1 << (PATTERN_LENGTH - b - 1);
                        }
                    }
                    self.config.feedback.channels[channel]
                        .pattern
                        .store(pattern);

                    if pattern >> PATTERN_LENGTH - 1 & 1 != 0 {
                        self.playing[channel] = Some(Sample {
                            start_clock: self.clock,
                        });
                    }
                }
            }

            let mut value = 0f32;
            for sample in self.playing.iter_mut() {
                if let Some(Sample { start_clock }) = *sample {
                    let time_sample = self.clock - start_clock;
                    if let Some(&v) = self.samples[0].get(time_sample as usize) {
                        value += v;
                    } else {
                        *sample = None;
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
