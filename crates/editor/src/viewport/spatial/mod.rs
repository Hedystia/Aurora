//! Isometric spatial-node editing overlays.

use egui::{Color32, Painter, Rect, Response, Stroke, Vec2, pos2};
use engine_core::scene::{
    NodeState,
    spatial::{Role, matrices},
};
use glam::Vec3;

use crate::{project::Project, theme};

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    #[default]
    Move,
    Rotate,
    Scale,
}

pub(super) struct Interaction<'a> {
    pub dragging: &'a mut Option<usize>,
    pub mode: Mode,
}

pub(super) fn draw(
    painter: &Painter,
    design: Rect,
    response: &Response,
    delta: Vec2,
    nodes: &[NodeState],
    interaction: Interaction<'_>,
    project: &mut Project,
) -> Option<usize> {
    let origin = design.center();
    let scale = (design.width().min(design.height()) / 10.0).clamp(12.0, 64.0);
    let screen_position = |position: Vec3| {
        pos2(
            origin.x + (position.x - position.z) * scale * 0.5,
            origin.y - (position.y + (position.x + position.z) * 0.25) * scale,
        )
    };
    let worlds = matrices(nodes);
    let mut markers = Vec::new();

    for node in nodes.iter().filter(|node| node.visible) {
        let Some(spatial) = &node.spatial else {
            continue;
        };
        let Some(world) = worlds.get(&node.id) else {
            continue;
        };
        let position = world.transform_point3(Vec3::ZERO);
        let center = screen_position(position);
        let color = match spatial.role {
            Role::Pivot => theme::TEXT,
            Role::Cube => theme::ACCENT,
            Role::Mesh => theme::ANIMATION,
            Role::Camera => theme::ANCHOR,
            Role::DirectionalLight => theme::WARNING,
        };
        let index = node.id.value() as usize;
        let selected = project.selected == Some(index);
        if matches!(spatial.role, Role::Cube | Role::Mesh) {
            let bounds = local_bounds(spatial);
            let corners = bounds.map(|point| screen_position(world.transform_point3(point)));
            for [from, to] in [
                [0, 1],
                [1, 3],
                [3, 2],
                [2, 0],
                [4, 5],
                [5, 7],
                [7, 6],
                [6, 4],
                [0, 4],
                [1, 5],
                [2, 6],
                [3, 7],
            ] {
                painter.line_segment(
                    [corners[from], corners[to]],
                    Stroke::new(if selected { 1.8 } else { 1.0 }, color),
                );
            }
            let rect = corners
                .iter()
                .copied()
                .fold(Rect::NOTHING, |bounds, point| {
                    Rect::from_min_max(bounds.min.min(point), bounds.max.max(point))
                });
            if selected {
                painter.rect_stroke(
                    rect.expand(3.0),
                    0.0,
                    Stroke::new(1.0, Color32::WHITE),
                    egui::StrokeKind::Inside,
                );
            }
            markers.push((index, center, rect.expand(4.0), color));
        } else {
            let radius = 5.0;
            painter.circle_filled(center, radius, color);
            if selected {
                painter.circle_stroke(center, radius + 4.0, Stroke::new(1.5, Color32::WHITE));
            }
            markers.push((
                index,
                center,
                Rect::from_center_size(center, Vec2::splat(radius + 5.0)),
                color,
            ));
        }
        if selected {
            match interaction.mode {
                Mode::Move => {
                    for (axis, tint) in gizmo_axes(scale) {
                        painter.arrow(center, axis, Stroke::new(1.8, tint));
                    }
                }
                Mode::Rotate => {
                    painter.circle_stroke(center, scale * 0.6, Stroke::new(1.8, theme::ACCENT));
                }
                Mode::Scale => {
                    for (axis, tint) in gizmo_axes(scale) {
                        let end = center + axis;
                        painter.line_segment([center, end], Stroke::new(1.8, tint));
                        painter.rect_filled(
                            Rect::from_center_size(end, Vec2::splat(7.0)),
                            0.0,
                            tint,
                        );
                    }
                }
            }
        }
    }

    let pointer = response.interact_pointer_pos();
    let primary_drag_started = response.drag_started_by(egui::PointerButton::Primary);
    let picked = response
        .clicked()
        .then_some(pointer)
        .flatten()
        .or_else(|| primary_drag_started.then_some(pointer).flatten())
        .and_then(|pointer| {
            markers
                .iter()
                .filter_map(|(index, center, bounds, _)| {
                    (bounds.contains(pointer)
                        || (project.selected == Some(*index)
                            && gizmo_hit(pointer, *center, scale, interaction.mode)))
                    .then_some((*index, pointer.distance(*center)))
                })
                .min_by(|left, right| left.1.total_cmp(&right.1))
                .map(|(index, _)| index)
        });

    if primary_drag_started {
        *interaction.dragging = picked;
    }
    if response.dragged_by(egui::PointerButton::Primary)
        && let Some(index) = *interaction.dragging
        && let Some(node) = nodes.iter().find(|node| node.id.value() as usize == index)
        && let Some(spatial) = project
            .document
            .nodes
            .get_mut(index)
            .and_then(|node| node.spatial.as_mut())
    {
        match interaction.mode {
            Mode::Move => {
                let parent = node
                    .parent
                    .and_then(|id| worlds.get(&id).copied())
                    .unwrap_or(glam::Mat4::IDENTITY);
                let world_delta = screen_delta_to_world(delta, scale);
                let local_delta = parent.inverse().transform_vector3(world_delta);
                if local_delta.is_finite() && local_delta != Vec3::ZERO {
                    let position = Vec3::from(spatial.position) + local_delta;
                    spatial.position = position.to_array();
                    project.selected = Some(index);
                    project.changed();
                }
            }
            Mode::Rotate => {
                if let Some(pointer) = pointer {
                    let previous = pointer - delta;
                    let position = worlds
                        .get(&node.id)
                        .map(|world| screen_position(world.transform_point3(Vec3::ZERO)));
                    if let Some(position) = position
                        && let Some(rotation) = rotation_delta(position, previous, pointer)
                    {
                        spatial.rotation[2] += rotation.to_degrees();
                        project.selected = Some(index);
                        project.changed();
                    }
                }
            }
            Mode::Scale => {
                let factor = scale_factor(delta.y);
                let mut changed = false;
                for value in &mut spatial.scale {
                    let next = (*value * factor).clamp(0.001, 1e5);
                    changed |= next != *value;
                    *value = next;
                }
                if changed {
                    project.selected = Some(index);
                    project.changed();
                }
            }
        }
    }
    if response.drag_stopped_by(egui::PointerButton::Primary) {
        *interaction.dragging = None;
    }

    for (index, center, bounds, color) in markers {
        if picked == Some(index) || project.selected == Some(index) {
            painter.rect_stroke(
                bounds,
                0.0,
                Stroke::new(1.5, color),
                egui::StrokeKind::Inside,
            );
            painter.circle_filled(center, 2.0, Color32::WHITE);
        }
    }
    picked
}

