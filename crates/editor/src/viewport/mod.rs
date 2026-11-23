//! Pan, zoom, sprite previews, picking and Game view presentation.

mod collider;

mod spatial;

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
    game_focus: bool,
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
            game_focus: true,
        }
    }
}

pub(super) fn show(ui: &mut Ui, editor: &mut Editor) {
    ui.horizontal(|ui| {
        ui.selectable_value(&mut editor.preview.game_view, false, strings::SCENE);
        ui.selectable_value(&mut editor.preview.game_view, true, strings::GAME_VIEW);
        if !editor.preview.game_view {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(strings::FRAME_VIEW).clicked() {
                    editor.viewport.pan = Vec2::ZERO;
                    editor.viewport.zoom = 1.0;
                }
                ui.label(
                    egui::RichText::new(format!("{:.0}%", editor.viewport.zoom * 100.0)).weak(),
                );
            });
        }
    });
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, theme::VIEWPORT);
    let drop_hint = response.dnd_hover_payload::<Drag>().map(|payload| {
        (
            editor.drop_allowed(payload.as_ref(), None),
            crate::node_tree::drop_hint(payload.as_ref()),
        )
    });
    if editor.preview.game_view {
        if let Some(texture) = &editor.preview.texture {
            let size = texture.size_vec2();
            let factor = (rect.width() / size.x).min(rect.height() / size.y);
            let target = Rect::from_center_size(rect.center(), size * factor);
            painter.image(texture.id(), target, theme::UV_RECT, Color32::WHITE);
            // A Game view drag maps to the same press, move and release phases
            // a finger or mouse produces in the standalone player.
            if let Some(position) = response
                .interact_pointer_pos()
                .filter(|position| target.contains(*position))
            {
                let normalized = [
                    (position.x - target.left()) / target.width(),
                    (position.y - target.top()) / target.height(),
                ];
                if response.drag_started() {
                    response.request_focus();
                    editor.preview.touch(normalized);
                } else if response.dragged() {
                    editor.preview.pointer_move(normalized);
                } else if response.drag_stopped() {
                    editor.preview.pointer_release(normalized);
                } else if response.clicked() {
                    response.request_focus();
                    editor.preview.touch(normalized);
                    editor.preview.pointer_release(normalized);
                }
            }
            // Real multi-touch devices give the Game view one contact per finger.
            let touches = ui.input(|input| {
                input
                    .events
                    .iter()
                    .filter_map(|event| match event {
                        egui::Event::Touch { id, phase, pos, .. } => Some((*id, *phase, *pos)),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            });
            for (id, phase, position) in touches {
                if phase == egui::TouchPhase::Start && !target.contains(position) {
                    continue;
                }
                let id = (id.0 % u64::from(u16::MAX)) as u16;
                let normalized = [
                    ((position.x - target.left()) / target.width()).clamp(0.0, 1.0),
                    ((position.y - target.top()) / target.height()).clamp(0.0, 1.0),
                ];
                match phase {
                    egui::TouchPhase::Start => editor.preview.touch_point(id, normalized),
                    egui::TouchPhase::Move => editor.preview.touch_move(id, normalized),
                    egui::TouchPhase::End => editor.preview.touch_end(id, normalized),
                    egui::TouchPhase::Cancel => editor.preview.touch_cancel(id),
                }
            }
            let focused = ui.input(|input| input.focused);
            if editor.viewport.game_focus && !focused {
                // The editor window lost focus mid-drag: release every contact.
                editor.preview.cancel_pointers();
            }
            editor.viewport.game_focus = focused;
            if let Some(position) = response
                .hover_pos()
                .filter(|position| target.contains(*position))
            {
                let delta = ui.input(|input| input.smooth_scroll_delta);
                if delta != egui::Vec2::ZERO {
                    editor.preview.wheel(
                        [
                            (position.x - target.left()) / target.width(),
                            (position.y - target.top()) / target.height(),
                        ],
                        [delta.x, delta.y],
                    );
                }
            }
            if response.lost_focus() {
                editor.preview.cancel_pointers();
            }
            if response.has_focus() {
                let keys = ui.input(|input| {
                    input
                        .events
                        .iter()
                        .filter_map(|event| match event {
                            egui::Event::Key {
                                key,
                                physical_key,
                                pressed,
                                repeat: false,
                                ..
                            } => Some((preview_key(physical_key.unwrap_or(*key))?, *pressed)),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                });
                for (key, pressed) in keys {
                    if pressed {
                        editor.preview.input(key.preview_byte());
                    } else {
                        editor.preview.key_up(key);
                    }
                }
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
        if viewport.revision != editor.project.revision {
            viewport.assets = Assets::new(None);
        }
        match editor
            .project
            .document
            .preview(&editor.project.root, &mut viewport.assets, cursor)
        {
            Ok(nodes) => {
                let live: std::collections::HashSet<_> = nodes
                    .iter()
                    .filter_map(|node| node.sprite.map(|sprite| sprite.texture))
                    .collect();
                viewport.textures.retain(|id, _| live.contains(id));
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
    let [w, h] = editor
        .project
        .document
        .display
        .unwrap_or_default()
        .resolution;
    let design_size = vec2(w as f32, h as f32);
    let scale = viewport.zoom
        * (rect.width() / design_size.x)
            .min(rect.height() / design_size.y)
            .max(theme::MIN_SCALE);
    let design = Rect::from_center_size(center, design_size * scale);
    painter.rect_stroke(
        design,
        0.0,
        (theme::OUTLINE, theme::FRAME),
        egui::StrokeKind::Inside,
    );
    draw_rulers(&painter, rect, center, scale);
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
    if let Some((valid, hint)) = drop_hint {
        crate::icons::paint_drop_frame(&painter, design, valid);
        painter.text(
            pos2(rect.center().x, design.bottom() + 16.0),
            egui::Align2::CENTER_TOP,
            hint,
            theme::font(),
            if valid { theme::ACCEPT } else { theme::REJECT },
        );
    }
    let project = |p: [f32; 2]| center + vec2(p[0], -p[1]) * scale;
    let mut picked = spatial::draw(&painter, design, &response, ui.input(|i| i.pointer.delta()), &viewport.nodes, &mut editor.project);
    let mut ordered: Vec<_> = viewport.nodes.iter().collect();
    ordered.sort_by_key(|node| node.appearance.map_or(0, |appearance| appearance.layer));
    for state in ordered {
        let index = state.id.value() as usize;
        if !state.visible {
            continue;
        }
        let appearance = state.appearance.unwrap_or_default();
        let world = |point| project(state.world.point(point));
        let origin = world([0.0; 2]);
        if let Some(sprite) = state.sprite {
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
            for corners in sprite.quads(appearance, state.draw_tick) {
                let corners = corners.map(world);
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
                        .is_some_and(|point| contains_sprite(corners, point))
                {
                    picked = Some(index);
                }
                if editor.project.selected == Some(index) {
                    for [a, b] in [[0, 1], [1, 3], [3, 2], [2, 0]] {
                        painter.line_segment(
                            [corners[a], corners[b]],
                            (theme::OUTLINE, theme::ACCENT),
                        );
                    }
                }
            }
        }
        if editor.project.selected == Some(index) && state.spatial.is_none() {
            cross(&painter, origin);
        }
    }
    if let Some(index) = editor.project.selected
        && let Some(body) = editor.project.document.nodes.get(index).and_then(|node| node.physics2d.as_ref())
        && let Some(node) = viewport.nodes.iter().find(|node| node.id.value() == index as u64 && node.visible)
    {
        collider::outline(&painter, &viewport.nodes, node, project(node.world.point([0.0;2])), scale, &body.collider);
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
    if response.clicked() {
        editor.project.selected = picked;
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
            if editor.project.document.nodes[index].spatial.is_none() { editor.project.document.nodes[index].transform.position = position; }
            editor.project.changed();
        }
    }
}

fn preview_key(key: egui::Key) -> Option<engine_core::input::Key> {
    use engine_core::input::Key;
    Some(match key {
        egui::Key::Space => Key::Space,
        egui::Key::ArrowUp => Key::ArrowUp,
        egui::Key::Backspace => Key::Backspace,
        egui::Key::Enter => Key::Enter,
        egui::Key::Escape => Key::Escape,
        egui::Key::ArrowLeft => Key::ArrowLeft,
        egui::Key::ArrowRight => Key::ArrowRight,
        egui::Key::ArrowDown => Key::ArrowDown,
        egui::Key::Num0 => Key::Digit0,
        egui::Key::Num1 => Key::Digit1,
        egui::Key::Num2 => Key::Digit2,
        egui::Key::Num3 => Key::Digit3,
        egui::Key::Num4 => Key::Digit4,
        egui::Key::Num5 => Key::Digit5,
        egui::Key::Num6 => Key::Digit6,
        egui::Key::Num7 => Key::Digit7,
        egui::Key::Num8 => Key::Digit8,
        egui::Key::Num9 => Key::Digit9,
        egui::Key::A => Key::A,
        egui::Key::B => Key::B,
        egui::Key::C => Key::C,
        egui::Key::D => Key::D,
        egui::Key::E => Key::E,
        egui::Key::F => Key::F,
        egui::Key::G => Key::G,
        egui::Key::H => Key::H,
        egui::Key::I => Key::I,
        egui::Key::J => Key::J,
        egui::Key::K => Key::K,
        egui::Key::L => Key::L,
        egui::Key::M => Key::M,
        egui::Key::N => Key::N,
        egui::Key::O => Key::O,
        egui::Key::P => Key::P,
        egui::Key::Q => Key::Q,
        egui::Key::R => Key::R,
        egui::Key::S => Key::S,
        egui::Key::T => Key::T,
        egui::Key::U => Key::U,
        egui::Key::V => Key::V,
        egui::Key::W => Key::W,
        egui::Key::X => Key::X,
        egui::Key::Y => Key::Y,
        egui::Key::Z => Key::Z,
        _ => return None,
    })
}

fn ruler_step(scale: f32) -> f32 {
    let units = (theme::RULER_SPACING / scale).max(1.0);
    let magnitude = 10.0_f32.powf(units.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|factor| factor * magnitude)
        .find(|&step| step >= units)
        .unwrap_or(10.0 * magnitude)
}

fn draw_rulers(painter: &egui::Painter, rect: Rect, center: Pos2, scale: f32) {
    let step = ruler_step(scale);
    let spacing = step * scale;
    let ticks = |min: f32, max: f32, origin: f32| {
        ((min - origin) / spacing).ceil() as i32..=((max - origin) / spacing).floor() as i32
    };
    for tick in ticks(rect.left(), rect.right(), center.x) {
        let x = center.x + tick as f32 * spacing;
        painter.line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            (theme::LINE, theme::GRID),
        );
        if x - rect.left() >= theme::RULER_CORNER {
            painter.text(
                pos2(x, rect.top()) + vec2(theme::PAD, 0.0),
                egui::Align2::LEFT_TOP,
                (tick as f32 * step) as i64,
                theme::font(),
                theme::TEXT,
            );
        }
    }
    for tick in ticks(rect.top(), rect.bottom(), center.y) {
        let y = center.y + tick as f32 * spacing;
        painter.line_segment(
            [pos2(rect.left(), y), pos2(rect.right(), y)],
            (theme::LINE, theme::GRID),
        );
        if y - rect.top() >= theme::PAD * 2.0 {
            painter.text(
                pos2(rect.left() + theme::PAD, y),
                egui::Align2::LEFT_CENTER,
                (-(tick as f32) * step) as i64,
                theme::font(),
                theme::TEXT,
            );
        }
    }
}

fn contains_sprite(corners: [Pos2; 4], point: Pos2) -> bool {
    let [a, b, c, d] = corners;
    let cross = |u: Vec2, v: Vec2| u.x * v.y - u.y * v.x;
    if cross(b - a, c - a) == 0.0 {
        return false;
    }
    let sides = [(a, b), (b, d), (d, c), (c, a)].map(|(from, to)| cross(to - from, point - from));
    sides.iter().all(|&side| side >= 0.0) || sides.iter().all(|&side| side <= 0.0)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ruler_keeps_labels_apart_at_every_zoom() {
        for scale in [0.0003, 0.01, 0.06, 0.1, 1.0, 4.0, 32.0] {
            assert!(ruler_step(scale) * scale >= theme::RULER_SPACING - f32::EPSILON);
        }
    }

    #[test]
    fn picking_follows_the_drawn_sprite_corners() {
        let diamond = [
            pos2(0.0, -1.0),
            pos2(-1.0, 0.0),
            pos2(1.0, 0.0),
            pos2(0.0, 1.0),
        ];
        assert!(contains_sprite(diamond, Pos2::ZERO));
        assert!(!contains_sprite(diamond, pos2(0.9, 0.9)));
        assert!(!contains_sprite([Pos2::ZERO; 4], Pos2::ZERO));
    }
}
