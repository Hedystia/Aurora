//! Editor palette and dimensions.

use egui::{Color32, Context, FontId, TextStyle, Visuals, vec2};

pub(super) const WINDOW_SIZE: [f32; 2] = [1180.0, 760.0];
pub(super) const VIEWPORT: Color32 = Color32::from_rgb(19, 26, 39);
pub(super) const LEFT_WIDTH: f32 = 0.21;
pub(super) const CENTER_WIDTH: f32 = 0.67;
pub(super) const TOP_HEIGHT: f32 = 0.72;
pub(super) const LIBRARY_HEIGHT: f32 = 0.32;

pub(super) fn apply(ctx: &Context) {
    let mut visuals = Visuals::dark();
    visuals.panel_fill = Color32::from_rgb(34, 43, 58);
    visuals.window_fill = visuals.panel_fill;
    visuals.extreme_bg_color = VIEWPORT;
    visuals.selection.bg_fill = Color32::from_rgb(50, 107, 170);
    ctx.set_visuals_of(egui::Theme::Dark, visuals);
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.spacing.item_spacing = vec2(7.0, 5.0);
        for (name, font) in &mut style.text_styles {
            if *name != TextStyle::Heading {
                *font = FontId::new(13.0, font.family.clone());
            }
        }
    });
}

pub(super) const DESIGN: egui::Vec2 = vec2(640.0, 384.0);
pub(super) const UV_RECT: egui::Rect =
    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
pub(super) const INDENT: f32 = 13.0;
pub(super) const PAD: f32 = 6.0;
pub(super) const LINE: f32 = 1.0;
pub(super) const ROUND: f32 = 3.0;
pub(super) const OUTLINE: f32 = 1.5;
pub(super) const GIZMO: f32 = 22.0;
pub(super) const CROSS: f32 = 5.0;
pub(super) const MIN_SCALE: f32 = 0.01;
pub(super) const ZOOM_FACTOR: f32 = 0.003;
pub(super) const MIN_ZOOM: f32 = 0.25;
pub(super) const MAX_ZOOM: f32 = 8.0;
pub(super) const VALUE_SPEED: f64 = 0.1;
pub(super) const ANGLE_SPEED: f64 = 0.02;
pub(super) const GRID: Color32 = Color32::from_rgb(39, 51, 70);
pub(super) const FRAME: Color32 = Color32::from_rgb(104, 119, 149);
pub(super) const ACCENT: Color32 = Color32::from_rgb(104, 181, 243);
pub(super) const TEXT: Color32 = Color32::from_rgb(172, 187, 207);
pub(super) const ACCEPT: Color32 = Color32::from_rgb(119, 219, 167);
pub(super) const REJECT: Color32 = Color32::from_rgb(249, 127, 134);
pub(super) const WARNING: Color32 = Color32::from_rgb(230, 184, 102);
pub(super) const AXIS_X: Color32 = Color32::from_rgb(232, 125, 134);
pub(super) const AXIS_Y: Color32 = Color32::from_rgb(128, 209, 141);
pub(super) const ANCHOR: Color32 = Color32::from_rgb(228, 143, 220);

pub(super) fn font() -> FontId {
    FontId::monospace(11.0)
}

pub(super) const TIMELINE_RULER: f32 = 30.0;
pub(super) const KEY_RADIUS: f32 = 3.0;
