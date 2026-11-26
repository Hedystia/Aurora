//! Unity/Roblox-style 3D scene view: navigation, shaded preview, picking,
//! transform gizmos with snapping, placement and the orientation gizmo.
//!
//! Controls:
//! - Right drag looks around; hold it and use WASD/QE to fly (Shift is faster,
//!   the wheel changes fly speed).
//! - Middle drag pans, Alt + left drag orbits, Alt + right drag zooms and the
//!   wheel dollies toward the point under the cursor.
//! - Q, W, E and R pick Select, Move, Rotate and Scale (Ctrl+1..4 as in Roblox
//!   Studio); X toggles world or local axes; F frames the selection; holding
//!   Ctrl inverts snapping while dragging; Delete removes the selection.

pub(crate) mod camera;
mod gizmo;
mod render;

use std::collections::BTreeMap;

use egui::{
    Color32, CursorIcon, Painter, PointerButton, Pos2, Rect, Response, Sense, Shape, Stroke, Ui,
    Vec2, pos2, vec2,
};
use engine_core::{
    ecs::EntityId,
    scene::{
        NodeState,
        spatial::{Role, matrices},
    },
};
use glam::{Mat4, Vec3};

pub(crate) use camera::{Camera, View};
pub(crate) use gizmo::{Space, Tool};

use crate::{project::Project, strings, theme};

/// Pixel-to-metre scale used when a 2D sprite is placed in the 3D view.
const PIXELS_PER_METRE: f32 = 32.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Navigation {
    Look,
    Pan,
    Orbit,
    Zoom,
}

/// Scene-view state that survives between frames.
#[derive(Default)]
pub(crate) struct State {
    /// The editor camera.
    pub camera: Camera,
    /// Active transform tool.
    pub tool: Tool,
    /// World or local gizmo axes.
    pub space: Space,
    /// Snapping increments.
    pub snap: gizmo::Snap,
    drag: Option<gizmo::Drag>,
    navigation: Option<Navigation>,
    hovered: Option<usize>,
    hot: Option<gizmo::Handle>,
    view: Option<View>,
}

impl State {
    /// Restores the default camera angle while keeping the fly speed.
    pub(crate) fn reset_camera(&mut self) {
        let speed = self.camera.fly_speed;
        self.camera = Camera::default();
        self.camera.fly_speed = speed;
    }

    /// The right mouse button is held for free-look and fly keys.
    pub(crate) fn flying(&self) -> bool {
        self.navigation == Some(Navigation::Look)
    }
}

/// Selection result of a frame of scene-view interaction.
pub(crate) struct Outcome {
    /// Node under a click or drag start, if any.
    pub picked: Option<usize>,
    /// The pointer grabbed a handle or the camera: keep the current selection.
    pub keep_selection: bool,
}

/// Applies camera navigation. Call before drawing the frame.
pub(crate) fn navigate(
    ui: &Ui,
    response: &Response,
    state: &mut State,
    rect: Rect,
    nodes: &[NodeState],
) {
    let (delta, motion, modifiers, seconds, wheel, any_down, buttons) = ui.input(|input| {
        let pointer = &input.pointer;
        (
            pointer.delta(),
            pointer.motion(),
            input.modifiers,
            input.stable_dt.min(0.1),
            input.smooth_scroll_delta.y,
            pointer.any_down(),
            [
                pointer.button_down(PointerButton::Primary),
                pointer.button_down(PointerButton::Secondary),
                pointer.button_down(PointerButton::Middle),
            ],
        )
    });
    if !any_down {
        state.navigation = None;
    }
    if state.navigation.is_none() && state.drag.is_none() && response.is_pointer_button_down_on() {
        state.navigation = match buttons {
            [_, true, _] if modifiers.alt => Some(Navigation::Zoom),
            [_, true, _] => Some(Navigation::Look),
            [_, _, true] => Some(Navigation::Pan),
            [true, _, _] if modifiers.alt => Some(Navigation::Orbit),
            _ => None,
        };
    }
    let view = View::new(state.camera, rect);
    let typing = ui.ctx().egui_wants_keyboard_input();
    match state.navigation {
        Some(Navigation::Look) => {
            state.camera.look(motion.unwrap_or(delta));
            let motion = ui.input(|input| fly_keys(input, true));
            state.camera.fly(motion, seconds, modifiers.shift);
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
        }
        Some(Navigation::Pan) => {
            state.camera.pan(delta, view.focal);
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        }
        Some(Navigation::Orbit) => {
            state.camera.orbit(delta);
            ui.ctx().set_cursor_icon(CursorIcon::AllScroll);
        }
        Some(Navigation::Zoom) => {
            state.camera.dolly((delta.x - delta.y) * 3.0, None);
            ui.ctx().set_cursor_icon(CursorIcon::ZoomIn);
        }
        None => {
            if response.hovered() && !typing {
                let motion = ui.input(|input| fly_keys(input, false));
                state.camera.fly(motion, seconds, modifiers.shift);
            }
        }
    }
    if response.hovered() && wheel != 0.0 {
        if state.flying() {
            state.camera.adjust_speed(wheel);
        } else {
            let toward = response.hover_pos().and_then(|pointer| {
                let worlds = matrices(nodes);
                render::pick(&view, nodes, &worlds, pointer)
                    .map(|hit| hit.point)
                    .or_else(|| {
                        view.ground(pointer).filter(|point| {
                            point.distance(view.eye()) < state.camera.distance * 40.0
                        })
                    })
            });
            state.camera.dolly(wheel, toward);
        }
    }
    if state.camera.animate(seconds) || state.navigation.is_some() {
        ui.ctx().request_repaint();
    }
}

