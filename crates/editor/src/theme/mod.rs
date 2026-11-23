//! Editor palette and dimensions.

use egui::{
    Button, Color32, Context, FontId, Response, Sense, Stroke, TextStyle, Ui, Visuals, WidgetText,
    vec2,
};

pub(super) const WINDOW_SIZE: [f32; 2] = [1180.0, 760.0];
pub(super) const VIEWPORT: Color32 = Color32::from_rgb(30, 32, 36);
pub(super) const SHELL: Color32 = Color32::from_rgb(26, 28, 32);
pub(super) const PANEL: Color32 = Color32::from_rgb(36, 38, 43);
pub(super) const BORDER: Color32 = Color32::from_rgb(19, 20, 23);
pub(super) const TOOLBAR_MARGIN: egui::Margin = egui::Margin::symmetric(10, 4);
const CONTROL: Color32 = Color32::from_rgb(45, 48, 54);
pub(super) const HOVER: Color32 = Color32::from_rgb(56, 60, 68);
const PRESSED: Color32 = Color32::from_rgb(43, 66, 96);
pub(super) const SELECTED: Color32 = Color32::from_rgb(43, 81, 120);
pub(super) const FOREGROUND: Color32 = Color32::from_rgb(228, 230, 234);
const BODY_FONT: f32 = 13.0;
const CONTROL_HEIGHT: f32 = 22.0;
const CONTROL_RADIUS: u8 = 4;
pub(super) const LEFT_WIDTH: f32 = 0.22;
pub(super) const LEFT_TOP_HEIGHT: f32 = 0.56;
pub(super) const CENTER_WIDTH: f32 = 0.74;
pub(super) const TOP_HEIGHT: f32 = 0.74;

pub(super) fn apply(ctx: &Context) {
    let mut visuals = Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.window_stroke = Stroke::new(LINE, BORDER);
    visuals.window_corner_radius = CONTROL_RADIUS.into();
    visuals.extreme_bg_color = VIEWPORT;
    visuals.faint_bg_color = SHELL;
    visuals.text_edit_bg_color = Some(VIEWPORT);
    visuals.hyperlink_color = ACCENT;
    visuals.weak_text_color = Some(TEXT);
    visuals.selection.bg_fill = SELECTED;
    visuals.selection.stroke = Stroke::new(LINE, FOREGROUND);

    let widgets = &mut visuals.widgets;
    for (state, background, border, text) in [
        (&mut widgets.noninteractive, PANEL, BORDER, FOREGROUND),
        (&mut widgets.inactive, CONTROL, BORDER, FOREGROUND),
        (&mut widgets.hovered, HOVER, FRAME, FOREGROUND),
        (&mut widgets.active, PRESSED, ACCENT, FOREGROUND),
        (&mut widgets.open, PRESSED, BORDER, FOREGROUND),
    ] {
        state.bg_fill = background;
        state.weak_bg_fill = background;
        state.bg_stroke = Stroke::new(LINE, border);
        state.fg_stroke.color = text;
        state.corner_radius = CONTROL_RADIUS.into();
        state.expansion = 0.0;
    }
    ctx.set_visuals_of(egui::Theme::Dark, visuals);
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.spacing.item_spacing = vec2(6.0, 4.0);
        style.spacing.button_padding = vec2(7.0, 2.0);
        style.spacing.interact_size.y = CONTROL_HEIGHT;
        style.spacing.indent = INDENT;
        style.spacing.menu_margin = egui::Margin::same(5);
        style.spacing.combo_width = 120.0;
        for (name, font) in &mut style.text_styles {
            if *name != TextStyle::Heading {
                *font = FontId::new(BODY_FONT, font.family.clone());
            }
        }
    });
}

pub(super) fn row(
    ui: &mut Ui,
    text: impl Into<WidgetText>,
    selected: bool,
    sense: Sense,
) -> Response {
    ui.add(
        Button::selectable(selected, ())
            .left_text(text.into())
            .sense(sense)
            .truncate()
            .min_size(vec2(ui.available_width(), PROJECT_ROW_HEIGHT)),
    )
}

