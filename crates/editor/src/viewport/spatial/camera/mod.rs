//! Scene-view camera: orbit, fly, pan, dolly and framing in the style of
//! Unity's Scene view and Roblox Studio.
//!
//! The camera is right-handed with +Y up, matching the engine's
//! `look_at_rh` player camera, so the editor shows the same handedness as Play.

use egui::{Pos2, Rect, Vec2, pos2};
use glam::{Mat4, Vec3};

/// Vertical field of view of the editor camera.
pub(crate) const FOV_Y: f32 = 60.0 * std::f32::consts::PI / 180.0;
/// Near clipping distance in metres.
pub(crate) const NEAR: f32 = 0.05;
/// Distance from the pivot used by the default and reset view.
pub(crate) const DEFAULT_DISTANCE: f32 = 12.0;
const MIN_PITCH: f32 = -89.0 * std::f32::consts::PI / 180.0;
const MAX_PITCH: f32 = 89.0 * std::f32::consts::PI / 180.0;
const MIN_DISTANCE: f32 = 0.25;
const MAX_DISTANCE: f32 = 4000.0;
const ORBIT_SPEED: f32 = 0.008;
const LOOK_SPEED: f32 = 0.004;
const DOLLY_SPEED: f32 = 0.0018;
/// Default fly speed in metres per second.
pub(crate) const DEFAULT_FLY_SPEED: f32 = 6.0;
const MIN_FLY_SPEED: f32 = 0.25;
const MAX_FLY_SPEED: f32 = 400.0;
/// How quickly a framing animation converges, per second.
const FOCUS_RATE: f32 = 14.0;

/// Orbit/fly camera shared by the spatial overlay and its picking tools.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Camera {
    /// Rotation around world +Y in radians.
    pub yaw: f32,
    /// Elevation above the ground plane in radians.
    pub pitch: f32,
    /// Orbit pivot.
    pub target: Vec3,
    /// Distance from the eye to the pivot.
    pub distance: f32,
    /// Fly speed in metres per second.
    pub fly_speed: f32,
    goal: Option<Goal>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Goal {
    target: Vec3,
    distance: f32,
    angles: Option<(f32, f32)>,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            // Three-quarter view from the +X/+Z corner, slightly above.
            yaw: std::f32::consts::FRAC_PI_4,
            pitch: 0.48,
            target: Vec3::ZERO,
            distance: DEFAULT_DISTANCE,
            fly_speed: DEFAULT_FLY_SPEED,
            goal: None,
        }
    }
}

impl Camera {
    /// Unit vector from the pivot to the eye.
    fn direction(self) -> Vec3 {
        let horizontal = self.pitch.cos();
        Vec3::new(
            horizontal * self.yaw.sin(),
            self.pitch.sin(),
            horizontal * self.yaw.cos(),
        )
    }

    /// World-space eye position.
    pub(crate) fn eye(self) -> Vec3 {
        self.target + self.direction() * self.distance
    }

    /// Right, up and forward unit vectors of a right-handed view.
    pub(crate) fn basis(self) -> (Vec3, Vec3, Vec3) {
        let forward = -self.direction();
        let right = forward.cross(Vec3::Y).normalize_or(Vec3::X);
        let up = right.cross(forward).normalize_or(Vec3::Y);
        (right, up, forward)
    }

    /// Orbits around the pivot (Alt + left drag).
    pub(crate) fn orbit(&mut self, delta: Vec2) {
        self.goal = None;
        self.yaw = (self.yaw - delta.x * ORBIT_SPEED).rem_euclid(std::f32::consts::TAU);
        self.pitch = (self.pitch + delta.y * ORBIT_SPEED).clamp(MIN_PITCH, MAX_PITCH);
    }

    /// Turns the camera around its own eye (right drag), like a first-person look.
    pub(crate) fn look(&mut self, delta: Vec2) {
        self.goal = None;
        let eye = self.eye();
        self.yaw = (self.yaw - delta.x * LOOK_SPEED).rem_euclid(std::f32::consts::TAU);
        self.pitch = (self.pitch + delta.y * LOOK_SPEED).clamp(MIN_PITCH, MAX_PITCH);
        self.target = eye - self.direction() * self.distance;
    }