/// WASD/QE while flying; arrow keys always move the camera, as in Unity.
fn fly_keys(input: &egui::InputState, flying: bool) -> Vec3 {
    use egui::Key;
    let axis = |positive: &[Key], negative: &[Key]| {
        let down = |keys: &[Key]| keys.iter().any(|key| input.key_down(*key));
        f32::from(u8::from(down(positive))) - f32::from(u8::from(down(negative)))
    };
    if flying {
        Vec3::new(
            axis(&[Key::D, Key::ArrowRight], &[Key::A, Key::ArrowLeft]),
            axis(&[Key::E, Key::PageUp], &[Key::Q, Key::PageDown]),
            axis(&[Key::W, Key::ArrowUp], &[Key::S, Key::ArrowDown]),
        )
    } else {
        Vec3::new(
            axis(&[Key::ArrowRight], &[Key::ArrowLeft]),
            0.0,
            axis(&[Key::ArrowUp], &[Key::ArrowDown]),
        )
    }
}

/// Tool, space, framing and delete shortcuts. Skipped while typing or flying.
pub(crate) fn shortcuts(
    ui: &Ui,
    state: &mut State,
    project: &mut Project,
    nodes: &[NodeState],
    hovered: bool,
) {
    if ui.ctx().egui_wants_keyboard_input() || state.flying() {
        return;
    }
    use egui::{Key, Modifiers};
    ui.input_mut(|input| {
        for (tools, key, roblox) in [
            (Tool::Select, Key::Q, Key::Num1),
            (Tool::Move, Key::W, Key::Num2),
            (Tool::Rotate, Key::E, Key::Num4),
            (Tool::Scale, Key::R, Key::Num3),
        ] {
            if input.consume_key(Modifiers::NONE, key)
                || input.consume_key(Modifiers::COMMAND, roblox)
            {
                state.tool = tools;
            }
        }
        if input.consume_key(Modifiers::NONE, Key::X)
            || input.consume_key(Modifiers::COMMAND, Key::L)
        {
            state.space = match state.space {
                Space::World => Space::Local,
                Space::Local => Space::World,
            };
        }
    });
    if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::F)) {
        frame_selection(state, project, nodes);
    }
    if hovered
        && let Some(index) = project.selected
        && ui.input_mut(|input| {
            input.consume_key(Modifiers::NONE, Key::Delete)
                || input.consume_key(Modifiers::NONE, Key::Backspace)
        })
        && project.remove(index).is_ok()
    {
        project.selected = None;
    }
}

