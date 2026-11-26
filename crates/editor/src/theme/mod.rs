//! Editor palette and dimensions.

use egui::{
    Button, Color32, Context, FontId, Response, Sense, Stroke, TextStyle, Ui, Visuals, WidgetText,
    vec2,
};

pub(super) const WINDOW_SIZE: [f32; 2] = [1440.0, 880.0];
// Neutral graphite palette with a single blue accent, in the spirit of
// current Unity, Godot 4 and Roblox Studio dark themes.
pub(super) const VIEWPORT: Color32 = Color32::from_rgb(30, 31, 35);
pub(super) const SHELL: Color32 = Color32::from_rgb(22, 23, 26);
pub(super) const PANEL: Color32 = Color32::from_rgb(33, 34, 39);
pub(super) const BORDER: Color32 = Color32::from_rgb(14, 15, 17);
pub(super) const TOOLBAR_MARGIN: egui::Margin = egui::Margin::symmetric(10, 5);
const CONTROL: Color32 = Color32::from_rgb(46, 48, 54);
pub(super) const HOVER: Color32 = Color32::from_rgb(58, 61, 69);
const PRESSED: Color32 = Color32::from_rgb(38, 84, 150);
pub(super) const SELECTED: Color32 = Color32::from_rgb(40, 88, 156);
pub(super) const FOREGROUND: Color32 = Color32::from_rgb(228, 231, 237);
const BODY_FONT: f32 = 13.5;
const CONTROL_HEIGHT: f32 = 24.0;
pub(super) const CONTROL_RADIUS: u8 = 5;
pub(super) const LEFT_WIDTH: f32 = 0.21;
pub(super) const LEFT_TOP_HEIGHT: f32 = 0.56;
pub(super) const CENTER_WIDTH: f32 = 0.74;
pub(super) const TOP_HEIGHT: f32 = 0.74;
pub(super) const RIGHT_BOTTOM_HEIGHT: f32 = 0.42;

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
    visuals.menu_corner_radius = CONTROL_RADIUS.into();
    visuals.handle_shape = egui::style::HandleShape::Circle;

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
        style.spacing.item_spacing = vec2(6.0, 5.0);
        style.spacing.button_padding = vec2(9.0, 3.0);
        style.spacing.window_margin = egui::Margin::same(12);
        style.visuals.popup_shadow = egui::Shadow {
            offset: [0, 6],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(110),
        };
        style.visuals.window_shadow = style.visuals.popup_shadow;
        style.visuals.slider_trailing_fill = true;
        style.visuals.striped = true;
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
    dock.dock_area_padding = Some(egui::Margin::same(4));
    dock.main_surface_border_stroke = Stroke::new(LINE, BORDER);
    dock.main_surface_border_rounding = 0.into();
    dock.tab_bar.height = 30.0;
    dock.tab_bar.corner_radius = egui::CornerRadius {
        nw: CONTROL_RADIUS,
        ne: CONTROL_RADIUS,
        sw: 0,
        se: 0,
    };
    dock.tab_bar.bg_fill = SHELL;
    dock.tab_bar.hline_color = BORDER;
    dock.tab_bar.inner_margin = egui::Margin::symmetric(5, 0);
    dock.tab.tab_body.bg_fill = PANEL;
    dock.tab.tab_body.inner_margin = egui::Margin::same(8);
    dock.tab.tab_body.stroke = Stroke::new(LINE, BORDER);
    dock.tab.tab_body.corner_radius = 0.into();
    dock.tab.active.bg_fill = PANEL;
    dock.tab.active.text_color = FOREGROUND;
    dock.tab.focused.bg_fill = PANEL;
    dock.tab.active.outline_color = BORDER;
    dock.tab.focused.outline_color = ACCENT;
    dock.tab.focused.text_color = FOREGROUND;
    dock.tab.inactive.bg_fill = SHELL;
    dock.tab.inactive.text_color = TEXT;
    dock.tab.hovered.bg_fill = HOVER;
    dock.tab.hovered.text_color = FOREGROUND;
    dock.tab.spacing = 2.0;
    dock.tab.hline_below_active_tab_name = true;
    dock.separator.width = 2.0;
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
pub(super) const GRID: Color32 = Color32::from_rgb(64, 67, 76);
pub(super) const GRID_MAJOR: Color32 = Color32::from_rgb(92, 96, 108);
pub(super) const FRAME: Color32 = Color32::from_rgb(104, 110, 124);
pub(super) const ACCENT: Color32 = Color32::from_rgb(76, 149, 255);
pub(super) const TEXT: Color32 = Color32::from_rgb(158, 165, 178);
pub(super) const ACCEPT: Color32 = Color32::from_rgb(98, 200, 120);
pub(super) const REJECT: Color32 = Color32::from_rgb(236, 96, 106);
pub(super) const WARNING: Color32 = Color32::from_rgb(240, 190, 90);
// Conventional X red, Y green, Z blue, as in Unity, Godot and Roblox Studio.
pub(super) const AXIS_X: Color32 = Color32::from_rgb(232, 74, 86);
pub(super) const AXIS_Y: Color32 = Color32::from_rgb(132, 204, 62);
pub(super) const AXIS_Z: Color32 = Color32::from_rgb(56, 130, 245);
pub(super) const ANCHOR: Color32 = Color32::from_rgb(228, 143, 220);
pub(super) const ANIMATION: Color32 = Color32::from_rgb(198, 148, 232);
/// Outline of the selected 3D object.
pub(super) const SELECTION: Color32 = Color32::from_rgb(255, 156, 36);
/// Outline of the 3D object under the cursor.
pub(super) const HOVER_OUTLINE: Color32 = Color32::from_rgb(110, 180, 255);
/// Hovered or dragged gizmo handle.
pub(super) const GIZMO_HOT: Color32 = Color32::from_rgb(255, 222, 66);
/// Scene-view sky gradient.
pub(super) const SKY_TOP: Color32 = Color32::from_rgb(52, 56, 66);
pub(super) const SKY_BOTTOM: Color32 = Color32::from_rgb(28, 29, 33);

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
pub(super) const PROJECT_DIALOG_WIDTH: f32 = 600.0;
pub(super) const PROJECT_ADDRESS_WIDTH: f32 = 470.0;
pub(super) const PROJECT_BROWSER_HEIGHT: f32 = 265.0;
pub(super) const PRIMARY: Color32 = Color32::from_rgb(46, 128, 238);