    /// Pans the pivot so the point under the cursor follows the pointer.
    pub(crate) fn pan(&mut self, delta: Vec2, focal: f32) {
        self.goal = None;
        let (right, up, _) = self.basis();
        let units = self.distance / focal.max(1.0);
        self.target += (-right * delta.x + up * delta.y) * units;
    }

    /// Dollies toward `toward` (the point under the cursor) by a wheel delta.
    /// Positive deltas move closer. Past the minimum distance the pivot
    /// itself is pushed forward so the camera can travel through the scene.
    pub(crate) fn dolly(&mut self, wheel: f32, toward: Option<Vec3>) {
        if wheel == 0.0 {
            return;
        }
        self.goal = None;
        let factor = (-wheel * DOLLY_SPEED).exp();
        let next = self.distance * factor;
        if next < MIN_DISTANCE {
            let (_, _, forward) = self.basis();
            self.target += forward * (self.distance * (1.0 - factor)).max(0.05);
            self.distance = MIN_DISTANCE;
            return;
        }
        if let Some(point) = toward.filter(|point| point.is_finite()) {
            self.target += (point - self.target) * (1.0 - factor);
        }
        self.distance = next.clamp(MIN_DISTANCE, MAX_DISTANCE);
    }

    /// Moves eye and pivot together in view space: x right, y world up, z forward.
    pub(crate) fn fly(&mut self, motion: Vec3, seconds: f32, boost: bool) {
        if motion == Vec3::ZERO {
            return;
        }
        self.goal = None;
        let (right, _, forward) = self.basis();
        let speed = self.fly_speed * if boost { 3.0 } else { 1.0 };
        let step = (right * motion.x + Vec3::Y * motion.y + forward * motion.z).normalize_or_zero()
            * speed
            * seconds;
        self.target += step;
    }

    /// Changes the fly speed with the wheel while flying.
    pub(crate) fn adjust_speed(&mut self, wheel: f32) {
        self.fly_speed =
            (self.fly_speed * (wheel * 0.004).exp()).clamp(MIN_FLY_SPEED, MAX_FLY_SPEED);
    }

    /// Smoothly frames a world-space sphere (F key).
    pub(crate) fn frame(&mut self, center: Vec3, radius: f32) {
        let radius = radius.max(0.25);
        let distance = (radius / (FOV_Y * 0.5).sin() * 1.15).clamp(MIN_DISTANCE, MAX_DISTANCE);
        self.goal = Some(Goal {
            target: center,
            distance,
            angles: None,
        });
    }

    /// Smoothly turns to look along a world axis (view-gizmo clicks).
    pub(crate) fn align(&mut self, yaw: f32, pitch: f32) {
        self.goal = Some(Goal {
            target: self.target,
            distance: self.distance,
            angles: Some((yaw, pitch.clamp(MIN_PITCH, MAX_PITCH))),
        });
    }

    /// Advances a framing animation. Returns true while it is still moving.
    pub(crate) fn animate(&mut self, seconds: f32) -> bool {
        let Some(goal) = self.goal else {
            return false;
        };
        let t = 1.0 - (-FOCUS_RATE * seconds.clamp(0.0, 0.1)).exp();
        self.target = self.target.lerp(goal.target, t);
        self.distance += (goal.distance - self.distance) * t;
        let mut settled = self.target.distance(goal.target) < 1e-3
            && (self.distance - goal.distance).abs() < 1e-3;
        if let Some((yaw, pitch)) = goal.angles {
            let turn = (yaw - self.yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
            self.yaw = (self.yaw + turn * t).rem_euclid(std::f32::consts::TAU);
            self.pitch += (pitch - self.pitch) * t;
            settled &= turn.abs() < 1e-3 && (pitch - self.pitch).abs() < 1e-3;
        }
        if settled {
            self.target = goal.target;
            self.distance = goal.distance;
            if let Some((yaw, pitch)) = goal.angles {
                self.yaw = yaw.rem_euclid(std::f32::consts::TAU);
                self.pitch = pitch;
            }
            self.goal = None;
        }
        !settled
    }
}

/// A camera bound to a screen rectangle: projection, clipping and rays.
#[derive(Clone, Copy, Debug)]
pub(crate) struct View {
    /// The camera this view projects through.
    pub camera: Camera,
    /// Screen rectangle of the view.
    pub rect: Rect,
    /// Focal length in pixels.
    pub focal: f32,
    eye: Vec3,
    right: Vec3,
    up: Vec3,
    forward: Vec3,
}

impl View {
    /// Builds the perspective view for a screen rectangle.
    pub(crate) fn new(camera: Camera, rect: Rect) -> Self {
        let (right, up, forward) = camera.basis();
        Self {
            camera,
            rect,
            focal: rect.height().max(1.0) * 0.5 / (FOV_Y * 0.5).tan(),
            eye: camera.eye(),
            right,
            up,
            forward,
        }
    }