/// Smoothly frames the selected node, or every spatial node when none is selected.
pub(crate) fn frame_selection(state: &mut State, project: &Project, nodes: &[NodeState]) {
    let worlds = matrices(nodes);
    let spheres: Vec<(Vec3, f32)> = nodes
        .iter()
        .filter(|node| {
            project
                .selected
                .is_none_or(|selected| node.id.value() as usize == selected)
        })
        .filter_map(|node| {
            let world = *worlds.get(&node.id)?;
            Some(match &node.spatial {
                Some(spatial) => render::world_sphere(spatial, world),
                None => {
                    let point = node.world.point([0.0; 2]);
                    (Vec3::new(point[0], -point[1], 0.0) / PIXELS_PER_METRE, 2.0)
                }
            })
        })
        .filter(|(center, radius)| center.is_finite() && radius.is_finite())
        .collect();
    let Some(first) = spheres.first() else {
        state.reset_camera();
        return;
    };
    let (low, high) = spheres.iter().fold(
        (
            first.0 - Vec3::splat(first.1),
            first.0 + Vec3::splat(first.1),
        ),
        |(low, high), (center, radius)| {
            (
                low.min(*center - Vec3::splat(*radius)),
                high.max(*center + Vec3::splat(*radius)),
            )
        },
    );
    state.camera.frame(
        (low + high) * 0.5,
        ((high - low).length() * 0.5).min(1500.0),
    );
}

fn pose_of(spatial: &engine_core::scene::spatial::SpatialNode) -> gizmo::Pose {
    gizmo::Pose {
        position: spatial.position,
        rotation: spatial.rotation,
        scale: spatial.scale,
    }
}

fn parent_matrix(nodes: &[NodeState], worlds: &BTreeMap<EntityId, Mat4>, index: usize) -> Mat4 {
    nodes
        .iter()
        .find(|node| node.id.value() as usize == index)
        .and_then(|node| node.parent)
        .and_then(|parent| worlds.get(&parent).copied())
        .unwrap_or(Mat4::IDENTITY)
}

/// Draws spatial nodes and handles picking, gizmos and dragging.
pub(crate) fn draw(
    ui: &Ui,
    painter: &Painter,
    view: View,
    response: &Response,
    nodes: &[NodeState],
    state: &mut State,
    project: &mut Project,
) -> Outcome {
    state.view = Some(view);
    let worlds = matrices(nodes);
    render::solid(painter, &view, nodes, &worlds, state.hovered);
    render::overlays(
        painter,
        &view,
        nodes,
        &worlds,
        project.selected,
        state.hovered,
    );

    let selected = project
        .selected
        .and_then(|index| nodes.iter().find(|node| node.id.value() as usize == index))
        .filter(|node| node.visible && node.spatial.is_some());
    let frame = selected
        .and_then(|node| worlds.get(&node.id))
        .map(|world| gizmo::Frame::new(&view, *world, state.space, state.tool));
    let pointer = response
        .hover_pos()
        .or_else(|| response.interact_pointer_pos());
    let idle = state.drag.is_none() && state.navigation.is_none();
    if idle {
        state.hot = frame
            .zip(pointer)
            .and_then(|(frame, pointer)| gizmo::hit(&view, &frame, state.tool, pointer));
        state.hovered = if state.hot.is_none() {
            response
                .hover_pos()
                .and_then(|pointer| render::pick(&view, nodes, &worlds, pointer))
                .map(|hit| hit.index)
        } else {
            None
        };
    } else if state.navigation.is_some() {
        state.hot = None;
        state.hovered = None;
    }
    if let Some(frame) = &frame {
        gizmo::draw(
            painter,
            &view,
            frame,
            state.tool,
            state.hot,
            state.drag.as_ref().map(|drag| drag.handle),
        );
    }
    if state.hot.is_some() && state.drag.is_none() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    let mut outcome = Outcome {
        picked: None,
        keep_selection: state.navigation.is_some() || ui.input(|input| input.modifiers.alt),
    };
    let snap = ui.input(|input| input.modifiers.command) != state.snap.enabled;
    let snap = snap.then_some(state.snap);
    if !outcome.keep_selection && response.drag_started_by(PointerButton::Primary) {
        let press = ui
            .input(|input| input.pointer.press_origin())
            .or_else(|| response.interact_pointer_pos());
        if let (Some(handle), Some(frame), Some(node), Some(press)) =
            (state.hot, frame, selected, press)
        {
            let index = node.id.value() as usize;
            state.drag = node.spatial.as_ref().and_then(|spatial| {
                gizmo::Drag::begin(
                    &view,
                    frame,
                    handle,
                    press,
                    index,
                    parent_matrix(nodes, &worlds, index),
                    pose_of(spatial),
                )
            });
            outcome.picked = Some(index);
            outcome.keep_selection = true;
        } else if let Some(press) = press {
            let hit = render::pick(&view, nodes, &worlds, press);
            outcome.picked = hit.map(|hit| hit.index);
            if let Some(hit) = hit
                && let Some(node) = nodes
                    .iter()
                    .find(|node| node.id.value() as usize == hit.index)
                && let (Some(spatial), Some(world)) = (&node.spatial, worlds.get(&node.id))
            {
                let frame = gizmo::Frame::new(&view, *world, Space::World, Tool::Move);
                state.drag = gizmo::Drag::begin(
                    &view,
                    frame,
                    gizmo::Handle::Body,
                    press,
                    hit.index,
                    parent_matrix(nodes, &worlds, hit.index),
                    pose_of(spatial),
                );
            }
        }
    } else if !outcome.keep_selection && response.clicked() {
        outcome.picked = if state.hot.is_some() {
            outcome.keep_selection = true;
            project.selected
        } else {
            response
                .interact_pointer_pos()
                .and_then(|pointer| render::pick(&view, nodes, &worlds, pointer))
                .map(|hit| hit.index)
        };
    }

    if response.dragged_by(PointerButton::Primary)
        && let Some(drag) = &mut state.drag
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let tool = if drag.handle == gizmo::Handle::Body {
            Tool::Move
        } else {
            state.tool
        };
        if let Some(pose) = drag.update(&view, tool, pointer, snap)
            && let Some(spatial) = project
                .document
                .nodes
                .get_mut(drag.index)
                .and_then(|node| node.spatial.as_mut())
            && (spatial.position != pose.position
                || spatial.rotation != pose.rotation
                || spatial.scale != pose.scale)
        {
            spatial.position = pose.position;
            spatial.rotation = pose.rotation;
            spatial.scale = pose.scale;
            project.selected = Some(drag.index);
            project.changed();
        }
        if !drag.readout.is_empty() {
            label(
                painter,
                gizmo::readout_position(pointer),
                &drag.readout,
                theme::GIZMO_HOT,
            );
        }
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    }
    if response.drag_stopped_by(PointerButton::Primary)
        || !ui.input(|input| input.pointer.primary_down())
    {
        state.drag = None;
    }
    outcome
}

