mod midi_writer;
pub mod sound_bank;
use itertools::multizip;
use midi_writer::MidiWriter;
use std::sync::Arc;

use anyhow::Result;
use array_init::array_init;
use crossbeam::{atomic::AtomicCell, channel};
use midly::{self, MetaMessage, TrackEvent, TrackEventKind};
use num::Integer;
use static_assertions::const_assert;

const NUM_CHANNELS: usize = 11;
pub const PATTERN_LENGTH: u64 = 32;
pub const NOTES_PER_CHANNEL: u64 = 12;

type MidiChannel = channel::Receiver<wmidi::MidiMessage<'static>>;

// TODO handle params using messages instead?
pub struct Params {
    pub gain_db: AtomicCell<f32>,
    pub locked: [AtomicCell<u16>; NUM_CHANNELS],
    pub bpm: AtomicCell<u32>,
    pub playing: AtomicCell<bool>,
    // TODO assert that this is wide enough
    pub muted: AtomicCell<u64>,
    pub channel_samples: [AtomicCell<sound_bank::Sample>; NUM_CHANNELS],
    pub channel_volumes_db: [AtomicCell<f32>; NUM_CHANNELS],
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
struct TimedClip {
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
                if triggered & (1 << n) != 0 {
                    // TODO use different divisors. n2, fib?
                    let div = n + 1;
                    let c = (b / div) & 1 == 0;
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
        let us_per_beat = (60 * 1_000_000 / self.params.bpm.load()).into();
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
                            _ => panic!("unexpected channel {}", c),
                        }
                        .into();
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
                                    key,
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
    sound_bank: Option<sound_bank::Bank>,

    clock: u64,
    midi_events: MidiChannel,

    config: Arc<Config>,
    playing: [Option<TimedClip>; NUM_CHANNELS],

    lowpass: [f32; NUM_CHANNELS],
}

impl Synth {
    pub fn new(midi_events: MidiChannel) -> Self {
        Self {
            sound_bank: None,
            clock: 0,
            midi_events,
            config: Arc::new(Config {
                params: Params {
                    gain_db: 0f32.into(),
                    locked: Default::default(),
                    bpm: 120.into(),
                    playing: true.into(),
                    muted: 0.into(),
                    channel_samples: array_init(|_| {
                        AtomicCell::new(sound_bank::Sample::HihatClosed)
                    }),
                    channel_volumes_db: array_init(|_| AtomicCell::new(0f32)),
                },
                feedback: Feedback::new(),
                selected: Default::default(),
            }),
            playing: Default::default(),
            lowpass: Default::default(),
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
        // set up samples
        if self.sound_bank.is_none()
            || self.sound_bank.as_ref().unwrap().get_sample_rate() != sample_rate
        {
            self.sound_bank = Some(sound_bank::Bank::new(sample_rate));
        }

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
        let frames_per_beat = sample_rate * 60 / (self.config.params.bpm.load() * 4);
        let gain = 10f32.powf(self.config.params.gain_db.load() / 10f32);
        let muted = self.config.params.muted.load();
        let playing = self.config.params.playing.load();
        for frame in output.chunks_exact_mut(channels) {
            if playing {
                let (beat, beat_frame) = self.clock.div_mod_floor(&frames_per_beat.into());
                if beat_frame == 0 {
                    for channel in 0..NUM_CHANNELS {
                        let mut pattern = 0u32;
                        const_assert!(PATTERN_LENGTH <= 32);
                        for b in 0..PATTERN_LENGTH {
                            if self.config.get_beat(channel, beat + b) {
                                pattern |= 1 << (PATTERN_LENGTH - b - 1);
                            }
                        }
                        self.config.feedback.channels[channel]
                            .pattern
                            .store(pattern);

                        if (pattern >> (PATTERN_LENGTH - 1)) & 1 != 0 {
                            self.playing[channel] = Some(TimedClip {
                                start_clock: self.clock,
                            });
                        }
                    }
                }

                let mut value = 0f32;
                for (i, sample, volume_db, lowpass) in multizip((
                    0..,
                    self.playing.iter_mut(),
                    self.config.params.channel_volumes_db.iter(),
                    self.lowpass.iter_mut(),
                )) {
                    let mut channel_value = 0f32;
                    if (muted >> i) & 1 == 0 {
                        if let Some(TimedClip { start_clock }) = *sample {
                            let time_sample = self.clock - start_clock;
                            if let Some(&v) = self
                                .sound_bank
                                .as_ref()
                                .unwrap()
                                .get_sound(self.config.params.channel_samples[i].load())
                                .get(time_sample as usize)
                            {
                                channel_value = v * 10f32.powf(volume_db.load() / 10f32);
                            } else {
                                *sample = None;
                            }
                        }
                    }
                    // TODO do proper lowpass
                    const LOWPASS_AMOUNT: f32 = 0.1;
                    *lowpass = LOWPASS_AMOUNT * *lowpass + (1f32 - LOWPASS_AMOUNT) * channel_value;
                    value += *lowpass;
                }

                value *= gain;

                for sample in frame.iter_mut() {
                    *sample = value;
                }
            } else {
                for sample in frame.iter_mut() {
                    *sample = 0f32;
                }
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
