//! The MIDI test panel: a piano and a row of controls covering the message
//! shapes a host has to parse, including the awkward ones.

use crate::midi_out::{self, MidiOut, Relative};
use nih_plug_egui::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2,
};
use std::f32::consts::PI;
use std::sync::atomic::{AtomicU32, Ordering};

pub const ACCENT: Color32 = Color32::from_rgb(232, 176, 48);
const KNOB_BG: Color32 = Color32::from_rgb(52, 52, 58);
const KEY_WHITE: Color32 = Color32::from_rgb(232, 232, 228);
const KEY_BLACK: Color32 = Color32::from_rgb(30, 30, 34);
/// Pixels of drag per encoder step.
const ENCODER_PX: f32 = 6.0;

/// Packs the note the piano holds for the audio thread: 0 is silence,
/// otherwise `(note + 1) << 8 | velocity`.
pub fn pack_preview(note: Option<(u8, u8)>) -> u32 {
    note.map_or(0, |(n, v)| (n as u32 + 1) << 8 | v as u32)
}

pub fn unpack_preview(packed: u32) -> Option<(u8, u8)> {
    (packed != 0).then(|| (((packed >> 8) - 1) as u8, (packed & 0xFF) as u8))
}

struct Encoder {
    format: Relative,
    number: u8,
    angle: f32,
    drag: f32,
    step: i32,
    accelerate: bool,
}

/// GUI-only state; nothing here reaches the audio thread except the preview note.
pub struct Panel {
    channel: u8,
    velocity: u8,
    octave: i32,
    zero_velocity_off: bool,
    held: Option<u8>,
    knob: f32,
    knob_cc: u8,
    mod_wheel: f32,
    bend: f32,
    pressure: f32,
    momentary_cc: u8,
    momentary_down: bool,
    latch_cc: u8,
    latch_on: bool,
    sustain_down: bool,
    encoders: [Encoder; 3],
}

impl Default for Panel {
    fn default() -> Self {
        let encoder = |format, number| Encoder {
            format,
            number,
            angle: 0.0,
            drag: 0.0,
            step: 1,
            accelerate: false,
        };
        Self {
            channel: 0,
            velocity: 100,
            octave: 4,
            zero_velocity_off: false,
            held: None,
            knob: 0.0,
            knob_cc: 7,
            mod_wheel: 0.0,
            bend: 0.5,
            pressure: 0.0,
            momentary_cc: 20,
            momentary_down: false,
            latch_cc: 21,
            latch_on: false,
            sustain_down: false,
            encoders: [
                encoder(Relative::Offset64, 30),
                encoder(Relative::TwosComplement, 31),
                encoder(Relative::SignBit, 32),
            ],
        }
    }
}

fn to7(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 127.0).round() as u8
}