/// Places a freshly added spatial node where the user expects it: on the
/// surface under the cursor, or on the ground in front of the camera, resting
/// on its base like Roblox Studio parts.
pub(crate) fn place(
    state: &State,
    nodes: &[NodeState],
    project: &mut Project,
    index: usize,
    pointer: Option<Pos2>,
) {
    let Some(view) = state.view else {
        return;
    };
    let Some(spatial) = project
        .document
        .nodes
        .get(index)
        .and_then(|node| node.spatial.as_ref())
    else {
        return;
    };
    if matches!(spatial.role, Role::Camera | Role::DirectionalLight) {
        return;
    }
    let worlds = matrices(nodes);
    let surface = pointer.and_then(|pointer| {
        render::pick(&view, nodes, &worlds, pointer)
            .map(|hit| hit.point)
            .or_else(|| view.ground(pointer))
    });
    let mut point = surface
        .or_else(|| view.ground(view.rect.center()))
        .filter(|point| point.distance(view.eye()) < state.camera.distance * 6.0)
        .unwrap_or(state.camera.target * Vec3::new(1.0, 0.0, 1.0));
    if state.snap.enabled && state.snap.step > 0.0 {
        point.x = (point.x / state.snap.step).round() * state.snap.step;
        point.z = (point.z / state.snap.step).round() * state.snap.step;
    }
    if matches!(spatial.role, Role::Cube | Role::Mesh) {
        let (low, _) = render::local_bounds(spatial);
        point.y += -low.y * spatial.scale[1];
    }
    let parent = project.document.nodes[index]
        .parent
        .and_then(|parent| nodes.iter().find(|node| node.id.value() as usize == parent))
        .and_then(|node| worlds.get(&node.id).copied())
        .unwrap_or(Mat4::IDENTITY);
    let local = parent.inverse().transform_point3(point);
    if local.is_finite()
        && let Some(spatial) = project.document.nodes[index].spatial.as_mut()
    {
        spatial.position = local.to_array();
        project.changed();
    }
}

