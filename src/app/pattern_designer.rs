use eframe::egui::{self, vec2, Align2, Rgba, TextStyle};

fn selector(ui: &mut egui::Ui, on: &mut bool, bg: bool, label: &str) {
    let y = ui.spacing().interact_size.y * 0.5;
    let (rect, response) = ui.allocate_exact_size(vec2(y, y), egui::Sense::click());
    if response.clicked() {
        *on = !*on;
    }
    let how_on = ui.ctx().animate_bool(response.id, *on);
    let visuals = ui.style().interact_selectable(&response, *on);
    let rect = rect.expand(visuals.expansion);
    let radius = visuals.corner_radius;
    let color = egui::lerp(
        if bg {
            Rgba::from_gray(0.5)
        } else {
            Rgba::from(visuals.bg_fill)
        }..=Rgba::from(visuals.fg_stroke.color),
        how_on,
    );
    ui.painter().rect(rect, radius, color, visuals.bg_stroke);
    let text_color = color
        + if color.intensity() > 0.5 {
            Rgba::from_gray(-0.7)
        } else {
            Rgba::from_gray(0.3)
        };
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        TextStyle::Small,
        text_color.into(),
    );
}

pub fn pattern_designer(
    ui: &mut egui::Ui,
    fg_pattern: &mut u16,
    bg_pattern: u16,
    pattern_length: u64,
) {
    debug_assert!(pattern_length <= 16);
    debug_assert!(*fg_pattern & (!((1 << (1 + pattern_length)) - 1)) == 0);
    debug_assert!(bg_pattern & (!((1 << (1 + pattern_length)) - 1)) == 0);
    ui.horizontal(|ui| {
        for i in 0..pattern_length {
            let mut b = (*fg_pattern >> i) & 1 != 0;
            let bg = (bg_pattern >> i) & 1 != 0;
            // TODO do the label further out to support different schemes
            selector(ui, &mut b, bg, &(i + 1).to_string());
            *fg_pattern = *fg_pattern & !(1 << i) | ((b as u16) << i);
        }
        if ui.small_button("🔏").clicked() {
            *fg_pattern |= bg_pattern;
        }
    });
}
