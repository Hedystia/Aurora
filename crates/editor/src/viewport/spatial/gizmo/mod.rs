//! Transform gizmos in the style of Unity and Roblox Studio: move arrows and
//! plane handles, rotation rings, axis and uniform scale handles, snapping
//! and world or local orientation.

use egui::{Color32, Painter, Pos2, Shape, Stroke, Vec2, pos2};
use glam::{EulerRot, Mat4, Quat, Vec3};

use super::camera::View;
use crate::theme;

/// On-screen length of a gizmo axis in pixels, independent of distance.
const GIZMO_PIXELS: f32 = 96.0;
const AXIS_PICK: f32 = 8.0;
const RING_SEGMENTS: usize = 72;
const PLANE_NEAR: f32 = 0.22;
const PLANE_FAR: f32 = 0.46;

/// The active scene-view tool (Q, W, E and R).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tool {
    /// Select and drag objects without handles.
    Select,
    /// Translate with arrows and plane handles.
    #[default]
    Move,
    /// Rotate with rings.
    Rotate,
    /// Scale per axis or uniformly.
    Scale,
}

/// Gizmo orientation for move and rotate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Space {
    /// World axes.
    #[default]
    World,
    /// The node's own axes.
    Local,
}

/// Increment snapping, toggled from the toolbar or held with Ctrl.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Snap {
    /// Snapping is on without holding Ctrl.
    pub enabled: bool,
    /// Translation increment in metres.
    pub step: f32,
    /// Rotation increment in degrees.
    pub angle: f32,
    /// Scale increment.
    pub scale: f32,
}

impl Default for Snap {
    fn default() -> Self {
        Self {
            enabled: false,
            step: 0.5,
            angle: 15.0,
            scale: 0.1,
        }
    }
}

/// A grabbable part of the gizmo, or the object body itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Handle {
    /// Move or scale along one axis.
    Axis(usize),
    /// Move in the plane whose normal is this axis.
    Plane(usize),
    /// Move in the screen plane, or scale uniformly.
    Center,
    /// Rotate around this axis.
    Ring(usize),
    /// Rotate around the view direction.
    ViewRing,
    /// Drag the object across the horizontal plane through it.
    Body,
}

/// Gizmo placement and orientation for one frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Frame {
    /// World-space pivot.
    pub origin: Vec3,
    /// Unit world directions of the gizmo axes.
    pub axes: [Vec3; 3],
    /// World length matching [`GIZMO_PIXELS`] at the pivot.
    pub size: f32,
}

impl Frame {
    /// Orients the gizmo for a node's world matrix.
    pub(crate) fn new(view: &View, world: Mat4, space: Space, tool: Tool) -> Self {
        let (_, rotation, origin) = world.to_scale_rotation_translation();
        let local = space == Space::Local || tool == Tool::Scale;
        let axes = [Vec3::X, Vec3::Y, Vec3::Z].map(|axis| {
            if local {
                (rotation * axis).normalize_or(axis)
            } else {
                axis
            }
        });
        Self {
            origin,
            axes,
            size: view.pixel_size(origin) * GIZMO_PIXELS,
        }
    }

    fn others(axis: usize) -> (usize, usize) {
        ((axis + 1) % 3, (axis + 2) % 3)
    }

    fn plane_quad(&self, view: &View, axis: usize) -> Option<[Pos2; 4]> {
        let (j, k) = Self::others(axis);
        let (a, b) = (self.axes[j] * self.size, self.axes[k] * self.size);
        let corners = [
            (PLANE_NEAR, PLANE_NEAR),
            (PLANE_FAR, PLANE_NEAR),
            (PLANE_FAR, PLANE_FAR),
            (PLANE_NEAR, PLANE_FAR),
        ];
        let mut quad = [Pos2::ZERO; 4];
        for (slot, (u, v)) in quad.iter_mut().zip(corners) {
            *slot = view.project(self.origin + a * u + b * v)?;
        }
        Some(quad)
    }

    fn facing(&self, view: &View, axis: usize) -> f32 {
        self.axes[axis]
            .dot((view.eye() - self.origin).normalize_or_zero())
            .abs()
    }

