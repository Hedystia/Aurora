//! Orbitable perspective spatial-node editing overlays.

use egui::{Color32, Painter, Pos2, Rect, Response, Stroke, Vec2, pos2};
use engine_core::scene::{
    NodeState,
    spatial::{Role, matrices},
};
use glam::Vec3;

use crate::{project::Project, theme};

const CAMERA_DISTANCE: f32 = 10.0;
const MIN_PITCH: f32 = 5.0 * std::f32::consts::PI / 180.0;
const MAX_PITCH: f32 = 85.0 * std::f32::consts::PI / 180.0;

/// Orbit camera shared by the spatial overlay and its picking tools.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Camera {
    yaw: f32,
    pitch: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            // Preserve the former isometric orientation: looking from the
            // negative X/Z corner with the ground plane rising up-screen.
            yaw: -3.0 * std::f32::consts::FRAC_PI_4,
            pitch: 0.34,
        }
    }
}

impl Camera {
    /// Orbits the camera with a pointer drag in screen pixels.
    pub(super) fn orbit(&mut self, delta: Vec2) {
        self.yaw = (self.yaw - delta.x * 0.01).rem_euclid(std::f32::consts::TAU);
        self.pitch = (self.pitch - delta.y * 0.01).clamp(MIN_PITCH, MAX_PITCH);
    }

    fn eye(self) -> Vec3 {
        let horizontal = self.pitch.cos();
        Vec3::new(
            horizontal * self.yaw.sin(),
            self.pitch.sin(),
            horizontal * self.yaw.cos(),
        ) * CAMERA_DISTANCE
    }

    fn basis(self) -> (Vec3, Vec3, Vec3) {
        let forward = -self.eye().normalize();
        let right = Vec3::Y.cross(forward).normalize();
        let up = forward.cross(right).normalize();
        (right, up, forward)
    }

    /// Projects a world point into the design rectangle.
    pub(super) fn project(self, point: Vec3, origin: Pos2, scale: f32) -> Pos2 {
        let (right, up, forward) = self.basis();
        let relative = point - self.eye();
        let depth = relative.dot(forward).max(0.05);
        let focal = scale * CAMERA_DISTANCE;
        pos2(
            origin.x + relative.dot(right) / depth * focal,
            origin.y - relative.dot(up) / depth * focal,
        )
    }

    fn screen_to_world(self, point: Pos2, plane_z: f32, origin: Pos2, scale: f32) -> Option<Vec3> {
        let (right, up, forward) = self.basis();
        let focal = scale * CAMERA_DISTANCE;
        let x = (point.x - origin.x) / focal;
        let y = -(point.y - origin.y) / focal;
        let direction = (forward + right * x + up * y).normalize();
        if direction.z.abs() < 1e-4 {
            return None;
        }
        let distance = (plane_z - self.eye().z) / direction.z;
        (distance.is_finite() && distance > 0.0).then(|| self.eye() + direction * distance)
    }

