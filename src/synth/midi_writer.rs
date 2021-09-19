use midly::{num::u28, MetaMessage, TrackEvent, TrackEventKind};
use std::{
    cmp::Ordering,
    collections::BinaryHeap,
    convert::{TryFrom, TryInto},
};

#[derive(PartialEq, Eq)]
pub struct Event<'a> {
    pub tick: u64,
    pub kind: TrackEventKind<'a>,
}

impl Ord for Event<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // TODO for same tick sort noteon before noteoff
        // reverse order since we want smallest ticks first
        other.tick.cmp(&self.tick).then_with(|| {
            match self.kind {
                TrackEventKind::Midi { channel, message } => match other.kind {
                    TrackEventKind::Midi {
                        channel: other_channel,
                        message: other_message,
                    } => other_channel.cmp(&channel).then_with(|| match message {
                        // note off goes before everything else
                        midly::MidiMessage::NoteOff { key, vel } => match other_message {
                            midly::MidiMessage::NoteOff {
                                key: other_key,
                                vel: other_vel,
                            } => key.cmp(&other_key).then_with(|| vel.cmp(&other_vel)),
                            midly::MidiMessage::NoteOn { .. } => Ordering::Greater,
                            _ => Ordering::Greater,
                        },
                        // note on goes last
                        midly::MidiMessage::NoteOn { key, vel } => match other_message {
                            midly::MidiMessage::NoteOff { .. } => Ordering::Less,
                            midly::MidiMessage::NoteOn {
                                key: other_key,
                                vel: other_vel,
                            } => key.cmp(&other_key).then_with(|| vel.cmp(&other_vel)),
                            _ => Ordering::Less,
                        },
                        _ => match other_message {
                            // noteoff goes before non note events
                            midly::MidiMessage::NoteOff { .. } => Ordering::Less,
                            _ => Ordering::Greater,
                        },
                    }),
                    _ => Ordering::Equal,
                },
                // non midi events goes first
                _ => Ordering::Greater,
            }
        })
    }
}

impl PartialOrd for Event<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

pub struct MidiWriter<'a, 'b> {
    track: &'a mut Vec<TrackEvent<'b>>,
    heap: BinaryHeap<Event<'b>>,
    last_tick: u64,
}

impl<'a, 'b> MidiWriter<'a, 'b> {
    pub fn new(track: &'a mut Vec<TrackEvent<'b>>) -> Self {
        Self {
            track,
            heap: BinaryHeap::new(),
            last_tick: 0,
        }
    }

    pub fn add_event(&mut self, event: Event<'b>) {
        self.heap.push(event);
        // TODO flush as much as possible?
    }

    pub fn flush(mut self) {
        // TODO reserve in track
        while let Some(Event { tick, kind }) = self.heap.pop() {
            self.track.push(TrackEvent {
                delta: u28::try_from(u32::try_from(tick - self.last_tick).unwrap()).unwrap(),
                kind: kind,
            });
            self.last_tick = tick;
        }
    }
}
