use eframe::egui::{self, vec2, Rgba};

fn selector(ui: &mut egui::Ui, on: &mut bool, bg: bool) {
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
    // TODO draw divisor
}

pub fn pattern_designer(
    ui: &mut egui::Ui,
    fg_pattern: &mut u16,
    bg_pattern: u16,
    pattern_length: u64,
) {
    debug_assert!(pattern_length <= 16);
    debug_assert!(*fg_pattern & (!((1 << 1 + pattern_length) - 1)) == 0);
    debug_assert!(bg_pattern & (!((1 << 1 + pattern_length) - 1)) == 0);
    ui.horizontal(|ui| {
        for i in 0..pattern_length {
            let mut b = *fg_pattern >> i & 1 != 0;
            let bg = bg_pattern >> i & 1 != 0;
            selector(ui, &mut b, bg);
            *fg_pattern = *fg_pattern & !(1 << i) | (b as u16) << i;
        }
        if ui.small_button("🔏").clicked() {
            *fg_pattern |= bg_pattern;
        }
    });
}