    fn ring(&self, view: &View, axis: usize) -> Vec<(Pos2, bool)> {
        let (j, k) = Self::others(axis);
        let radius = self.size * 0.9;
        let toward = (view.eye() - self.origin).normalize_or_zero();
        (0..=RING_SEGMENTS)
            .filter_map(|step| {
                let angle = step as f32 / RING_SEGMENTS as f32 * std::f32::consts::TAU;
                let offset = (self.axes[j] * angle.cos() + self.axes[k] * angle.sin()) * radius;
                let front = offset.normalize_or_zero().dot(toward) >= -0.05;
                view.project(self.origin + offset)
                    .map(|point| (point, front))
            })
            .collect()
    }
}

fn segment_distance(point: Pos2, from: Pos2, to: Pos2) -> f32 {
    let line = to - from;
    if line.length_sq() < 1e-6 {
        return point.distance(from);
    }
    let t = ((point - from).dot(line) / line.length_sq()).clamp(0.0, 1.0);
    point.distance(from + line * t)
}

fn inside(quad: &[Pos2; 4], point: Pos2) -> bool {
    let cross = |a: Vec2, b: Vec2| a.x * b.y - a.y * b.x;
    let sides: Vec<f32> = (0..4)
        .map(|side| cross(quad[(side + 1) % 4] - quad[side], point - quad[side]))
        .collect();
    sides.iter().all(|side| *side >= 0.0) || sides.iter().all(|side| *side <= 0.0)
}

/// Finds the handle under the pointer.
pub(crate) fn hit(view: &View, frame: &Frame, tool: Tool, pointer: Pos2) -> Option<Handle> {
    let center = view.project(frame.origin)?;
    let tip = |axis: usize| view.project(frame.origin + frame.axes[axis] * frame.size);
    match tool {
        Tool::Select => None,
        Tool::Move => {
            if pointer.distance(center) <= 9.0 {
                return Some(Handle::Center);
            }
            if let Some(axis) = (0..3).find(|&axis| {
                frame.facing(view, axis) > 0.2
                    && frame
                        .plane_quad(view, axis)
                        .is_some_and(|quad| inside(&quad, pointer))
            }) {
                return Some(Handle::Plane(axis));
            }
            (0..3)
                .filter_map(|axis| {
                    let end = tip(axis)?;
                    ((end - center).length() > 12.0)
                        .then(|| (axis, segment_distance(pointer, center, end)))
                })
                .filter(|(_, distance)| *distance <= AXIS_PICK)
                .min_by(|left, right| left.1.total_cmp(&right.1))
                .map(|(axis, _)| Handle::Axis(axis))
        }
        Tool::Rotate => {
            let nearest = (0..3)
                .filter_map(|axis| {
                    let ring = frame.ring(view, axis);
                    ring.windows(2)
                        .filter(|pair| pair[0].1 && pair[1].1)
                        .map(|pair| segment_distance(pointer, pair[0].0, pair[1].0))
                        .min_by(f32::total_cmp)
                        .map(|distance| (axis, distance))
                })
                .filter(|(_, distance)| *distance <= AXIS_PICK)
                .min_by(|left, right| left.1.total_cmp(&right.1));
            if let Some((axis, _)) = nearest {
                return Some(Handle::Ring(axis));
            }
            ((pointer.distance(center) - GIZMO_PIXELS * 1.08).abs() <= AXIS_PICK)
                .then_some(Handle::ViewRing)
        }
        Tool::Scale => {
            if pointer.distance(center) <= 10.0 {
                return Some(Handle::Center);
            }
            (0..3)
                .filter_map(|axis| {
                    let end = tip(axis)?;
                    let distance = segment_distance(pointer, center, end)
                        .min(pointer.distance(end) - 3.0)
                        .max(0.0);
                    ((end - center).length() > 12.0).then_some((axis, distance))
                })
                .filter(|(_, distance)| *distance <= AXIS_PICK)
                .min_by(|left, right| left.1.total_cmp(&right.1))
                .map(|(axis, _)| Handle::Axis(axis))
        }
    }
}

