mod pattern_designer;
mod toggle;
mod utils;
use crate::midi::MidiReader;
use crate::periodic_updater::PeriodicUpdater;
use crate::synth::{ChannelFeedback, Synth, PATTERN_LENGTH};
use crate::{audio::AudioManager, synth};
use cpal::traits::DeviceTrait;
use crossbeam::channel;
use eframe::egui::{emath, pos2, ComboBox, Rect, Stroke};
use eframe::{
    egui::{self, epaint, vec2, Color32},
    epi::{self, App},
};
use itertools::multizip;
use log::warn;
use parking_lot::Mutex;
use pattern_designer::pattern_designer;
use rfd::{MessageDialog, MessageLevel};
use std::{collections::VecDeque, sync::Arc};

const NAME: &str = "Drumchords";
const VIS_SIZE: usize = 512;

#[derive(PartialEq)]
enum Setting {
    Input,
    Output,
}

pub struct Data {
    setting_tab: Setting,
    audio: AudioManager<Synth>,
    midi: Arc<MidiReader>,
    status_text: Arc<Mutex<String>>,
    forced_buffer_size: Option<u32>,
    left_vis_buffer: VecDeque<f32>,
    synth_config: Arc<synth::Config>,
    periodic_updater: Option<PeriodicUpdater>,
}

pub enum Drumchords {
    Initialized(Box<Data>),
    Uninitialized,
}

impl Drumchords {
    pub fn init(&mut self) {
        let (midi_tx, midi_rx) = channel::bounded(256);
        let midi = MidiReader::new(midi_tx.clone());
        let synth = Synth::new(midi_rx);
        let status_text = Arc::new(Mutex::new("".to_string()));
        let synth_config = synth.get_config();
        let status_clone = status_text.clone();
        let audio = AudioManager::new(synth, move |e| {
            *status_clone.lock() = e;
        });
        *self = Self::Initialized(Box::new(Data {
            setting_tab: Setting::Output,
            audio,
            midi,
            status_text,
            forced_buffer_size: None,
            left_vis_buffer: VecDeque::with_capacity(VIS_SIZE * 2),
            synth_config,
            periodic_updater: None,
        }));
    }

    pub fn new() -> Self {
        let mut s = Self::Uninitialized;
        // need to defer initializion in wasm due to chrome's autoplay blocking and such
        if cfg!(not(target_arch = "wasm32")) {
            s.init();
        }
        s
    }
}

impl App for Drumchords {
    fn name(&self) -> &str {
        NAME
    }

    fn on_exit(&mut self) {
        if let Self::Initialized(data) = self {
            data.periodic_updater.take();
        }
    }

