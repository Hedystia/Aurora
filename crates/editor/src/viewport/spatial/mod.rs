//! Orbitable perspective spatial-node editing overlays.

use egui::{Color32, Painter, Pos2, Rect, Response, Stroke, Vec2, pos2};
use engine_core::{
    ecs::EntityId,
    scene::{
        NodeState,
        spatial::{Role, matrices},
    },
};
use glam::{Mat4, Vec3};
use std::collections::BTreeMap;

/// Pixel-to-metre scale used when a 2D sprite is placed in the 3D view.
const PIXELS_PER_METRE: f32 = 32.0;

use crate::{project::Project, theme};

const CAMERA_DISTANCE: f32 = 10.0;
const MIN_PITCH: f32 = 5.0 * std::f32::consts::PI / 180.0;
const MAX_PITCH: f32 = 85.0 * std::f32::consts::PI / 180.0;
const MIN_DISTANCE: f32 = 1.0;
const MAX_DISTANCE: f32 = 1000.0;

/// Orbit camera shared by the spatial overlay and its picking tools.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Camera {
    yaw: f32,
    pitch: f32,
    target: Vec3,
    distance: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            // Preserve the former isometric orientation: looking from the
            // negative X/Z corner with the ground plane rising up-screen.
            yaw: -3.0 * std::f32::consts::FRAC_PI_4,
            pitch: 0.34,
            target: Vec3::ZERO,
            distance: CAMERA_DISTANCE,
        }
    }
}

impl Camera {
    /// Orbits the camera with a pointer drag in screen pixels.
    pub(super) fn orbit(&mut self, delta: Vec2) {
        self.yaw = (self.yaw - delta.x * 0.01).rem_euclid(std::f32::consts::TAU);
        self.pitch = (self.pitch - delta.y * 0.01).clamp(MIN_PITCH, MAX_PITCH);
    }

    /// Pans the look-at target in the camera's screen plane.
    pub(super) fn pan(&mut self, delta: Vec2, scale: f32) {
        let (right, up, _) = self.basis();
        let units = self.distance / (scale * CAMERA_DISTANCE);
        self.target -= right * (delta.x * units) + up * (-delta.y * units);
    }

    /// Dollies the camera. Positive delta zooms out.
    pub(super) fn zoom(&mut self, delta: f32) {
        self.distance = (self.distance * (delta * 0.003).exp()).clamp(MIN_DISTANCE, MAX_DISTANCE);
    }

    fn eye(self) -> Vec3 {
        let horizontal = self.pitch.cos();
        self.target
            + Vec3::new(
                horizontal * self.yaw.sin(),
                self.pitch.sin(),
                horizontal * self.yaw.cos(),
            ) * self.distance
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
        let focal = scale * self.distance;
        pos2(
            origin.x + relative.dot(right) / depth * focal,
            origin.y - relative.dot(up) / depth * focal,
        )
    }

    fn screen_to_world(self, point: Pos2, plane_z: f32, origin: Pos2, scale: f32) -> Option<Vec3> {
        let (right, up, forward) = self.basis();
        let focal = scale * self.distance;
        let x = (point.x - origin.x) / focal;
        let y = -(point.y - origin.y) / focal;
        let direction = (forward + right * x + up * y).normalize();
        if direction.z.abs() < 1e-4 {
            return None;
        }
        let distance = (plane_z - self.eye().z) / direction.z;
        (distance.is_finite() && distance > 0.0).then(|| self.eye() + direction * distance)
    }