/// Projects one sprite quad point into the perspective view.
///
/// Spatial nodes use their authored world matrix. Plain 2D nodes are placed on
/// the `z = 0` plane at the engine's 32-pixel-per-metre scale so a mixed scene
/// shows its HUD beside the 3D geometry.
pub(crate) fn project_sprite_point(
    view: &View,
    worlds: &BTreeMap<EntityId, Mat4>,
    node: &NodeState,
    point: [f32; 2],
) -> Pos2 {
    view.project_clamped(sprite_point(worlds, node, point))
}

fn sprite_point(worlds: &BTreeMap<EntityId, Mat4>, node: &NodeState, point: [f32; 2]) -> Vec3 {
    let local = if node.spatial.is_some() {
        point
    } else {
        node.world.point(point)
    };
    let local = Vec3::new(
        local[0] / PIXELS_PER_METRE,
        -local[1] / PIXELS_PER_METRE,
        0.0,
    );
    let matrix = worlds.get(&node.id).copied().unwrap_or(Mat4::IDENTITY);
    matrix.transform_point3(local)
}

/// View depth of a sprite's origin, for far-to-near painting.
pub(crate) fn sprite_depth(
    view: &View,
    worlds: &BTreeMap<EntityId, Mat4>,
    node: &NodeState,
) -> f32 {
    view.depth(sprite_point(worlds, node, [0.0; 2]))
}

/// Paints the sky gradient and an adaptive ground grid that fades with distance.
pub(crate) fn backdrop(painter: &Painter, view: &View) {
    render::sky(painter, view.rect);
    let camera = view.camera;
    let reach = view.eye().y.abs().max(camera.distance * 0.4).max(0.5);
    let step = 10.0_f32.powf((reach / 4.0).log10().floor()).max(0.01);
    let major = step * 10.0;
    let extent = (reach * 14.0).clamp(12.0, 5000.0);
    let lines = ((extent / step).ceil() as i32).min(160);
    let center = Vec3::new(
        (camera.target.x / major).round() * major,
        0.0,
        (camera.target.z / major).round() * major,
    );
    for direction in [0, 1] {
        for index in -lines..=lines {
            let offset = index as f32 * step;
            let value = if direction == 0 {
                center.x + offset
            } else {
                center.z + offset
            };
            let fade = (1.0 - index.unsigned_abs() as f32 / lines as f32).powf(1.6);
            if fade <= 0.02 {
                continue;
            }
            let on_axis = value.abs() < step * 0.5;
            let on_major = ((value / major).round() * major - value).abs() < step * 0.5;
            let (color, width) = if on_axis {
                (
                    if direction == 0 {
                        theme::AXIS_Z
                    } else {
                        theme::AXIS_X
                    },
                    1.6,
                )
            } else if on_major {
                (theme::GRID_MAJOR, 1.0)
            } else {
                (theme::GRID, 1.0)
            };
            let (from, to) = if direction == 0 {
                (
                    Vec3::new(value, 0.0, center.z - extent),
                    Vec3::new(value, 0.0, center.z + extent),
                )
            } else {
                (
                    Vec3::new(center.x - extent, 0.0, value),
                    Vec3::new(center.x + extent, 0.0, value),
                )
            };
            if let Some(segment) = view.segment(from, to) {
                painter.line_segment(segment, Stroke::new(width, color.gamma_multiply(fade)));
            }
        }
    }
}