impl Panel {
    pub fn show(&mut self, ui: &mut Ui, out: &mut MidiOut, preview: &AtomicU32) {
        ui.horizontal(|ui| {
            let (dot, text) = if out.is_open() {
                (ACCENT, format!("port: {}", out.name))
            } else {
                (Color32::from_rgb(200, 80, 70), "port: not open".to_string())
            };
            ui.colored_label(dot, "●");
            ui.label(text);
        });
        if let Some(e) = &out.error {
            ui.colored_label(Color32::from_rgb(220, 110, 90), e);
        }

        ui.horizontal(|ui| {
            ui.label("Channel");
            let mut ch = self.channel + 1;
            ui.add(egui::DragValue::new(&mut ch).range(1..=16));
            self.channel = ch - 1;
            ui.separator();
            ui.label("Velocity");
            ui.add(egui::DragValue::new(&mut self.velocity).range(1..=127));
        });
        ui.horizontal(|ui| {
            if ui.button("Oct -").clicked() {
                self.octave = (self.octave - 1).max(0);
            }
            ui.label(format!("C{}", self.octave));
            if ui.button("Oct +").clicked() {
                self.octave = (self.octave + 1).min(8);
            }
            ui.checkbox(&mut self.zero_velocity_off, "Release as note-on vel 0");
        });

        self.piano(ui, out, preview);
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                if knob(ui, &mut self.knob, &format!("CC {}", self.knob_cc)) {
                    out.send(&midi_out::cc(self.channel, self.knob_cc, to7(self.knob)));
                }
                ui.add(
                    egui::DragValue::new(&mut self.knob_cc)
                        .range(0..=127)
                        .prefix("CC "),
                );
            });
            ui.vertical(|ui| {
                if knob(ui, &mut self.mod_wheel, "Mod (CC 1)") {
                    out.send(&midi_out::cc(self.channel, 1, to7(self.mod_wheel)));
                }
            });
            ui.vertical(|ui| {
                if knob(ui, &mut self.pressure, "Pressure") {
                    out.send(&midi_out::channel_pressure(
                        self.channel,
                        to7(self.pressure),
                    ));
                }
            });
            ui.vertical(|ui| {
                let before = self.bend;
                knob(ui, &mut self.bend, "Pitch bend");
                if ui.small_button("centre").clicked() {
                    self.bend = 0.5;
                }
                if self.bend != before {
                    let v = (self.bend * 16383.0).round() as u16;
                    out.send(&midi_out::pitch_bend(self.channel, v));
                }
            });
        });
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            if let Some(down) = hold_button(
                ui,
                &format!("Hold CC {}", self.momentary_cc),
                &mut self.momentary_down,
            ) {
                out.send(&midi_out::cc(
                    self.channel,
                    self.momentary_cc,
                    if down { 127 } else { 0 },
                ));
            }
            ui.add(egui::DragValue::new(&mut self.momentary_cc).range(0..=127));
            ui.separator();
            let label = format!(
                "Latch CC {} {}",
                self.latch_cc,
                if self.latch_on { "on" } else { "off" }
            );
            if ui
                .add(egui::Button::new(label).selected(self.latch_on))
                .clicked()
            {
                self.latch_on = !self.latch_on;
                out.send(&midi_out::cc(
                    self.channel,
                    self.latch_cc,
                    if self.latch_on { 127 } else { 0 },
                ));
            }
            ui.add(egui::DragValue::new(&mut self.latch_cc).range(0..=127));
            ui.separator();
            if let Some(down) = hold_button(ui, "Sustain (CC 64)", &mut self.sustain_down) {
                out.send(&midi_out::cc(self.channel, 64, if down { 127 } else { 0 }));
            }
        });
        ui.add_space(6.0);

        ui.label("Endless encoders (relative CC)");
        ui.horizontal(|ui| {
            let channel = self.channel;
            for (i, enc) in self.encoders.iter_mut().enumerate() {
                ui.vertical(|ui| {
                    let steps = encoder(ui, enc);
                    if let Some(v) = enc.format.encode(steps) {
                        out.send(&midi_out::cc(channel, enc.number, v));
                    }
                    egui::ComboBox::from_id_salt(("encoder-format", i))
                        .selected_text(enc.format.label())
                        .show_ui(ui, |ui| {
                            for f in Relative::ALL {
                                ui.selectable_value(&mut enc.format, f, f.label());
                            }
                        });
                    ui.add(
                        egui::DragValue::new(&mut enc.number)
                            .range(0..=127)
                            .prefix("CC "),
                    );
                    ui.add(
                        egui::DragValue::new(&mut enc.step)
                            .range(1..=16)
                            .prefix("step "),
                    );
                    ui.checkbox(&mut enc.accelerate, "accelerate");
                });
            }
        });
        ui.add_space(6.0);

        ui.label("Sent");
        egui::Frame::new()
            .fill(Color32::from_rgb(24, 24, 28))
            .inner_margin(6.0)
            .corner_radius(4.0)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                if out.log.is_empty() {
                    ui.weak("nothing yet");
                }
                for line in &out.log {
                    ui.monospace(line);
                }
            });
    }

    fn piano(&mut self, ui: &mut Ui, out: &mut MidiOut, preview: &AtomicU32) {
        const WHITE: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
        const BLACK: [(u8, usize); 5] = [(1, 0), (3, 1), (6, 3), (8, 4), (10, 5)];
        let octaves = 2;
        let width = ui.available_width().min(560.0);
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(width, 90.0), Sense::click_and_drag());
        let white_w = rect.width() / (7 * octaves) as f32;
        let base = (self.octave as u8 + 1) * 12;

        let mut whites = Vec::new();
        let mut blacks = Vec::new();
        for o in 0..octaves {
            for (i, semi) in WHITE.iter().enumerate() {
                let x = rect.left() + (o * 7 + i) as f32 * white_w;
                let r = Rect::from_min_size(
                    Pos2::new(x, rect.top()),
                    Vec2::new(white_w, rect.height()),
                );
                whites.push((r, base + o as u8 * 12 + semi));
            }
            for (semi, after) in BLACK {
                let x = rect.left() + (o * 7 + after + 1) as f32 * white_w - white_w * 0.3;
                let r = Rect::from_min_size(
                    Pos2::new(x, rect.top()),
                    Vec2::new(white_w * 0.6, rect.height() * 0.6),
                );
                blacks.push((r, base + o as u8 * 12 + semi));
            }
        }

        let pointer = response
            .interact_pointer_pos()
            .filter(|_| response.is_pointer_button_down_on());
        let under = pointer.and_then(|p| {
            blacks
                .iter()
                .chain(whites.iter())
                .find(|(r, _)| r.contains(p))
                .map(|&(_, n)| n)
        });
        if under != self.held {
            if let Some(n) = self.held {
                out.send(&midi_out::note_off(self.channel, n, self.zero_velocity_off));
            }
            if let Some(n) = under {
                out.send(&midi_out::note_on(self.channel, n, self.velocity));
            }
            self.held = under;
            preview.store(
                pack_preview(under.map(|n| (n, self.velocity))),
                Ordering::Relaxed,
            );
        }

        let painter = ui.painter();
        for &(r, n) in &whites {
            let fill = if self.held == Some(n) {
                ACCENT
            } else {
                KEY_WHITE
            };
            painter.rect(
                r.shrink(0.5),
                2.0,
                fill,
                Stroke::new(1.0, KEY_BLACK),
                StrokeKind::Inside,
            );
            if n % 12 == 0 {
                painter.text(
                    Pos2::new(r.center().x, r.bottom() - 8.0),
                    Align2::CENTER_CENTER,
                    format!("C{}", n / 12 - 1),
                    FontId::proportional(9.0),
                    Color32::from_rgb(90, 90, 96),
                );
            }
        }
        for &(r, n) in &blacks {
            let fill = if self.held == Some(n) {
                ACCENT
            } else {
                KEY_BLACK
            };
            painter.rect_filled(r, 2.0, fill);
        }
    }
}

