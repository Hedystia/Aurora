//! Pan, zoom, sprite previews, picking and Game view presentation.

use std::collections::HashMap;

use egui::{Color32, Mesh, Pos2, Rect, Sense, TextureHandle, TextureOptions, Ui, Vec2, pos2, vec2};
use engine_core::{
    assets::{AssetId, Assets},
    scene::NodeState,
};

use crate::{Editor, node_tree::Drag, strings, theme};

pub(super) struct Viewport {
    assets: Assets,
    textures: HashMap<AssetId, TextureHandle>,
    nodes: Vec<NodeState>,
    revision: u64,
    cursor: Option<(usize, f64)>,
    pan: Vec2,
    zoom: f32,
    error: Option<String>,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            assets: Assets::new(None),
            textures: HashMap::new(),
            nodes: Vec::new(),
            revision: 0,
            cursor: None,
            pan: Vec2::ZERO,
            zoom: 1.0,
            error: None,
        }
    }
}

pub(super) fn show(ui: &mut Ui, editor: &mut Editor) {
    ui.horizontal(|ui| {
        ui.checkbox(&mut editor.preview.game_view, strings::GAME_VIEW);
        if !editor.preview.game_view && ui.small_button(strings::FRAME_VIEW).clicked() {
            editor.viewport.pan = Vec2::ZERO;
            editor.viewport.zoom = 1.0;
        }
    });
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, theme::VIEWPORT);
    if editor.preview.game_view {
        if let Some(texture) = &editor.preview.texture {
            let size = texture.size_vec2();
            let factor = (rect.width() / size.x).min(rect.height() / size.y);
            let target = Rect::from_center_size(rect.center(), size * factor);
            painter.image(texture.id(), target, theme::UV_RECT, Color32::WHITE);
            if response.clicked() {
                response.request_focus();
                editor.preview.input(b't');
            }
            if response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Space)) {
                editor.preview.input(b' ');
            }
        } else {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                strings::GAME_EMPTY,
                theme::font(),
                theme::TEXT,
            );
        }
        return;
    }

    let viewport = &mut editor.viewport;
    let cursor = editor
        .animation_display
        .or_else(|| editor.timeline.cursor_for_preview());
    if viewport.revision != editor.project.revision || viewport.cursor != cursor {
        match editor
            .project
            .document
            .preview(&editor.project.root, &mut viewport.assets, cursor)
        {
            Ok(nodes) => {
                viewport.nodes = nodes;
                viewport.error = None;
            }
            Err(error) => {
                viewport.nodes.clear();
                viewport.error = Some(error.to_string());
            }
        }
        viewport.revision = editor.project.revision;
        viewport.cursor = cursor;
    }
    if response.dragged_by(egui::PointerButton::Secondary) {
        viewport.pan += ui.input(|input| input.pointer.delta());
    }
    if response.hovered() {
        let scroll = ui.input(|input| input.smooth_scroll_delta.y);
        viewport.zoom = (viewport.zoom * (scroll * theme::ZOOM_FACTOR).exp())
            .clamp(theme::MIN_ZOOM, theme::MAX_ZOOM);
    }
    let center = rect.center() + viewport.pan;
    let scale = viewport.zoom
        * (rect.width() / theme::DESIGN.x)
            .min(rect.height() / theme::DESIGN.y)
            .max(theme::MIN_SCALE);
    let design = Rect::from_center_size(center, theme::DESIGN * scale);
    painter.rect_stroke(
        design,
        0.0,
        (theme::OUTLINE, theme::FRAME),
        egui::StrokeKind::Inside,
    );
    for value in (-10..=10).map(|index| index * 100) {
        let x = center.x + value as f32 * scale;
        if rect.x_range().contains(x) {
            painter.line_segment(
                [pos2(x, rect.top()), pos2(x, rect.bottom())],
                (theme::LINE, theme::GRID),
            );
            painter.text(
                pos2(x, rect.top()),
                egui::Align2::LEFT_TOP,
                value,
                theme::font(),
                theme::TEXT,
            );
        }
        let y = center.y - value as f32 * scale;
        if rect.y_range().contains(y) {
            painter.line_segment(
                [pos2(rect.left(), y), pos2(rect.right(), y)],
                (theme::LINE, theme::GRID),
            );
            painter.text(
                pos2(rect.left(), y),
                egui::Align2::LEFT_TOP,
                value,
                theme::font(),
                theme::TEXT,
            );
        }
    }
    painter.arrow(
        center,
        vec2(theme::GIZMO, 0.0),
        (theme::OUTLINE, theme::AXIS_X),
    );
    painter.arrow(
        center,
        vec2(0.0, -theme::GIZMO),
        (theme::OUTLINE, theme::AXIS_Y),
    );
    let project = |p: [f32; 2]| center + vec2(p[0], -p[1]) * scale;
    let mut picked = None;
    for state in &viewport.nodes {
        let index = state.id.value() as usize;
        if !state.visible {
            continue;
        }
        let appearance = state.appearance.unwrap_or_default();
        let world = |point| project(state.world.point(point));
        let origin = world([0.0; 2]);
        if let Some(sprite) = state.sprite {
            let corners = appearance.corners(sprite.size).map(world);
            let bounds = corners
                .iter()
                .copied()
                .fold(Rect::NOTHING, |mut rect, point| {
                    rect.extend_with(point);
                    rect
                });
            if let std::collections::hash_map::Entry::Vacant(entry) =
                viewport.textures.entry(sprite.texture)
                && let Some(image) = viewport.assets.texture(sprite.texture)
            {
                let pixels = egui::ColorImage::from_rgba_unmultiplied(
                    [image.width() as usize, image.height() as usize],
                    image.as_raw(),
                );
                entry.insert(ui.ctx().load_texture(
                    format!("sprite-{}", sprite.texture),
                    pixels,
                    TextureOptions::LINEAR,
                ));
            }
            if let Some(texture) = viewport.textures.get(&sprite.texture) {
                let mut mesh = Mesh::with_texture(texture.id());
                let [r, g, b, a] = appearance.color;
                let alpha = (u16::from(a) * u16::from(appearance.opacity) / 255) as u8;
                let tint = Color32::from_rgba_unmultiplied(r, g, b, alpha);
                for (position, uv) in corners.into_iter().zip([
                    pos2(0.0, 0.0),
                    pos2(0.0, 1.0),
                    pos2(1.0, 0.0),
                    pos2(1.0, 1.0),
                ]) {
                    mesh.vertices.push(egui::epaint::Vertex {
                        pos: position,
                        uv,
                        color: tint,
                    });
                }
                mesh.indices.extend([0, 1, 2, 2, 1, 3]);
                painter.add(egui::Shape::mesh(mesh));
            }
            if index < editor.project.document.nodes.len()
                && response
                    .interact_pointer_pos()
                    .is_some_and(|p| bounds.contains(p))
            {
                picked = Some(index);
            }
            if editor.project.selected == Some(index) {
                for [a, b] in [[0, 1], [1, 3], [3, 2], [2, 0]] {
                    painter.line_segment([corners[a], corners[b]], (theme::OUTLINE, theme::ACCENT));
                }
            }
        }
        if editor.project.selected == Some(index) {
            cross(&painter, origin);
        }
    }
    if let Some(error) = &viewport.error {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            error,
            theme::font(),
            theme::REJECT,
        );
    }
    painter.text(
        rect.left_bottom() + vec2(theme::PAD, -theme::PAD),
        egui::Align2::LEFT_BOTTOM,
        strings::VIEW_HINT,
        theme::font(),
        theme::TEXT,
    );
    if response.clicked()
        && let Some(picked) = picked
    {
        editor.project.selected = Some(picked);
    }
    if let Some(payload) = response.dnd_release_payload::<Drag>() {
        let drop = response.interact_pointer_pos().map(|position| {
            [
                (position.x - center.x) / scale,
                (center.y - position.y) / scale,
            ]
        });
        let previous_nodes = editor.project.document.nodes.len();
        editor.drop_into(payload.as_ref(), None);
        if editor.project.document.nodes.len() > previous_nodes
            && let (Some(index), Some(position)) = (editor.project.selected, drop)
        {
            editor.project.document.nodes[index].transform.position = position;
            editor.project.changed();
        }
    }
}

fn cross(painter: &egui::Painter, center: Pos2) {
    let size = theme::CROSS;
    painter.line_segment(
        [center - vec2(size, 0.0), center + vec2(size, 0.0)],
        (theme::OUTLINE, theme::ANCHOR),
    );
    painter.line_segment(
        [center - vec2(0.0, size), center + vec2(0.0, size)],
        (theme::OUTLINE, theme::ANCHOR),
    );
}