    /// Builds a view through an authored camera node's world matrix and FOV.
    pub(crate) fn from_camera(world: Mat4, fov: f32, rect: Rect) -> Self {
        let right = Vec3::new(world.x_axis.x, world.y_axis.x, world.z_axis.x);
        let up = Vec3::new(world.x_axis.y, world.y_axis.y, world.z_axis.y);
        let forward = -Vec3::new(world.x_axis.z, world.y_axis.z, world.z_axis.z);
        let (right, up) = (right.normalize_or(Vec3::X), up.normalize_or(Vec3::Y));
        Self {
            camera: Camera::default(),
            rect,
            focal: rect.height().max(1.0) * 0.5 / (fov.to_radians() * 0.5).tan().max(1e-3),
            eye: world.inverse().w_axis.truncate(),
            right,
            up,
            forward: forward.normalize_or(Vec3::NEG_Z),
        }
    }

    /// Eye position.
    pub(crate) fn eye(&self) -> Vec3 {
        self.eye
    }

    /// Unit view direction.
    pub(crate) fn forward(&self) -> Vec3 {
        self.forward
    }

    /// Distance of a point in front of the eye along the view direction.
    pub(crate) fn depth(&self, point: Vec3) -> f32 {
        (point - self.eye).dot(self.forward)
    }

    /// Projects a point, or returns None when it is behind the near plane.
    pub(crate) fn project(&self, point: Vec3) -> Option<Pos2> {
        let depth = self.depth(point);
        (depth >= NEAR).then(|| self.project_at(point, depth))
    }

    /// Projects a point, clamping its depth to the near plane.
    pub(crate) fn project_clamped(&self, point: Vec3) -> Pos2 {
        self.project_at(point, self.depth(point).max(NEAR))
    }

    fn project_at(&self, point: Vec3, depth: f32) -> Pos2 {
        let relative = point - self.eye;
        let center = self.rect.center();
        pos2(
            center.x + relative.dot(self.right) / depth * self.focal,
            center.y - relative.dot(self.up) / depth * self.focal,
        )
    }

    /// Projects a segment after clipping it against the near plane.
    pub(crate) fn segment(&self, from: Vec3, to: Vec3) -> Option<[Pos2; 2]> {
        let (a, b) = (self.depth(from), self.depth(to));
        if a < NEAR && b < NEAR {
            return None;
        }
        let clip = |inside: Vec3, inside_depth: f32, outside: Vec3, outside_depth: f32| {
            let t = (inside_depth - NEAR) / (inside_depth - outside_depth);
            inside + (outside - inside) * t
        };
        let from = if a < NEAR { clip(to, b, from, a) } else { from };
        let to = if b < NEAR {
            clip(from, a.max(NEAR), to, b)
        } else {
            to
        };
        Some([self.project_clamped(from), self.project_clamped(to)])
    }

    /// World-space ray through a screen point: origin and unit direction.
    pub(crate) fn ray(&self, point: Pos2) -> (Vec3, Vec3) {
        let center = self.rect.center();
        let x = (point.x - center.x) / self.focal;
        let y = -(point.y - center.y) / self.focal;
        (
            self.eye,
            (self.forward + self.right * x + self.up * y).normalize_or(self.forward),
        )
    }

    /// Intersects a screen point with a plane.
    pub(crate) fn on_plane(&self, point: Pos2, origin: Vec3, normal: Vec3) -> Option<Vec3> {
        let (eye, direction) = self.ray(point);
        let facing = direction.dot(normal);
        if facing.abs() < 1e-5 {
            return None;
        }
        let distance = (origin - eye).dot(normal) / facing;
        (distance.is_finite() && distance > 0.0).then(|| eye + direction * distance)
    }

    /// Intersects a screen point with the ground plane (`y = 0`).
    pub(crate) fn ground(&self, point: Pos2) -> Option<Vec3> {
        self.on_plane(point, Vec3::ZERO, Vec3::Y)
    }