/// A rotary knob over 0..1; drag up to raise. True when the value moved.
fn knob(ui: &mut Ui, value: &mut f32, label: &str) -> bool {
    let size = Vec2::new(64.0, 64.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::drag());
    let before = *value;
    if response.dragged() {
        *value = (*value - response.drag_delta().y / 150.0).clamp(0.0, 1.0);
    }
    if response.double_clicked() {
        *value = 0.0;
    }
    let c = rect.center() - Vec2::new(0.0, 6.0);
    let r = 20.0;
    let start = 0.75 * PI;
    let sweep = 1.5 * PI;
    let painter = ui.painter();
    painter.add(arc(c, r, start, start + sweep, Stroke::new(4.0, KNOB_BG)));
    if *value > 0.0 {
        painter.add(arc(
            c,
            r,
            start,
            start + sweep * *value,
            Stroke::new(4.0, ACCENT),
        ));
    }
    let a = start + sweep * *value;
    painter.line_segment(
        [c, c + Vec2::angled(a) * (r - 6.0)],
        Stroke::new(2.0, Color32::WHITE),
    );
    painter.text(
        Pos2::new(rect.center().x, rect.bottom() - 4.0),
        Align2::CENTER_CENTER,
        format!("{label}  {}", to7(*value)),
        FontId::proportional(10.0),
        ui.visuals().text_color(),
    );
    *value != before
}

/// An endless knob with no stop; returns the signed steps it turned this frame.
fn encoder(ui: &mut Ui, enc: &mut Encoder) -> i32 {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(64.0, 52.0), Sense::drag());
    let mut steps = 0;
    if response.dragged() {
        let dy = -response.drag_delta().y;
        enc.drag += dy;
        let whole = (enc.drag / ENCODER_PX).trunc();
        enc.drag -= whole * ENCODER_PX;
        if whole != 0.0 {
            // Fast turns send bigger steps, the way accelerating encoders do.
            let speed = if enc.accelerate {
                1 + (dy.abs() / 8.0) as i32
            } else {
                1
            };
            steps = whole as i32 * enc.step * speed;
        }
    }
    enc.angle += steps as f32 * 0.15;
    let c = rect.center();
    let r = 20.0;
    let painter = ui.painter();
    painter.circle(c, r, KNOB_BG, Stroke::new(1.0, Color32::from_gray(90)));
    for i in 0..12 {
        let a = i as f32 * PI / 6.0;
        painter.line_segment(
            [
                c + Vec2::angled(a) * (r + 2.0),
                c + Vec2::angled(a) * (r + 5.0),
            ],
            Stroke::new(1.0, Color32::from_gray(90)),
        );
    }
    let color = if response.dragged() {
        ACCENT
    } else {
        Color32::WHITE
    };
    painter.line_segment(
        [c, c + Vec2::angled(enc.angle - PI / 2.0) * (r - 4.0)],
        Stroke::new(2.0, color),
    );
    steps
}

/// A button that sends on press and on release. Some(true) when it went down,
/// Some(false) when it came up.
fn hold_button(ui: &mut Ui, label: &str, down: &mut bool) -> Option<bool> {
    let response = ui.add(
        egui::Button::new(label)
            .selected(*down)
            .sense(Sense::drag()),
    );
    let now = response.is_pointer_button_down_on();
    if now == *down {
        return None;
    }
    *down = now;
    Some(now)
}

fn arc(c: Pos2, r: f32, from: f32, to: f32, stroke: Stroke) -> Shape {
    let n = 32;
    let points = (0..=n)
        .map(|i| {
            let a = from + (to - from) * i as f32 / n as f32;
            c + Vec2::angled(a) * r
        })
        .collect();
    Shape::line(points, stroke)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_round_trips() {
        assert_eq!(unpack_preview(pack_preview(None)), None);
        assert_eq!(unpack_preview(pack_preview(Some((0, 1)))), Some((0, 1)));
        assert_eq!(
            unpack_preview(pack_preview(Some((127, 127)))),
            Some((127, 127))
        );
    }
}
