//! Small vector icons used by Aurora's native editor.

use std::path::Path;

use egui::{
    Button, Color32, Painter, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2,
};

#[derive(Clone, Copy)]
pub(super) enum Icon {
    Eye,
    EyeOff,
    Canvas,
    Folder,
    Grid,
    List,
    Node,
    Parent,
    Pause,
    Play,
    Plus,
    Prefab,
    Refresh,
    Script,
    Sprite,
    Stop,
    Text,
    Trash,
}

pub(super) fn node(sprite: bool) -> Icon {
    if sprite { Icon::Sprite } else { Icon::Node }
}

pub(super) fn file(path: &Path) -> Icon {
    if path.is_dir() {
        Icon::Folder
    } else if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
    {
        Icon::Sprite
    } else {
        Icon::Script
    }
}

pub(super) fn button(ui: &mut Ui, icon: Icon, label: &str) -> Response {
    action(ui, icon, label, None, true)
}

pub(super) fn icon_button(ui: &mut Ui, icon: Icon, label: &str) -> Response {
    icon_button_enabled(ui, icon, label, true)
}

pub(super) fn icon_button_enabled(ui: &mut Ui, icon: Icon, label: &str, enabled: bool) -> Response {
    let response = ui
        .add_enabled(
            enabled,
            Button::new("").min_size(vec2(crate::theme::ICON_BUTTON, crate::theme::ROW_HEIGHT)),
        )
        .on_hover_text(label);
    paint(
        ui.painter(),
        Rect::from_center_size(response.rect.center(), vec2(14.0, 14.0)),
        icon,
        if enabled {
            crate::theme::FOREGROUND
        } else {
            crate::theme::TEXT
        },
    );
    response
}

pub(super) fn action(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    fill: Option<Color32>,
    enabled: bool,
) -> Response {
    let response = ui.add_enabled(
        enabled,
        Button::new(label)
            .fill(fill.unwrap_or(Color32::TRANSPARENT))
            .min_size(vec2(ui.available_width().min(130.0), 22.0)),
    );
    let rect = Rect::from_center_size(
        pos2(response.rect.left() + 11.0, response.rect.center().y),
        vec2(14.0, 14.0),
    );
    paint(ui.painter(), rect, icon, crate::theme::FOREGROUND);
    response
}

pub(super) fn inline(ui: &mut Ui, icon: Icon, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(16.0, 18.0), Sense::hover());
    paint(
        ui.painter(),
        Rect::from_center_size(rect.center(), vec2(14.0, 14.0)),
        icon,
        color,
    );
}

pub(super) fn row_background(ui: &mut Ui, rect: Rect, selected: bool, hovered: bool) {
    let fill = if selected {
        crate::theme::SELECTED
    } else if hovered {
        crate::theme::HOVER
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, crate::theme::ROUND, fill);
    }
}

pub(super) fn paint_drop_frame(painter: &Painter, rect: Rect, valid: bool) {
    let color = if valid {
        crate::theme::ACCEPT
    } else {
        crate::theme::REJECT
    };
    painter.rect_stroke(
        rect,
        crate::theme::ROUND,
        Stroke::new(crate::theme::OUTLINE, color),
        StrokeKind::Inside,
    );
}