fn axis_color(axis: usize) -> Color32 {
    [theme::AXIS_X, theme::AXIS_Y, theme::AXIS_Z][axis]
}

/// Paints the gizmo with the hovered or dragged handle highlighted.
pub(crate) fn draw(
    painter: &Painter,
    view: &View,
    frame: &Frame,
    tool: Tool,
    hot: Option<Handle>,
    active: Option<Handle>,
) {
    let Some(center) = view.project(frame.origin) else {
        return;
    };
    let lit = |handle: Handle| hot == Some(handle) || active == Some(handle);
    let tint = |handle: Handle, base: Color32| {
        if lit(handle) {
            theme::GIZMO_HOT
        } else if active.is_some() {
            base.gamma_multiply(0.45)
        } else {
            base
        }
    };
    let tip = |axis: usize| view.project(frame.origin + frame.axes[axis] * frame.size);
    match tool {
        Tool::Select => {}
        Tool::Move => {
            for axis in 0..3 {
                if frame.facing(view, axis) <= 0.2 {
                    continue;
                }
                if let Some(quad) = frame.plane_quad(view, axis) {
                    let color = tint(Handle::Plane(axis), axis_color(axis));
                    painter.add(Shape::convex_polygon(
                        quad.to_vec(),
                        color.gamma_multiply(if lit(Handle::Plane(axis)) { 0.55 } else { 0.3 }),
                        Stroke::new(1.2, color),
                    ));
                }
            }
            for axis in 0..3 {
                let Some(end) = tip(axis) else {
                    continue;
                };
                if (end - center).length() <= 12.0 {
                    continue;
                }
                let color = tint(Handle::Axis(axis), axis_color(axis));
                let direction = (end - center).normalized();
                let base = end - direction * 15.0;
                painter.line_segment([center, base], Stroke::new(2.6, color));
                let side = direction.rot90() * 5.5;
                painter.add(Shape::convex_polygon(
                    vec![end, base + side, base - side],
                    color,
                    Stroke::NONE,
                ));
            }
            let color = tint(Handle::Center, Color32::WHITE);
            painter.rect_stroke(
                egui::Rect::from_center_size(center, Vec2::splat(11.0)),
                2.0,
                Stroke::new(1.6, color),
                egui::StrokeKind::Middle,
            );
        }
        Tool::Rotate => {
            painter.circle_filled(center, GIZMO_PIXELS * 0.9, Color32::from_white_alpha(10));
            painter.circle_stroke(
                center,
                GIZMO_PIXELS * 1.08,
                Stroke::new(1.6, tint(Handle::ViewRing, theme::TEXT)),
            );
            for axis in 0..3 {
                let color = tint(Handle::Ring(axis), axis_color(axis));
                let ring = frame.ring(view, axis);
                for pair in ring.windows(2) {
                    let front = pair[0].1 && pair[1].1;
                    painter.line_segment(
                        [pair[0].0, pair[1].0],
                        if front {
                            Stroke::new(2.4, color)
                        } else {
                            Stroke::new(1.0, color.gamma_multiply(0.3))
                        },
                    );
                }
            }
        }
        Tool::Scale => {
            for axis in 0..3 {
                let Some(end) = tip(axis) else {
                    continue;
                };
                if (end - center).length() <= 12.0 {
                    continue;
                }
                let color = tint(Handle::Axis(axis), axis_color(axis));
                painter.line_segment([center, end], Stroke::new(2.4, color));
                painter.rect_filled(
                    egui::Rect::from_center_size(end, Vec2::splat(10.0)),
                    1.5,
                    color,
                );
            }
            painter.rect_filled(
                egui::Rect::from_center_size(center, Vec2::splat(12.0)),
                2.0,
                tint(Handle::Center, Color32::from_gray(225)),
            );
        }
    }
}

/// Parent-relative pose being edited.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Pose {
    /// Parent-relative position.
    pub position: [f32; 3],
    /// XYZ Euler rotation in degrees.
    pub rotation: [f32; 3],
    /// Local scale.
    pub scale: [f32; 3],
}