    fn update(&mut self, ctx: &egui::CtxRef, frame: &epi::Frame) {
        // TODO scrolling
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading(NAME);
                match self {
                    Self::Uninitialized => {
                        // TODO make button larger
                        if ui.button("▶").clicked() {
                            self.init();
                        }
                    }
                    Self::Initialized(data) => {
                        // send repaint periodically instead of each frame since the rendering doesn't seem to be vsynced when the window is hidden on mac
                        // TODO stop this when not in focus
                        if data.periodic_updater.is_none() {
                            data.periodic_updater = Some(PeriodicUpdater::new(frame.clone()));
                        }
                        // TODO nicer to use destructuring here?
                        let audio = &mut data.audio;
                        let midi = &data.midi;
                        let left_vis_buffer = &mut data.left_vis_buffer;
                        let forced_buffer_size = &mut data.forced_buffer_size;
                        let status_text = &data.status_text;
                        let config = data.synth_config.as_ref();
                        let setting_tab = &mut data.setting_tab;
                        let synth_config = &data.synth_config;
                        ui.horizontal(|ui| {
                            let mut playing = config.params.playing.load();
                            ui.selectable_value(&mut playing, true, "▶");
                            ui.selectable_value(&mut playing, false, "⏹");
                            config.params.playing.store(playing);

                            if ui.button("💾").clicked() {
                                match synth_config.generate_midi() {
                                    Ok(midi) => {
                                        utils::save_midi_file(&midi);
                                    }
                                    Err(e) => {
                                        warn!("{:?}", e);
                                        let _ = MessageDialog::new()
                                            .set_level(MessageLevel::Error)
                                            .set_title("midi export error")
                                            .set_description(&e.to_string())
                                            .show();
                                    }
                                }
                            }
                        });
                        ui.collapsing("settings:", |ui| {
                            ui.horizontal(|ui| {
                                ui.selectable_value(setting_tab, Setting::Input, "input");
                                ui.selectable_value(setting_tab, Setting::Output, "output");
                            });
                            ui.separator();
                            match setting_tab {
                                Setting::Input => {
                                    ui.horizontal(|ui| {
                                        ui.label("midi:");
                                        ui.label(midi.get_name());
                                    });
                                }
                                Setting::Output => {
                                    ui.horizontal(|ui| {
                                        ui.label("device:");
                                        let mut selected =
                                            audio.get_name().unwrap_or_else(|| "-".to_string());
                                        egui::ComboBox::from_id_source("audio combo box")
                                            .selected_text(&selected)
                                            .show_ui(ui, |ui| {
                                                // TODO cache this to not poll too often
                                                for device in audio.get_devices() {
                                                    if let Ok(name) = device.name() {
                                                        ui.selectable_value(
                                                            &mut selected,
                                                            name.clone(),
                                                            name,
                                                        );
                                                    }
                                                }
                                            });
                                        if Some(&selected) != audio.get_name().as_ref() {
                                            if let Some(device) =
                                                audio.get_devices().into_iter().find(|d| {
                                                    if let Ok(name) = d.name() {
                                                        name == selected
                                                    } else {
                                                        false
                                                    }
                                                })
                                            {
                                                audio.set_device(device);
                                            }
                                        }
                                    });
                                    let buffer_range = audio.get_buffer_size_range();
                                    ui.horizontal(|ui| {
                                        ui.label("buffer size:");
                                        ui.group(|ui| {
                                            if buffer_range.is_none() {
                                                ui.set_enabled(false);
                                                *forced_buffer_size = None;
                                            }
                                            let mut forced = forced_buffer_size.is_some();
                                            ui.horizontal(|ui| {
                                                ui.checkbox(&mut forced, "force");
                                                ui.set_enabled(forced);
                                                let mut size = match forced_buffer_size.to_owned() {
                                                    Some(size) => size,
                                                    None => audio.get_buffer_size().unwrap_or(0),
                                                };
                                                let range = match buffer_range {
                                                    // limit max to something sensible
                                                    Some((min, max)) => min..=max.min(16384),
                                                    None => 0..=1,
                                                };
                                                ui.add(egui::Slider::new(&mut size, range));
                                                if forced {
                                                    *forced_buffer_size = Some(size);
                                                } else {
                                                    *forced_buffer_size = None;
                                                }
                                                audio.set_forced_buffer_size(*forced_buffer_size);
                                            });
                                        });
                                    });
                                }
                            };
                            ui.label(&*status_text.lock());
                        });

                        audio.pop_each_left_vis_buffer(|value| {
                            left_vis_buffer.push_back(value);
                        });

                        let mut prev = None;
                        let mut it = left_vis_buffer.iter().copied().rev();
                        it.nth(VIS_SIZE / 2 - 1);
                        for value in &mut it {
                            if let Some(prev) = prev {
                                if prev >= 0. && value < 0. {
                                    break;
                                }
                            }
                            prev = Some(value);
                        }
                        let plot_width = ui.available_width().min(300.);
                        let (_, rect) = ui.allocate_space(vec2(plot_width, plot_width * 0.5));
                        let p = ui.painter_at(rect);
                        p.rect_filled(rect, 10f32, Color32::BLACK);
                        let to_rect = emath::RectTransform::from_to(
                            Rect::from_x_y_ranges(0.0..=(VIS_SIZE / 2) as f32, -1.0..=1.0),
                            rect,
                        );
                        p.add(epaint::Shape::line(
                            it.take(VIS_SIZE / 2)
                                .enumerate()
                                .map(|(x, y)| to_rect * pos2(x as f32, y))
                                .collect(),
                            Stroke::new(1f32, Color32::GRAY),
                        ));
                        if left_vis_buffer.len() > VIS_SIZE {
                            drop(left_vis_buffer.drain(0..left_vis_buffer.len() - VIS_SIZE));
                        }
                        ui.horizontal(|ui| {
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label("bpm:");
                                    let mut bpm = config.params.bpm.load();
                                    // TODO make fixed size
                                    ui.add(
                                        egui::DragValue::new(&mut bpm)
                                            .speed(1)
                                            .clamp_range(1..=1000)
                                            .max_decimals(0),
                                    );
                                    config.params.bpm.store(bpm);
                                });
                            });
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label("gain:");
                                    let mut gain = config.params.gain_db.load();
                                    // TODO make fixed size
                                    ui.add(
                                        egui::DragValue::new(&mut gain)
                                            .suffix("dB")
                                            .speed(0.1)
                                            .min_decimals(1),
                                    );
                                    config.params.gain_db.store(gain);
                                });
                            });
                        });
                        ui.group(|ui| {
                            ui.label("channels:");
                            ui.vertical(|ui| {
                                let mut muted = config.params.muted.load();
                                for (
                                    channel_id,
                                    ChannelFeedback { pattern },
                                    locked,
                                    feedback_selected,
                                    selected_sound_atomic,
                                    volume_atomic,
                                ) in multizip((
                                    0..,
                                    config.feedback.channels.iter(),
                                    config.params.locked.iter(),
                                    config.selected.iter(),
                                    config.params.channel_samples.iter(),
                                    config.params.channel_volumes_db.iter(),
                                )) {
                                    ui.horizontal(|ui| {
                                        {
                                            let pattern = pattern.load();
                                            let cell_width = 4f32;
                                            let cell_height = 8f32;
                                            let horizontal_spacing = 1f32;
                                            let (_id, rect) = ui.allocate_space(vec2(
                                                (cell_width + horizontal_spacing)
                                                    * PATTERN_LENGTH as f32,
                                                cell_height,
                                            ));
                                            let painter = ui.painter_at(rect);
                                            let mut r = rect;
                                            r.set_right(r.left() + cell_width);
                                            for i in 0..PATTERN_LENGTH {
                                                let filled =
                                                    pattern >> (PATTERN_LENGTH - 1 - i) & 1 != 0;
                                                let color = if filled {
                                                    if i == 0 {
                                                        Color32::RED
                                                    } else {
                                                        Color32::WHITE
                                                    }
                                                } else {
                                                    Color32::BLACK
                                                };
                                                painter.rect_filled(r, 1f32, color);
                                                r = r.translate(vec2(cell_width + 1., 0f32));
                                            }
                                        }
                                        let mut fg_pattern = locked.load();
                                        let bg_pattern = feedback_selected.load();
                                        pattern_designer(
                                            ui,
                                            &mut fg_pattern,
                                            bg_pattern,
                                            synth::NOTES_PER_CHANNEL,
                                        );
                                        locked.store(fg_pattern);

                                        // mute toggle
                                        let mut channel_muted = muted >> channel_id & 1 != 0;
                                        toggle::toggle(ui, &mut channel_muted, "🔇");
                                        muted = muted & !(1 << channel_id)
                                            | u64::from(channel_muted);

                                        // volume
                                        let mut volume = volume_atomic.load();
                                        // TODO make fixed size
                                        ui.add(
                                            egui::DragValue::new(&mut volume)
                                                .suffix("dB")
                                                .speed(0.1)
                                                .min_decimals(1),
                                        );
                                        volume_atomic.store(volume);

                                        // sample selector
                                        let mut selected_sound = selected_sound_atomic.load();
                                        ComboBox::from_id_source(
                                            egui::Id::new(channel_id).with("sample_combo"),
                                        )
                                        .selected_text(selected_sound.to_string())
                                        .width(70f32)
                                        .show_ui(
                                            ui,
                                            |ui| {
                                                for s in
                                                    enum_iterator::all::<synth::sound_bank::Sample>(
                                                    )
                                                {
                                                    ui.selectable_value(
                                                        &mut selected_sound,
                                                        s,
                                                        s.to_string(),
                                                    );
                                                }
                                            },
                                        );
                                        selected_sound_atomic.store(selected_sound);
                                    });
                                }
                                config.params.muted.store(muted);
                            });
                        });
                    }
                }
            });
        });
    }
}