pub(super) fn dock_style(style: &egui::Style) -> egui_dock::Style {
    let mut dock = egui_dock::Style::from_egui(style);
    dock.dock_area_padding = Some(egui::Margin::same(3));
    dock.main_surface_border_stroke = Stroke::new(LINE, BORDER);
    dock.main_surface_border_rounding = 0.into();
    dock.tab_bar.height = 27.0;
    dock.tab_bar.bg_fill = SHELL;
    dock.tab_bar.hline_color = BORDER;
    dock.tab_bar.inner_margin = egui::Margin::symmetric(5, 0);
    dock.tab.tab_body.bg_fill = PANEL;
    dock.tab.tab_body.inner_margin = egui::Margin::same(7);
    dock.tab.tab_body.stroke = Stroke::new(LINE, BORDER);
    dock.tab.tab_body.corner_radius = 0.into();
    dock.tab.active.bg_fill = PANEL;
    dock.tab.active.text_color = FOREGROUND;
    dock.tab.focused.bg_fill = SELECTED;
    dock.tab.focused.text_color = FOREGROUND;
    dock.tab.inactive.bg_fill = SHELL;
    dock.tab.inactive.text_color = TEXT;
    dock.tab.hovered.bg_fill = HOVER;
    dock.tab.hovered.text_color = FOREGROUND;
    dock.tab.spacing = 2.0;
    dock.tab.hline_below_active_tab_name = true;
    dock.separator.width = 1.0;
    dock.separator.extra_interact_width = 5.0;
    dock.separator.color_idle = BORDER;
    dock.separator.color_hovered = ACCENT;
    dock.separator.color_dragged = ACCENT;
    dock.overlay.selection_color = ACCENT.gamma_multiply(0.45);
    dock
}

pub(super) const UV_RECT: egui::Rect =
    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
pub(super) const INDENT: f32 = 14.0;
pub(super) const ROW_HEIGHT: f32 = 24.0;
pub(super) const ICON: f32 = 14.0;
pub(super) const ICON_BUTTON: f32 = 22.0;
pub(super) const TILE: f32 = 76.0;
pub(super) const PAD: f32 = 6.0;
pub(super) const LINE: f32 = 1.0;
pub(super) const ROUND: f32 = 3.0;
pub(super) const OUTLINE: f32 = 1.5;
pub(super) const GIZMO: f32 = 22.0;
pub(super) const CROSS: f32 = 5.0;
pub(super) const MIN_SCALE: f32 = 0.01;
pub(super) const RULER_SPACING: f32 = 66.0;
pub(super) const RULER_CORNER: f32 = 40.0;
pub(super) const ZOOM_FACTOR: f32 = 0.003;
pub(super) const MIN_ZOOM: f32 = 0.25;
pub(super) const MAX_ZOOM: f32 = 8.0;
pub(super) const VALUE_SPEED: f64 = 0.1;
pub(super) const ANGLE_SPEED: f64 = 0.02;
pub(super) const GRID: Color32 = Color32::from_rgb(37, 40, 45);
pub(super) const FRAME: Color32 = Color32::from_rgb(88, 94, 104);
pub(super) const ACCENT: Color32 = Color32::from_rgb(79, 158, 227);
pub(super) const TEXT: Color32 = Color32::from_rgb(164, 170, 180);
pub(super) const ACCEPT: Color32 = Color32::from_rgb(111, 191, 115);
pub(super) const REJECT: Color32 = Color32::from_rgb(224, 108, 117);
pub(super) const WARNING: Color32 = Color32::from_rgb(229, 192, 123);
pub(super) const AXIS_X: Color32 = Color32::from_rgb(232, 125, 134);
pub(super) const AXIS_Y: Color32 = Color32::from_rgb(128, 209, 141);
pub(super) const ANCHOR: Color32 = Color32::from_rgb(228, 143, 220);
pub(super) const ANIMATION: Color32 = Color32::from_rgb(198, 148, 232);

pub(super) fn font() -> FontId {
    FontId::monospace(11.0)
}

pub(super) const TIMELINE_RULER: f32 = 30.0;
pub(super) const KEY_RADIUS: f32 = 3.0;

pub(super) const PROJECT_MARGIN: i8 = 16;
pub(super) const PROJECT_TITLE: f32 = 28.0;
pub(super) const PROJECT_ACTION_WIDTH: f32 = 144.0;
pub(super) const PROJECT_BUTTON_HEIGHT: f32 = 30.0;
pub(super) const PROJECT_ROW_HEIGHT: f32 = 26.0;
pub(super) const PROJECT_DIALOG_WIDTH: f32 = 440.0;
pub(super) const PROJECT_ADDRESS_WIDTH: f32 = 350.0;
pub(super) const PROJECT_BROWSER_HEIGHT: f32 = 265.0;
pub(super) const PRIMARY: Color32 = Color32::from_rgb(36, 89, 143);