fn local_bounds(spatial: &engine_core::scene::spatial::SpatialNode) -> [Vec3; 8] {
    let (minimum, maximum) = spatial.geometry.as_ref().map_or_else(
        || (Vec3::splat(-0.5), Vec3::splat(0.5)),
        |geometry| {
            geometry.positions.iter().fold(
                (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
                |(minimum, maximum), position| {
                    let position = Vec3::from(*position);
                    (minimum.min(position), maximum.max(position))
                },
            )
        },
    );
    [
        Vec3::new(minimum.x, minimum.y, minimum.z),
        Vec3::new(maximum.x, minimum.y, minimum.z),
        Vec3::new(minimum.x, maximum.y, minimum.z),
        Vec3::new(maximum.x, maximum.y, minimum.z),
        Vec3::new(minimum.x, minimum.y, maximum.z),
        Vec3::new(maximum.x, minimum.y, maximum.z),
        Vec3::new(minimum.x, maximum.y, maximum.z),
        Vec3::new(maximum.x, maximum.y, maximum.z),
    ]
}

fn screen_delta_to_world(delta: Vec2, scale: f32) -> Vec3 {
    let x = 2.0 * delta.x / scale;
    Vec3::new(x, -delta.y / scale - 0.25 * x, 0.0)
}

fn gizmo_axes(scale: f32) -> [(Vec2, Color32); 3] {
    [
        (Vec2::new(scale * 0.5, -scale * 0.25), theme::AXIS_X),
        (Vec2::new(0.0, -scale), theme::AXIS_Y),
        (Vec2::new(-scale * 0.5, -scale * 0.25), theme::ANCHOR),
    ]
}

fn gizmo_hit(pointer: egui::Pos2, center: egui::Pos2, scale: f32, mode: Mode) -> bool {
    let delta = pointer - center;
    match mode {
        Mode::Move => gizmo_axes(scale)
            .into_iter()
            .any(|(axis, _)| segment_distance(delta, axis) <= 6.0),
        Mode::Rotate => (delta.length() - scale * 0.6).abs() <= 6.0,
        Mode::Scale => gizmo_axes(scale)
            .into_iter()
            .any(|(axis, _)| (delta - axis).length() <= 7.0),
    }
}

fn segment_distance(point: Vec2, end: Vec2) -> f32 {
    let projection = (point.dot(end) / end.length_sq()).clamp(0.0, 1.0);
    (point - end * projection).length()
}

fn rotation_delta(center: egui::Pos2, previous: egui::Pos2, current: egui::Pos2) -> Option<f32> {
    let previous = previous - center;
    let current = current - center;
    (previous.length_sq() > 1.0 && current.length_sq() > 1.0).then(|| {
        let previous = previous.y.atan2(previous.x);
        let current = current.y.atan2(current.x);
        (current - previous + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI
    })
}

fn scale_factor(delta_y: f32) -> f32 {
    (-delta_y * 0.01).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_delta_moves_cubes_in_the_projected_xy_plane() {
        let scale = 40.0;
        let screen = Vec2::new(12.0, -20.0);
        let world = screen_delta_to_world(screen, scale);
        let projected = Vec2::new(
            (world.x - world.z) * scale * 0.5,
            -(world.y + (world.x + world.z) * 0.25) * scale,
        );
        assert!((projected - screen).length() < 0.001);
        assert_eq!(world.z, 0.0);
    }

    #[test]
    fn rotation_delta_wraps_at_the_angle_boundary() -> Result<(), String> {
        let center = pos2(0.0, 0.0);
        let previous = pos2(-1.0, 0.01);
        let current = pos2(-1.0, -0.01);
        let delta = rotation_delta(center, previous, current)
            .ok_or_else(|| "test positions must be outside the pivot".to_owned())?;
        assert!(delta.abs() < 0.03);
        Ok(())
    }

    #[test]
    fn scale_drag_is_positive_and_directional() {
        assert!(scale_factor(-10.0) > 1.0);
        assert!(scale_factor(10.0) < 1.0);
        assert!(scale_factor(10.0) > 0.0);
    }

    #[test]
    fn transform_tools_pick_their_visible_handles() {
        let center = pos2(10.0, 20.0);
        assert!(gizmo_hit(pos2(20.0, 15.0), center, 40.0, Mode::Move));
        assert!(gizmo_hit(pos2(10.0, -4.0), center, 40.0, Mode::Rotate));
        assert!(gizmo_hit(pos2(10.0, -20.0), center, 40.0, Mode::Scale));
    }
}