/// A drag in progress, frozen at its starting state so it never drifts.
#[derive(Clone, Debug)]
pub(crate) struct Drag {
    /// Document index of the dragged node.
    pub index: usize,
    /// Grabbed handle.
    pub handle: Handle,
    /// Text shown next to the pointer (offset, angle or factor).
    pub readout: String,
    frame: Frame,
    parent: Mat4,
    start: Pose,
    start_pointer: Pos2,
    plane_normal: Vec3,
    anchor: Vec3,
    anchor_scalar: f32,
    screen_angle: bool,
    last_angle: f32,
    total_angle: f32,
}

fn screen_angle(view: &View, origin: Vec3, pointer: Pos2) -> Option<f32> {
    let center = view.project(origin)?;
    let offset = pointer - center;
    (offset.length() > 2.0).then(|| offset.y.atan2(offset.x))
}

fn wrap(angle: f32) -> f32 {
    (angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

fn snap_value(value: f32, step: Option<f32>) -> f32 {
    match step {
        Some(step) if step > 0.0 => (value / step).round() * step,
        _ => value,
    }
}

impl Drag {
    /// Starts a drag on a handle. Returns None when the view is edge-on to it.
    pub(crate) fn begin(
        view: &View,
        frame: Frame,
        handle: Handle,
        pointer: Pos2,
        index: usize,
        parent: Mat4,
        start: Pose,
    ) -> Option<Self> {
        let mut drag = Self {
            index,
            handle,
            readout: String::new(),
            frame,
            parent,
            start,
            start_pointer: pointer,
            plane_normal: view.forward(),
            anchor: frame.origin,
            anchor_scalar: 0.0,
            screen_angle: false,
            last_angle: 0.0,
            total_angle: 0.0,
        };
        let (_, direction) = view.ray(pointer);
        match handle {
            Handle::Axis(axis) => {
                drag.anchor_scalar = view.along_line(pointer, frame.origin, frame.axes[axis])?;
            }
            Handle::Plane(axis) => {
                drag.plane_normal = frame.axes[axis];
                drag.anchor = view.on_plane(pointer, frame.origin, drag.plane_normal)?;
            }
            Handle::Center => {
                drag.anchor = view.on_plane(pointer, frame.origin, drag.plane_normal)?;
            }
            Handle::Body => {
                // Roblox-style: slide across the horizontal plane through the
                // object unless the camera is nearly level with it.
                if direction.y.abs() > 0.12 {
                    drag.plane_normal = Vec3::Y;
                }
                drag.anchor = view.on_plane(pointer, frame.origin, drag.plane_normal)?;
            }
            Handle::Ring(axis) => {
                let normal = frame.axes[axis];
                match view
                    .on_plane(pointer, frame.origin, normal)
                    .filter(|_| direction.dot(normal).abs() > 0.15)
                {
                    Some(point) => drag.anchor = point - frame.origin,
                    None => {
                        drag.screen_angle = true;
                        drag.last_angle = screen_angle(view, frame.origin, pointer)?;
                    }
                }
            }
            Handle::ViewRing => {
                drag.screen_angle = true;
                drag.last_angle = screen_angle(view, frame.origin, pointer)?;
            }
        }
        Some(drag)
    }

    /// Computes the pose for the current pointer. `snap` holds the active increments.
    pub(crate) fn update(
        &mut self,
        view: &View,
        tool: Tool,
        pointer: Pos2,
        snap: Option<Snap>,
    ) -> Option<Pose> {
        let mut pose = self.start;
        let frame = self.frame;
        match (tool, self.handle) {
            (Tool::Rotate, Handle::Ring(_) | Handle::ViewRing) => {
                let axis = match self.handle {
                    Handle::Ring(axis) => frame.axes[axis],
                    _ => -view.forward(),
                };
                let raw = if self.screen_angle {
                    screen_angle(view, frame.origin, pointer)?
                } else {
                    let point = view.on_plane(pointer, frame.origin, axis)? - frame.origin;
                    let start = self.anchor;
                    axis.dot(start.cross(point)).atan2(start.dot(point))
                };
                // Accumulate wrapped steps so a drag can turn past half a revolution.
                self.total_angle += wrap(raw - self.last_angle);
                self.last_angle = raw;
                let angle = if self.screen_angle {
                    // Screen angles grow clockwise; a positive turn about an
                    // axis facing the viewer is counter-clockwise.
                    let toward = axis.dot(view.eye() - frame.origin);
                    -self.total_angle * if toward < 0.0 { -1.0 } else { 1.0 }
                } else {
                    self.total_angle
                };
                let degrees = snap_value(angle.to_degrees(), snap.map(|s| s.angle));
                let parent = self.parent.to_scale_rotation_translation().1;
                let [x, y, z] = self.start.rotation.map(f32::to_radians);
                let local = Quat::from_euler(EulerRot::XYZ, x, y, z);
                let turn = Quat::from_axis_angle(axis.normalize_or(Vec3::Z), degrees.to_radians());
                let next = (parent.inverse() * turn * parent * local).normalize();
                let (x, y, z) = next.to_euler(EulerRot::XYZ);
                pose.rotation = [x, y, z].map(f32::to_degrees);
                self.readout = format!("{degrees:.1}°");
            }
            (Tool::Scale, Handle::Axis(axis)) => {
                let center = view.project(frame.origin)?;
                let screen = view.project(frame.origin + frame.axes[axis] * frame.size)? - center;
                let length = screen.length_sq();
                if length < 1.0 {
                    return None;
                }
                let factor = 1.0 + (pointer - self.start_pointer).dot(screen) / length;
                let value = snap_value(self.start.scale[axis] * factor, snap.map(|s| s.scale));
                pose.scale[axis] = value.clamp(0.001, 1e5);
                self.readout = format!("{:.2}", pose.scale[axis]);
            }
            (Tool::Scale, Handle::Center) => {
                let moved = pointer - self.start_pointer;
                let factor = ((moved.x - moved.y) * 0.008).exp();
                for (value, start) in pose.scale.iter_mut().zip(self.start.scale) {
                    *value = snap_value(start * factor, snap.map(|s| s.scale)).clamp(0.001, 1e5);
                }
                self.readout = format!("×{factor:.2}");
            }
            (_, handle) => {
                let step = snap.map(|s| s.step);
                let offset = match handle {
                    Handle::Axis(axis) => {
                        let along = view.along_line(pointer, frame.origin, frame.axes[axis])?;
                        frame.axes[axis] * snap_value(along - self.anchor_scalar, step)
                    }
                    Handle::Plane(axis) => {
                        let point = view.on_plane(pointer, frame.origin, self.plane_normal)?;
                        let moved = point - self.anchor;
                        let (j, k) = Frame::others(axis);
                        frame.axes[j] * snap_value(moved.dot(frame.axes[j]), step)
                            + frame.axes[k] * snap_value(moved.dot(frame.axes[k]), step)
                    }
                    _ => {
                        let point = view.on_plane(pointer, frame.origin, self.plane_normal)?;
                        let moved = point - self.anchor;
                        Vec3::new(
                            snap_value(moved.x, step),
                            snap_value(moved.y, step),
                            snap_value(moved.z, step),
                        )
                    }
                };
                if !offset.is_finite() || offset.length() > 1e5 {
                    return None;
                }
                let world = frame.origin + offset;
                let local = self.parent.inverse().transform_point3(world);
                if !local.is_finite() {
                    return None;
                }
                pose.position = local.to_array();
                self.readout = format!("{:+.2}  {:+.2}  {:+.2}", offset.x, offset.y, offset.z);
            }
        }
        Some(pose)
    }
}

/// Screen point where a drag's readout label is drawn.
pub(crate) fn readout_position(pointer: Pos2) -> Pos2 {
    pos2(pointer.x + 16.0, pointer.y - 18.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport::spatial::camera::Camera;

    fn view() -> View {
        View::new(
            Camera::default(),
            egui::Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(800.0, 600.0)),
        )
    }

    fn pose() -> Pose {
        Pose {
            position: [0.0; 3],
            rotation: [0.0; 3],
            scale: [1.0; 3],
        }
    }

    #[test]
    fn dragging_the_x_arrow_moves_only_along_x() {
        let view = view();
        let frame = Frame::new(&view, Mat4::IDENTITY, Space::World, Tool::Move);
        let start = view
            .project(frame.origin + Vec3::X * frame.size * 0.6)
            .unwrap_or(Pos2::ZERO);
        assert_eq!(hit(&view, &frame, Tool::Move, start), Some(Handle::Axis(0)));
        let mut drag = Drag::begin(
            &view,
            frame,
            Handle::Axis(0),
            start,
            0,
            Mat4::IDENTITY,
            pose(),
        );
        let target = view
            .project(Vec3::X * (frame.size * 0.6 + 2.0))
            .unwrap_or(Pos2::ZERO);
        let moved = drag
            .as_mut()
            .and_then(|drag| drag.update(&view, Tool::Move, target, None));
        let position = moved.map_or([f32::NAN; 3], |pose| pose.position);
        assert!((position[0] - 2.0).abs() < 0.05, "{position:?}");
        assert!(position[1].abs() < 1e-4 && position[2].abs() < 1e-4);
    }

    #[test]
    fn snapping_rounds_translation_to_the_step() {
        let view = view();
        let frame = Frame::new(&view, Mat4::IDENTITY, Space::World, Tool::Move);
        let start = view.project(frame.origin).unwrap_or(Pos2::ZERO);
        let mut drag = Drag::begin(&view, frame, Handle::Body, start, 0, Mat4::IDENTITY, pose());
        let target = view.project(Vec3::new(1.3, 0.0, 0.7)).unwrap_or(Pos2::ZERO);
        let moved = drag.as_mut().and_then(|drag| {
            drag.update(
                &view,
                Tool::Move,
                target,
                Some(Snap {
                    step: 1.0,
                    ..Snap::default()
                }),
            )
        });
        assert_eq!(moved.map(|pose| pose.position), Some([1.0, 0.0, 1.0]));
    }

    #[test]
    fn the_y_ring_rotates_around_y() {
        let view = view();
        let frame = Frame::new(&view, Mat4::IDENTITY, Space::World, Tool::Rotate);
        let radius = frame.size * 0.9;
        let start = view
            .project(Vec3::new(0.0, 0.0, radius))
            .unwrap_or(Pos2::ZERO);
        let mut drag = Drag::begin(
            &view,
            frame,
            Handle::Ring(1),
            start,
            0,
            Mat4::IDENTITY,
            pose(),
        );
        let target = view
            .project(Vec3::new(radius, 0.0, 0.0))
            .unwrap_or(Pos2::ZERO);
        let rotated = drag
            .as_mut()
            .and_then(|drag| drag.update(&view, Tool::Rotate, target, None));
        let rotation = rotated.map_or([f32::NAN; 3], |pose| pose.rotation);
        assert!((rotation[1] - 90.0).abs() < 1.0, "{rotation:?}");
    }

    #[test]
    fn scale_handles_grow_along_their_axis() {
        let view = view();
        let frame = Frame::new(&view, Mat4::IDENTITY, Space::World, Tool::Scale);
        let center = view.project(frame.origin).unwrap_or(Pos2::ZERO);
        let end = view
            .project(frame.origin + Vec3::Y * frame.size)
            .unwrap_or(Pos2::ZERO);
        assert_eq!(hit(&view, &frame, Tool::Scale, end), Some(Handle::Axis(1)));
        let mut drag = Drag::begin(
            &view,
            frame,
            Handle::Axis(1),
            end,
            0,
            Mat4::IDENTITY,
            pose(),
        );
        let scaled = drag
            .as_mut()
            .and_then(|drag| drag.update(&view, Tool::Scale, end + (end - center), None));
        let scale = scaled.map_or([f32::NAN; 3], |pose| pose.scale);
        assert!(
            (scale[1] - 2.0).abs() < 0.01 && scale[0] == 1.0,
            "{scale:?}"
        );
    }
}
