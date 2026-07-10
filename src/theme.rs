//! Titan-Design: schwarzer Hintergrund, glänzende Titan-Elemente.

use egui::{Color32, Rounding, Stroke, Visuals};

pub const BG: Color32 = Color32::from_rgb(7, 8, 10);
pub const PANEL: Color32 = Color32::from_rgb(14, 16, 20);
pub const CARD: Color32 = Color32::from_rgb(22, 25, 31);
pub const CARD_HOVER: Color32 = Color32::from_rgb(32, 36, 44);
pub const TITAN_LIGHT: Color32 = Color32::from_rgb(226, 232, 240);
pub const TITAN: Color32 = Color32::from_rgb(168, 178, 192);
pub const TITAN_DARK: Color32 = Color32::from_rgb(94, 104, 118);
pub const ACCENT: Color32 = Color32::from_rgb(140, 190, 255);
pub const GREEN: Color32 = Color32::from_rgb(96, 220, 140);
pub const RED: Color32 = Color32::from_rgb(240, 100, 100);

pub fn apply(ctx: &egui::Context) {
    let mut v = Visuals::dark();
    v.override_text_color = Some(TITAN_LIGHT);
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.extreme_bg_color = BG;
    v.faint_bg_color = CARD;
    v.code_bg_color = BG;
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = Color32::from_rgb(60, 80, 110);
    v.selection.stroke = Stroke::new(1.0, TITAN_LIGHT);

    let r = Rounding::same(8.0);
    v.widgets.noninteractive.bg_fill = CARD;
    v.widgets.noninteractive.weak_bg_fill = CARD;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(40, 44, 52));
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TITAN);
    v.widgets.noninteractive.rounding = r;

    v.widgets.inactive.bg_fill = CARD;
    v.widgets.inactive.weak_bg_fill = CARD;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(52, 58, 68));
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TITAN_LIGHT);
    v.widgets.inactive.rounding = r;

    v.widgets.hovered.bg_fill = CARD_HOVER;
    v.widgets.hovered.weak_bg_fill = CARD_HOVER;
    v.widgets.hovered.bg_stroke = Stroke::new(1.2, TITAN);
    v.widgets.hovered.fg_stroke = Stroke::new(1.2, Color32::WHITE);
    v.widgets.hovered.rounding = r;

    v.widgets.active.bg_fill = Color32::from_rgb(45, 52, 62);
    v.widgets.active.weak_bg_fill = Color32::from_rgb(45, 52, 62);
    v.widgets.active.bg_stroke = Stroke::new(1.4, TITAN_LIGHT);
    v.widgets.active.fg_stroke = Stroke::new(1.4, Color32::WHITE);
    v.widgets.active.rounding = r;

    v.widgets.open = v.widgets.active;

    let mut style = (*ctx.style()).clone();
    style.visuals = v;
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(14.0, 8.0);
    ctx.set_style(style);
}

/// Zeichnet einen glänzenden Titan-Verlauf (Metallic-Schimmer) in ein Rechteck.
pub fn titanium_gradient(painter: &egui::Painter, rect: egui::Rect, rounding: f32) {
    use egui::epaint::{Mesh, Shape, Vertex, WHITE_UV};
    let top = Color32::from_rgb(58, 64, 74);
    let mid = Color32::from_rgb(120, 130, 144);
    let bottom = Color32::from_rgb(28, 31, 37);

    let mid_y = rect.top() + rect.height() * 0.42;
    let mut mesh = Mesh::default();
    let mut add_quad = |y0: f32, y1: f32, c0: Color32, c1: Color32| {
        let base = mesh.vertices.len() as u32;
        for (p, c) in [
            (egui::pos2(rect.left(), y0), c0),
            (egui::pos2(rect.right(), y0), c0),
            (egui::pos2(rect.right(), y1), c1),
            (egui::pos2(rect.left(), y1), c1),
        ] {
            mesh.vertices.push(Vertex {
                pos: p,
                uv: WHITE_UV,
                color: c,
            });
        }
        mesh.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    };
    add_quad(rect.top(), mid_y, top, mid);
    add_quad(mid_y, rect.bottom(), mid, bottom);
    painter.add(Shape::mesh(mesh));
    // Glanzlinie oben
    painter.line_segment(
        [
            egui::pos2(rect.left() + rounding, rect.top() + 1.0),
            egui::pos2(rect.right() - rounding, rect.top() + 1.0),
        ],
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 70)),
    );
}

/// Karte mit Titan-Rahmen
pub fn card_frame() -> egui::Frame {
    egui::Frame::none()
        .fill(CARD)
        .rounding(Rounding::same(12.0))
        .stroke(Stroke::new(1.0, Color32::from_rgb(48, 54, 64)))
        .inner_margin(egui::Margin::same(16.0))
}