    /// Converts a screen drag into world movement on the node's XY plane.
    pub(super) fn screen_delta_to_world(
        self,
        position: Vec3,
        delta: Vec2,
        origin: Pos2,
        scale: f32,
    ) -> Vec3 {
        let screen = self.project(position, origin, scale);
        let Some(target) = self.screen_to_world(screen + delta, position.z, origin, scale) else {
            return Vec3::ZERO;
        };
        target - position
    }
}

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
    pub camera: Camera,
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
    let camera = interaction.camera;
    let origin = design.center();
    let scale = (design.width().min(design.height()) / 10.0).clamp(12.0, 64.0);
    let screen_position = |position: Vec3| camera.project(position, origin, scale);
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
            markers.push((index, center, rect.expand(4.0), color, position));
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
                position,
            ));
        }
        if selected {
            let axes = gizmo_axes(camera, origin, scale, position);
            match interaction.mode {
                Mode::Move => {
                    for (axis, tint) in axes {
                        painter.arrow(center, axis, Stroke::new(1.8, tint));
                    }
                }
                Mode::Rotate => {
                    painter.circle_stroke(center, scale * 0.6, Stroke::new(1.8, theme::ACCENT));
                }
                Mode::Scale => {
                    for (axis, tint) in axes {
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
                .filter_map(|(index, center, bounds, _, position)| {
                    (bounds.contains(pointer)
                        || (project.selected == Some(*index)
                            && gizmo_hit(
                                pointer,
                                *center,
                                &gizmo_axes(camera, origin, scale, *position),
                                scale,
                                interaction.mode,
                            )))
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
                let world_position = worlds.get(&node.id).map_or_else(
                    || Vec3::from(spatial.position),
                    |world| world.transform_point3(Vec3::ZERO),
                );
                let world_delta =
                    camera.screen_delta_to_world(world_position, delta, origin, scale);
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

    for (index, center, bounds, color, _) in markers {
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

fn gizmo_axes(camera: Camera, origin: Pos2, scale: f32, position: Vec3) -> [(Vec2, Color32); 3] {
    let base = camera.project(position, origin, scale);
    let axis = |offset: Vec3| camera.project(position + offset, origin, scale) - base;
    [
        (axis(Vec3::X * 0.5), theme::AXIS_X),
        (axis(Vec3::Y), theme::AXIS_Y),
        (axis(Vec3::Z * 0.5), theme::ANCHOR),
    ]
}

fn gizmo_hit(
    pointer: egui::Pos2,
    center: egui::Pos2,
    axes: &[(Vec2, Color32); 3],
    scale: f32,
    mode: Mode,
) -> bool {
    let delta = pointer - center;
    match mode {
        Mode::Move => axes
            .iter()
            .any(|(axis, _)| segment_distance(delta, *axis) <= 6.0),
        Mode::Rotate => (delta.length() - scale * 0.6).abs() <= 6.0,
        Mode::Scale => axes.iter().any(|(axis, _)| (delta - *axis).length() <= 7.0),
    }
}

fn segment_distance(point: Vec2, end: Vec2) -> f32 {
    if end.length_sq() < 1e-6 {
        return point.length();
    }
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
    fn perspective_camera_projects_the_target_to_the_origin() {
        let camera = Camera::default();
        let origin = pos2(120.0, 80.0);
        let projected = camera.project(Vec3::ZERO, origin, 40.0);
        assert!((projected - origin).length() < 0.001);
    }

    #[test]
    fn screen_delta_moves_cubes_in_the_projected_xy_plane() {
        let camera = Camera::default();
        let origin = pos2(120.0, 80.0);
        let position = Vec3::new(-2.0, 1.0, 3.0);
        let scale = 40.0;
        let screen = Vec2::new(12.0, -20.0);
        let world = camera.screen_delta_to_world(position, screen, origin, scale);
        let projected = camera.project(position + world, origin, scale)
            - camera.project(position, origin, scale);
        assert!((projected - screen).length() < 0.001);
        assert!(world.z.abs() < 0.001);
    }

    #[test]
    fn orbit_changes_the_view_and_clamps_the_pitch() {
        let mut camera = Camera::default();
        let before = camera.project(Vec3::X, pos2(0.0, 0.0), 40.0);
        camera.orbit(Vec2::new(30.0, 0.0));
        assert_ne!(camera, Camera::default());
        assert_ne!(camera.project(Vec3::X, pos2(0.0, 0.0), 40.0), before);

        camera.orbit(Vec2::new(0.0, 10_000.0));
        assert_eq!(camera.pitch, MIN_PITCH);
        camera.orbit(Vec2::new(0.0, -10_000.0));
        assert_eq!(camera.pitch, MAX_PITCH);
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
        let axes = [
            (Vec2::new(10.0, -5.0), theme::AXIS_X),
            (Vec2::new(0.0, -40.0), theme::AXIS_Y),
            (Vec2::new(-10.0, -5.0), theme::ANCHOR),
        ];
        assert!(gizmo_hit(pos2(20.0, 15.0), center, &axes, 40.0, Mode::Move));
        assert!(gizmo_hit(
            pos2(10.0, -4.0),
            center,
            &axes,
            40.0,
            Mode::Rotate
        ));
        assert!(gizmo_hit(
            pos2(10.0, -20.0),
            center,
            &axes,
            40.0,
            Mode::Scale
        ));
    }
}
