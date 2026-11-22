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