    /// World units covered by one pixel at a point's depth.
    pub(crate) fn pixel_size(&self, point: Vec3) -> f32 {
        self.depth(point).max(NEAR) / self.focal
    }

    /// Closest point on an infinite line to the ray through a screen point,
    /// as a parameter along the unit `axis` from `origin`.
    pub(crate) fn along_line(&self, point: Pos2, origin: Vec3, axis: Vec3) -> Option<f32> {
        let (eye, direction) = self.ray(point);
        let w = origin - eye;
        let b = axis.dot(direction);
        let denominator = 1.0 - b * b;
        if denominator < 1e-4 {
            return None;
        }
        let d = axis.dot(w);
        let e = direction.dot(w);
        let s = (b * e - d) / denominator;
        s.is_finite().then_some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> View {
        View::new(
            Camera::default(),
            Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(800.0, 600.0)),
        )
    }

    #[test]
    fn the_pivot_projects_to_the_view_centre() {
        let view = view();
        let projected = view.project(Vec3::ZERO).unwrap_or(Pos2::ZERO);
        assert!((projected - view.rect.center()).length() < 0.001);
    }

    #[test]
    fn the_view_is_right_handed_like_the_player() {
        // Looking down -Z from +Z, +X must appear on the right.
        let camera = Camera {
            yaw: 0.0,
            pitch: 0.0,
            ..Camera::default()
        };
        let view = View::new(camera, view().rect);
        let right = view.project(Vec3::X).unwrap_or(Pos2::ZERO);
        assert!(right.x > view.rect.center().x);
        let up = view.project(Vec3::Y).unwrap_or(Pos2::ZERO);
        assert!(up.y < view.rect.center().y);
    }

    #[test]
    fn rays_round_trip_through_the_ground_plane() {
        let view = view();
        let point = Vec3::new(2.0, 0.0, -1.0);
        let screen = view.project(point).unwrap_or(Pos2::ZERO);
        let ground = view.ground(screen).unwrap_or(Vec3::NEG_INFINITY);
        assert!(ground.distance(point) < 0.01);
    }

    #[test]
    fn segments_are_clipped_at_the_near_plane() {
        let view = view();
        let eye = view.eye();
        let behind = eye - view.forward() * 5.0;
        assert!(view.segment(behind, behind - view.forward()).is_none());
        assert!(view.segment(behind, Vec3::ZERO).is_some());
    }

    #[test]
    fn looking_keeps_the_eye_still_and_orbiting_keeps_the_pivot() {
        let mut camera = Camera::default();
        let eye = camera.eye();
        camera.look(Vec2::new(40.0, -25.0));
        assert!(camera.eye().distance(eye) < 1e-3);

        let mut camera = Camera::default();
        camera.orbit(Vec2::new(40.0, 25.0));
        assert_eq!(camera.target, Vec3::ZERO);
        camera.orbit(Vec2::new(0.0, 1e6));
        assert_eq!(camera.pitch, MAX_PITCH);
    }

    #[test]
    fn dolly_moves_toward_the_cursor_point() {
        let mut camera = Camera::default();
        camera.dolly(200.0, Some(Vec3::new(4.0, 0.0, 0.0)));
        assert!(camera.distance < DEFAULT_DISTANCE);
        assert!(camera.target.x > 0.0);
        let mut close = Camera {
            distance: MIN_DISTANCE,
            ..Camera::default()
        };
        let before = close.target;
        close.dolly(500.0, None);
        assert_ne!(close.target, before);
    }

    #[test]
    fn framing_converges_on_the_goal() {
        let mut camera = Camera::default();
        camera.frame(Vec3::new(3.0, 1.0, -2.0), 1.0);
        for _ in 0..200 {
            camera.animate(1.0 / 60.0);
        }
        assert!(camera.target.distance(Vec3::new(3.0, 1.0, -2.0)) < 1e-3);
        assert!(!camera.animate(1.0 / 60.0));
    }

    #[test]
    fn the_closest_point_on_an_axis_follows_the_pointer() {
        let view = view();
        let point = Vec3::new(1.5, 0.0, 0.0);
        let screen = view.project(point).unwrap_or(Pos2::ZERO);
        let s = view
            .along_line(screen, Vec3::ZERO, Vec3::X)
            .unwrap_or(f32::NAN);
        assert!((s - 1.5).abs() < 0.01);
    }
}