    /// Intersects a screen point with the ground plane (`y = 0`).
    pub(super) fn screen_to_ground(self, point: Pos2, origin: Pos2, scale: f32) -> Option<Vec3> {
        let (right, up, forward) = self.basis();
        let focal = scale * self.distance;
        let x = (point.x - origin.x) / focal;
        let y = -(point.y - origin.y) / focal;
        let direction = (forward + right * x + up * y).normalize();
        if direction.y.abs() < 1e-4 {
            return None;
        }
        let distance = -self.eye().y / direction.y;
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
    pub axis: &'a mut Option<usize>,
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

    // Non-cuboid colliders get a distinct projected outline so the chosen
    // shape and trigger flag are visible while authoring.
    for node in nodes.iter().filter(|node| node.visible) {
        let Some(spatial) = &node.spatial else {
            continue;
        };
        if spatial.body == engine_core::scene::spatial::Body::None
            || spatial.collider == engine_core::scene::spatial::Collider::Cuboid
        {
            continue;
        }
        let Some(world) = worlds.get(&node.id) else {
            continue;
        };
        let center = screen_position(world.transform_point3(Vec3::ZERO));
        let radius = (world
            .transform_vector3(Vec3::X * spatial.scale[0] * 0.5)
            .length()
            / CAMERA_DISTANCE
            * scale)
            .clamp(2.0, 400.0);
        let tint = if spatial.trigger {
            theme::WARNING
        } else {
            theme::ACCENT
        };
        let stroke = Stroke::new(1.2, tint);
        match spatial.collider {
            engine_core::scene::spatial::Collider::Sphere => {
                painter.circle_stroke(center, radius, stroke);
            }
            engine_core::scene::spatial::Collider::Capsule => {
                let half = radius * 0.6;
                painter.line_segment([center - Vec2::Y * half, center + Vec2::Y * half], stroke);
                painter.circle_stroke(center - Vec2::Y * half, radius * 0.5, stroke);
                painter.circle_stroke(center + Vec2::Y * half, radius * 0.5, stroke);
            }
            engine_core::scene::spatial::Collider::Mesh
            | engine_core::scene::spatial::Collider::ConvexHull => {
                painter.circle_stroke(center, radius, stroke);
            }
            engine_core::scene::spatial::Collider::Cuboid => {}
        }
    }

    let pointer = response.interact_pointer_pos();
    let primary_drag_started = response.drag_started_by(egui::PointerButton::Primary);
    let mut picked_axis = None;
    let picked = response
        .clicked()
        .then_some(pointer)
        .flatten()
        .or_else(|| primary_drag_started.then_some(pointer).flatten())
        .and_then(|pointer| {
            markers
                .iter()
                .filter_map(|(index, center, bounds, _, position)| {
                    let hit_axis = if project.selected == Some(*index) {
                        gizmo_axis(
                            pointer,
                            *center,
                            &gizmo_axes(camera, origin, scale, *position),
                            scale,
                            interaction.mode,
                        )
                    } else {
                        None
                    };
                    if bounds.contains(pointer) || hit_axis.is_some() {
                        if hit_axis.is_some() {
                            picked_axis = hit_axis;
                        }
                        Some((*index, pointer.distance(*center)))
                    } else {
                        None
                    }
                })
                .min_by(|left, right| left.1.total_cmp(&right.1))
                .map(|(index, _)| index)
        });

    if primary_drag_started {
        *interaction.dragging = picked;
        *interaction.axis = picked_axis;
    }
    if response.drag_stopped_by(egui::PointerButton::Primary) {
        *interaction.axis = None;
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
                let mut local_delta = parent.inverse().transform_vector3(world_delta);
                if let Some(axis) = *interaction.axis {
                    let mut constrained = Vec3::ZERO;
                    constrained[axis] = local_delta[axis];
                    local_delta = constrained;
                }
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
                        let axis = interaction.axis.unwrap_or(2);
                        spatial.rotation[axis] += rotation.to_degrees();
                        project.selected = Some(index);
                        project.changed();
                    }
                }
            }
            Mode::Scale => {
                let factor = scale_factor(delta.y);
                let mut changed = false;
                for (axis, value) in spatial.scale.iter_mut().enumerate() {
                    if interaction.axis.is_some_and(|selected| selected != axis) {
                        continue;
                    }
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

fn gizmo_axis(
    pointer: egui::Pos2,
    center: egui::Pos2,
    axes: &[(Vec2, Color32); 3],
    scale: f32,
    mode: Mode,
) -> Option<usize> {
    let delta = pointer - center;
    match mode {
        Mode::Move => axes
            .iter()
            .enumerate()
            .filter(|(_, (axis, _))| segment_distance(delta, *axis) <= 6.0)
            .min_by(|(_, (left, _)), (_, (right, _))| {
                segment_distance(delta, *left).total_cmp(&segment_distance(delta, *right))
            })
            .map(|(index, _)| index),
        Mode::Rotate => ((delta.length() - scale * 0.6).abs() <= 6.0).then_some(2),
        Mode::Scale => axes
            .iter()
            .enumerate()
            .filter(|(_, (axis, _))| (delta - *axis).length() <= 7.0)
            .min_by(|(_, (left, _)), (_, (right, _))| {
                (delta - *left)
                    .length()
                    .total_cmp(&(delta - *right).length())
            })
            .map(|(index, _)| index),
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

/// Projects one sprite quad into the perspective view.
///
/// Spatial nodes use their authored world matrix. Plain 2D nodes are placed on
/// the `z = 0` plane at the engine's 32-pixel-per-metre scale so a mixed scene
/// shows its HUD beside the 3D geometry.
pub(super) fn project_sprite_point(
    camera: Camera,
    worlds: &BTreeMap<EntityId, Mat4>,
    node: &NodeState,
    point: [f32; 2],
    origin: Pos2,
    scale: f32,
) -> Pos2 {
    let local = if node.spatial.is_some() {
        Vec3::new(
            point[0] / PIXELS_PER_METRE,
            -point[1] / PIXELS_PER_METRE,
            0.0,
        )
    } else {
        let world = node.world.point(point);
        Vec3::new(
            world[0] / PIXELS_PER_METRE,
            -world[1] / PIXELS_PER_METRE,
            0.0,
        )
    };
    let matrix = worlds.get(&node.id).copied().unwrap_or(Mat4::IDENTITY);
    camera.project(matrix.transform_point3(local), origin, scale)
}

/// Draws an adaptive XZ ground grid with coloured origin axes.
pub(super) fn draw_grid(painter: &Painter, design: Rect, camera: Camera, scale: f32) {
    let origin = design.center();
    let raw = (camera.distance / 8.0).max(0.25);
    let step = if raw < 1.0 { 1.0 } else { raw.round().max(1.0) };
    let extent = (camera.distance * 3.0).clamp(12.0, 600.0);
    let limit = (extent / step).ceil() as i32;
    let project = |x: f32, z: f32| camera.project(Vec3::new(x, 0.0, z), origin, scale);
    for index in -limit..=limit {
        let value = index as f32 * step;
        let tint = if index == 0 {
            theme::AXIS_X
        } else {
            theme::GRID
        };
        painter.line_segment(
            [project(value, -extent), project(value, extent)],
            Stroke::new(if index == 0 { 1.6 } else { 1.0 }, tint),
        );
        let tint = if index == 0 {
            theme::ANCHOR
        } else {
            theme::GRID
        };
        painter.line_segment(
            [project(-extent, value), project(extent, value)],
            Stroke::new(if index == 0 { 1.6 } else { 1.0 }, tint),
        );
    }
}

/// Draws a corner orientation widget with labelled X, Y and Z axes.
pub(super) fn draw_axis_widget(painter: &Painter, rect: Rect, camera: Camera) {
    let center = pos2(rect.right() - 42.0, rect.bottom() - 46.0);
    let (right, up, _) = camera.basis();
    let axes = [
        (Vec3::X, theme::AXIS_X, "X"),
        (Vec3::Y, theme::AXIS_Y, "Y"),
        (Vec3::Z, theme::ANCHOR, "Z"),
    ];
    for (axis, tint, label) in axes {
        let projected = right * axis.dot(right) + up * axis.dot(up);
        let end = center + Vec2::new(projected.x, -projected.y) * 20.0;
        painter.line_segment([center, end], Stroke::new(2.0, tint));
        painter.circle_filled(end, 3.0, tint);
        painter.text(
            end + Vec2::new(6.0, -6.0),
            egui::Align2::CENTER_CENTER,
            label,
            theme::font(),
            tint,
        );
    }
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
    fn panning_and_zooming_change_the_projected_view() {
        let mut camera = Camera::default();
        let origin = pos2(120.0, 80.0);
        let before = camera.project(Vec3::ZERO, origin, 40.0);
        camera.pan(Vec2::new(30.0, 12.0), 40.0);
        assert_ne!(camera.project(Vec3::ZERO, origin, 40.0), before);
        camera.zoom(10.0);
        assert!(camera.distance > CAMERA_DISTANCE);
        camera.zoom(-10.0);
        assert!((camera.distance - CAMERA_DISTANCE).abs() < 0.001);
    }

    #[test]
    fn ground_plane_intersection_maps_the_screen_back_to_xz() {
        let camera = Camera::default();
        let origin = pos2(120.0, 80.0);
        let point = camera.project(Vec3::new(2.0, 0.0, -1.0), origin, 40.0);
        let ground = camera
            .screen_to_ground(point, origin, 40.0)
            .unwrap_or(Vec3::NEG_INFINITY);
        assert!((ground.x - 2.0).abs() < 0.01);
        assert!((ground.z + 1.0).abs() < 0.01);
    }

    #[test]
    fn transform_tools_pick_their_visible_handles() {
        let center = pos2(10.0, 20.0);
        let axes = [
            (Vec2::new(10.0, -5.0), theme::AXIS_X),
            (Vec2::new(0.0, -40.0), theme::AXIS_Y),
            (Vec2::new(-10.0, -5.0), theme::ANCHOR),
        ];
        assert!(gizmo_axis(pos2(20.0, 15.0), center, &axes, 40.0, Mode::Move).is_some());
        assert!(gizmo_axis(pos2(10.0, -4.0), center, &axes, 40.0, Mode::Rotate).is_some());
        assert!(gizmo_axis(pos2(10.0, -20.0), center, &axes, 40.0, Mode::Scale).is_some());
    }
}
