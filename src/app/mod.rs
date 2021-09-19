mod pattern_designer;
mod utils;
use crate::midi::MidiReader;
use crate::periodic_updater::PeriodicUpdater;
use crate::synth::{ChannelFeedback, Config, Synth, PATTERN_LENGTH};
use crate::{
    audio::AudioManager,
    synth::{self, Feedback},
};
use cpal::traits::DeviceTrait;
use crossbeam::channel;
use eframe::egui::{emath, pos2, Rect, Stroke};
use eframe::{
    egui::{self, epaint, vec2, Color32},
    epi::{self, App},
};
use itertools::multizip;
use parking_lot::Mutex;
use pattern_designer::pattern_designer;
use rfd::{FileDialog, MessageDialog, MessageLevel};
use std::{collections::VecDeque, sync::Arc};
use log::warn;

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
    Initialized(Data),
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
        *self = Self::Initialized(Data {
            setting_tab: Setting::Output,
            audio,
            midi,
            status_text,
            forced_buffer_size: None,
            left_vis_buffer: VecDeque::with_capacity(VIS_SIZE * 2),
            synth_config,
            periodic_updater: None,
        });
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
        if let Self::Initialized(Data {
            periodic_updater, ..
        }) = self
        {
            periodic_updater.take();
        }
    }

    fn update(&mut self, ctx: &egui::CtxRef, frame: &mut epi::Frame<'_>) {
        // TODO scrolling
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::auto_sized().show(ui, |ui| {
                ui.heading(NAME);
                match self {
                    Self::Uninitialized => {
                        if ui.button("start").clicked() {
                            self.init();
                        }
                    }
                    Self::Initialized(data) => {
                        // send repaint periodically instead of each frame since the rendering doesn't seem to be vsynced when the window is hidden on mac
                        // TODO stop this when not in focus
                        if data.periodic_updater.is_none() {
                            let repaint_signal = frame.repaint_signal();
                            data.periodic_updater = Some(PeriodicUpdater::new(repaint_signal));
                        }
                        let audio = &mut data.audio;
                        let midi = &data.midi;
                        let left_vis_buffer = &mut data.left_vis_buffer;
                        let forced_buffer_size = &mut data.forced_buffer_size;
                        let status_text = &data.status_text;
                        let config = data.synth_config.as_ref();
                        let setting_tab = &mut data.setting_tab;
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
                        let p = ui.painter();
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
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.label("gain:");
                                let mut gain = config.params.gain.load();
                                ui.add(egui::Slider::new(&mut gain, 0f32..=1f32));
                                config.params.gain.store(gain);
                            });
                        });
                        ui.group(|ui| {
                            ui.label("channels:");
                            ui.vertical(|ui| {
                                for (ChannelFeedback { pattern }, locked, feedback_selected) in
                                    multizip((
                                        config.feedback.channels.iter(),
                                        config.params.locked.iter(),
                                        config.selected.iter(),
                                    ))
                                {
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
                                    });
                                }
                            });
                        });
                        if ui.button("💾").clicked() {
                            match data.synth_config.generate_midi() {
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
                    }
                }
            });
        });
    }
}