/// Clickable orientation gizmo in the top-right corner; clicking an axis
/// turns the camera to look along it, like Unity's scene gizmo.
pub(crate) fn view_gizmo(ui: &mut Ui, rect: Rect, state: &mut State) {
    let center = pos2(rect.right() - 56.0, rect.top() + 56.0);
    let area = Rect::from_center_size(center, Vec2::splat(100.0));
    let response = ui.interact(area, ui.id().with("spatial-view-gizmo"), Sense::click());
    let painter = ui.painter_at(rect);
    painter.circle_filled(
        center,
        46.0,
        Color32::from_black_alpha(if response.hovered() { 80 } else { 45 }),
    );
    let (right, up, forward) = state.camera.basis();
    let mut handles: Vec<(f32, Pos2, usize, bool)> = Vec::new();
    for (axis, direction) in [Vec3::X, Vec3::Y, Vec3::Z].into_iter().enumerate() {
        for positive in [true, false] {
            let direction = if positive { direction } else { -direction };
            let screen = center + vec2(direction.dot(right), -direction.dot(up)) * 32.0;
            handles.push((direction.dot(forward), screen, axis, positive));
        }
    }
    handles.sort_by(|left, right| right.0.total_cmp(&left.0));
    let hovered = response
        .hover_pos()
        .and_then(|pointer| nearest_handle(&handles, pointer));
    let colors = [theme::AXIS_X, theme::AXIS_Y, theme::AXIS_Z];
    for (index, (_, screen, axis, positive)) in handles.iter().enumerate() {
        let lit = hovered == Some(index);
        let color = if lit { theme::GIZMO_HOT } else { colors[*axis] };
        if *positive {
            painter.line_segment([center, *screen], Stroke::new(2.0, color));
            painter.circle_filled(*screen, 9.0, color);
            painter.text(
                *screen,
                egui::Align2::CENTER_CENTER,
                ["X", "Y", "Z"][*axis],
                egui::FontId::proportional(11.0),
                Color32::from_gray(20),
            );
        } else {
            painter.circle_filled(*screen, 6.5, color.gamma_multiply(0.35));
            painter.circle_stroke(*screen, 6.5, Stroke::new(1.0, color));
        }
    }
    if hovered.is_some() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if response.clicked()
        && let Some(index) = response
            .interact_pointer_pos()
            .and_then(|pointer| nearest_handle(&handles, pointer))
    {
        let (_, _, axis, positive) = handles[index];
        let pi = std::f32::consts::PI;
        let (yaw, pitch) = match (axis, positive) {
            (0, true) => (pi * 0.5, 0.0),
            (0, false) => (-pi * 0.5, 0.0),
            (1, true) => (state.camera.yaw, pi * 0.5),
            (1, false) => (state.camera.yaw, -pi * 0.5),
            (_, true) => (0.0, 0.0),
            (_, false) => (pi, 0.0),
        };
        state.camera.align(yaw, pitch);
    }
}

fn nearest_handle(handles: &[(f32, Pos2, usize, bool)], pointer: Pos2) -> Option<usize> {
    handles
        .iter()
        .enumerate()
        .map(|(index, (_, screen, _, _))| (index, screen.distance(pointer)))
        .filter(|(_, distance)| *distance <= 11.0)
        .min_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(index, _)| index)
}

/// Bottom status pills: cursor ground position, fly speed and control hints.
pub(crate) fn hud(painter: &Painter, view: &View, state: &State, pointer: Option<Pos2>) {
    let rect = view.rect;
    let mut status = Vec::new();
    if let Some(ground) = pointer.and_then(|pointer| view.ground(pointer)) {
        status.push(format!("X {:.2}   Z {:.2}", ground.x, ground.z));
    }
    if state.flying() {
        status.push(format!(
            "{} {:.1} m/s",
            strings::FLY_SPEED,
            state.camera.fly_speed
        ));
    }
    if state.snap.enabled {
        status.push(strings::SNAP_ON.to_owned());
    }
    if !status.is_empty() {
        label(
            painter,
            pos2(rect.left() + 10.0, rect.bottom() - 12.0),
            &status.join("   ·   "),
            theme::FOREGROUND,
        );
    }
    let galley = painter.layout_no_wrap(
        strings::SPATIAL_HINT.to_owned(),
        egui::FontId::proportional(11.0),
        theme::TEXT,
    );
    if galley.size().x + 260.0 < rect.width() {
        painter.galley(
            pos2(
                rect.right() - galley.size().x - 10.0,
                rect.bottom() - galley.size().y - 7.0,
            ),
            galley,
            theme::TEXT,
        );
    }
}

/// Paints a text pill; `anchor` is the left-centre point.
fn label(painter: &Painter, anchor: Pos2, text: &str, color: Color32) {
    let galley = painter.layout_no_wrap(text.to_owned(), egui::FontId::monospace(11.5), color);
    let rect = Rect::from_min_size(
        pos2(anchor.x, anchor.y - galley.size().y * 0.5),
        galley.size(),
    )
    .expand2(vec2(7.0, 3.5));
    painter.rect_filled(rect, 5.0, Color32::from_black_alpha(150));
    painter.galley(
        pos2(anchor.x, anchor.y - galley.size().y * 0.5),
        galley,
        color,
    );
}