pub(super) fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let stroke = Stroke::new(1.6, color);
    let center = rect.center();
    let left = rect.left();
    let right = rect.right();
    let top = rect.top();
    let bottom = rect.bottom();
    let line = |a, b| painter.line_segment([a, b], stroke);
    let outline = |r| painter.rect_stroke(r, 1.0, stroke, StrokeKind::Inside);

    match icon {
        Icon::Canvas => {
            outline(rect.shrink(1.5));
            line(pos2(left + 2.0, top + 5.0), pos2(right - 2.0, top + 5.0));
            painter.circle_filled(pos2(left + 4.0, top + 3.0), 0.8, color);
            painter.circle_filled(pos2(left + 7.0, top + 3.0), 0.8, color);
        }
        Icon::Node | Icon::Sprite => {
            outline(rect.shrink(1.5));
            if matches!(icon, Icon::Sprite) {
                line(pos2(left + 3.0, bottom - 3.0), pos2(center.x, center.y));
                line(pos2(center.x, center.y), pos2(right - 3.0, bottom - 3.0));
                painter.circle_filled(pos2(right - 4.0, top + 4.0), 1.5, color);
            }
        }
        Icon::Prefab => {
            outline(rect.shrink(2.0));
            outline(rect.shrink(5.0));
        }
        Icon::Script => {
            outline(rect.shrink(2.0));
            line(pos2(left + 4.0, top + 5.0), pos2(right - 4.0, top + 5.0));
            line(pos2(left + 4.0, center.y), pos2(right - 4.0, center.y));
            line(
                pos2(left + 4.0, bottom - 5.0),
                pos2(right - 4.0, bottom - 5.0),
            );
        }
        Icon::Folder => {
            let body =
                Rect::from_min_max(pos2(left + 1.0, top + 4.0), pos2(right - 1.0, bottom - 1.0));
            outline(body);
            line(pos2(left + 2.0, top + 4.0), pos2(left + 2.0, top + 1.0));
            line(pos2(left + 2.0, top + 1.0), pos2(center.x, top + 1.0));
            line(pos2(center.x, top + 1.0), pos2(center.x + 2.0, top + 4.0));
        }
        Icon::Parent => {
            line(pos2(center.x, bottom - 1.0), pos2(center.x, top + 2.0));
            line(pos2(center.x, top + 2.0), pos2(left + 2.0, center.y + 1.0));
            line(pos2(center.x, top + 2.0), pos2(right - 2.0, center.y + 1.0));
            line(
                pos2(left + 2.0, center.y + 1.0),
                pos2(left + 2.0, bottom - 2.0),
            );
            line(
                pos2(left + 2.0, bottom - 2.0),
                pos2(right - 2.0, bottom - 2.0),
            );
            line(
                pos2(right - 2.0, bottom - 2.0),
                pos2(right - 2.0, center.y + 1.0),
            );
        }
        Icon::Eye | Icon::EyeOff => {
            painter.circle_stroke(center, rect.width() * 0.36, stroke);
            painter.circle_filled(center, 2.0, color);
            if matches!(icon, Icon::EyeOff) {
                line(pos2(left + 1.0, top + 1.0), pos2(right - 1.0, bottom - 1.0));
            }
        }
        Icon::Play => {
            painter.add(Shape::convex_polygon(
                vec![
                    pos2(left + 3.0, top + 1.0),
                    pos2(right - 1.0, center.y),
                    pos2(left + 3.0, bottom - 1.0),
                ],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Pause => {
            painter.rect_filled(
                Rect::from_min_max(
                    pos2(left + 2.0, top + 1.0),
                    pos2(center.x - 1.0, bottom - 1.0),
                ),
                1.0,
                color,
            );
            painter.rect_filled(
                Rect::from_min_max(
                    pos2(center.x + 1.0, top + 1.0),
                    pos2(right - 2.0, bottom - 1.0),
                ),
                1.0,
                color,
            );
        }
        Icon::Stop => {
            painter.rect_filled(rect.shrink(2.0), 1.0, color);
        }
        Icon::Text => {
            for y in [top + 3.0, center.y, bottom - 3.0] {
                line(pos2(left + 2.0, y), pos2(right - 2.0, y));
            }
        }
        Icon::Plus => {
            line(pos2(center.x, top + 1.0), pos2(center.x, bottom - 1.0));
            line(pos2(left + 1.0, center.y), pos2(right - 1.0, center.y));
        }
        Icon::Trash => {
            outline(Rect::from_min_max(
                pos2(left + 3.0, top + 4.0),
                pos2(right - 3.0, bottom - 1.0),
            ));
            line(pos2(left + 2.0, top + 2.0), pos2(right - 2.0, top + 2.0));
            line(
                pos2(center.x - 2.0, top + 1.0),
                pos2(center.x + 2.0, top + 1.0),
            );
        }
        Icon::Refresh => {
            painter.circle_stroke(center, rect.width() * 0.34, stroke);
            line(pos2(right - 2.0, top + 3.0), pos2(right - 2.0, top + 7.0));
            line(pos2(right - 2.0, top + 3.0), pos2(right - 6.0, top + 3.0));
        }
        Icon::List => {
            for y in [top + 3.0, center.y, bottom - 3.0] {
                line(pos2(left + 2.0, y), pos2(right - 2.0, y));
            }
        }
        Icon::Grid => {
            for (x, y) in [
                (left + 1.0, top + 1.0),
                (center.x + 1.0, top + 1.0),
                (left + 1.0, center.y + 1.0),
                (center.x + 1.0, center.y + 1.0),
            ] {
                outline(Rect::from_min_size(pos2(x, y), vec2(5.0, 5.0)));
            }
        }
    }
}
