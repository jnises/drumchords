use eframe::egui::{self, Align2, Rgba, TextStyle, vec2};

pub fn toggle(ui: &mut egui::Ui, on: &mut bool, text: &str) {
    let y = ui.spacing().interact_size.y * 0.75;
    let (rect, response) = ui.allocate_exact_size(vec2(y, y), egui::Sense::click());
    if response.clicked() {
        *on = !*on;
    }
    let how_on = ui.ctx().animate_bool(response.id, *on);
    let visuals = ui.style().interact_selectable(&response, *on);
    let rect = rect.expand(visuals.expansion);
    let radius = visuals.corner_radius;
    let disabled_color = egui::lerp(Rgba::from(visuals.bg_fill)..=Rgba::from(visuals.fg_stroke.color), 0.5);
    let text_color = egui::lerp(
        disabled_color..=Rgba::from(visuals.fg_stroke.color),
        how_on,
    ).into();
    let painter = ui.painter();
    painter.rect(rect, radius, visuals.bg_fill, visuals.bg_stroke);
    painter.text(rect.center(), Align2::CENTER_CENTER, text, TextStyle::Button, text_color);
}