/// Tool, space and snap controls shown above the 3D view.
pub(crate) fn toolbar(ui: &mut Ui, state: &mut State) {
    for (tool, tip) in [
        (Tool::Select, strings::TOOL_SELECT),
        (Tool::Move, strings::TOOL_MOVE),
        (Tool::Rotate, strings::TOOL_ROTATE),
        (Tool::Scale, strings::TOOL_SCALE),
    ] {
        let response = ui
            .add(egui::Button::selectable(state.tool == tool, "").min_size(vec2(30.0, 24.0)))
            .on_hover_text(tip);
        let color = if state.tool == tool {
            Color32::WHITE
        } else {
            theme::FOREGROUND
        };
        tool_icon(ui.painter(), response.rect, tool, color);
        if response.clicked() {
            state.tool = tool;
        }
    }
    ui.separator();
    let (label, tip) = match state.space {
        Space::World => (strings::SPACE_WORLD, strings::SPACE_TIP),
        Space::Local => (strings::SPACE_LOCAL, strings::SPACE_TIP),
    };
    if ui.button(label).on_hover_text(tip).clicked() {
        state.space = match state.space {
            Space::World => Space::Local,
            Space::Local => Space::World,
        };
    }
    ui.separator();
    ui.toggle_value(&mut state.snap.enabled, strings::SNAP)
        .on_hover_text(strings::SNAP_TIP);
    ui.add(
        egui::DragValue::new(&mut state.snap.step)
            .range(0.01..=100.0)
            .speed(0.05)
            .suffix(" m"),
    )
    .on_hover_text(strings::SNAP_MOVE);
    ui.add(
        egui::DragValue::new(&mut state.snap.angle)
            .range(1.0..=180.0)
            .speed(0.5)
            .suffix("°"),
    )
    .on_hover_text(strings::SNAP_ROTATE);
    ui.add(
        egui::DragValue::new(&mut state.snap.scale)
            .range(0.01..=10.0)
            .speed(0.01)
            .prefix("×"),
    )
    .on_hover_text(strings::SNAP_SCALE);
}

fn tool_icon(painter: &Painter, rect: Rect, tool: Tool, color: Color32) {
    let c = rect.center();
    let stroke = Stroke::new(1.6, color);
    match tool {
        Tool::Select => {
            let points = [
                (-3.5, -7.0),
                (-3.5, 5.5),
                (-0.5, 2.8),
                (1.8, 7.5),
                (3.6, 6.6),
                (1.4, 2.0),
                (5.2, 1.8),
            ]
            .map(|(x, y)| c + vec2(x, y))
            .to_vec();
            painter.add(Shape::closed_line(points, stroke));
        }
        Tool::Move => {
            for direction in [Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y] {
                let end = c + direction * 7.5;
                painter.line_segment([c, end], stroke);
                let side = direction.rot90() * 2.6;
                painter.add(Shape::convex_polygon(
                    vec![
                        end + direction * 1.5,
                        end - direction * 2.5 + side,
                        end - direction * 2.5 - side,
                    ],
                    color,
                    Stroke::NONE,
                ));
            }
        }
        Tool::Rotate => {
            let points: Vec<Pos2> = (0..=20)
                .map(|step| {
                    let angle = 0.7 + step as f32 / 20.0 * 4.6;
                    c + Vec2::angled(angle) * 6.5
                })
                .collect();
            let end = points.last().copied().unwrap_or(c);
            painter.add(Shape::line(points, stroke));
            let tangent = Vec2::angled(5.3 + std::f32::consts::FRAC_PI_2);
            painter.add(Shape::convex_polygon(
                vec![
                    end + tangent * 3.5,
                    end + tangent.rot90() * 3.0,
                    end - tangent.rot90() * 3.0,
                ],
                color,
                Stroke::NONE,
            ));
        }
        Tool::Scale => {
            painter.rect_stroke(
                Rect::from_min_size(c + vec2(-6.5, -1.0), vec2(7.5, 7.5)),
                1.0,
                stroke,
                egui::StrokeKind::Middle,
            );
            painter.line_segment([c + vec2(-1.0, 1.0), c + vec2(6.5, -6.5)], stroke);
            painter.line_segment([c + vec2(6.5, -6.5), c + vec2(2.5, -6.5)], stroke);
            painter.line_segment([c + vec2(6.5, -6.5), c + vec2(6.5, -2.5)], stroke);
        }
    }
}
