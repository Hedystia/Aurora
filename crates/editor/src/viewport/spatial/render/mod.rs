//! Shaded scene-view drawing and ray picking for spatial nodes.
//!
//! egui has no depth buffer, so triangles are flat shaded on the CPU and
//! painted far-to-near. This is plenty for authoring scenes and keeps the
//! editor independent of the player's renderer.

use std::collections::BTreeMap;

use egui::{Color32, Mesh, Painter, Pos2, Rect, Stroke, Vec2, pos2};
use engine_core::{
    ecs::EntityId,
    scene::{
        NodeState,
        spatial::{Body, Collider, Role, SpatialNode},
    },
};
use glam::{Mat4, Vec3};

use super::camera::View;
use crate::theme;

/// Upper bound on shaded triangles per frame; further meshes fall back to boxes.
const TRIANGLE_BUDGET: usize = 90_000;
/// Screen radius for picking point-like nodes (pivots, cameras, lights).
const MARKER_PICK: f32 = 11.0;

const BOX_EDGES: [[usize; 2]; 12] = [
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
];

/// A pickable spatial node as seen this frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Hit {
    /// Document index of the node.
    pub index: usize,
    /// World-space point under the cursor.
    pub point: Vec3,
    /// Ray distance, comparable between nodes.
    pub distance: f32,
}

/// Local-space vertices and triangles of a shaded node.
///
/// Cube faces are split into roughly one-metre tiles so that painter's-order
/// sorting stays correct when a large floor meets small props.
fn shape(spatial: &SpatialNode) -> Option<(Vec<Vec3>, std::borrow::Cow<'_, [u32]>)> {
    match spatial.role {
        Role::Cube => {
            let (vertices, indices) = tiled_cube(spatial.scale);
            Some((vertices, indices.into()))
        }
        Role::Mesh => spatial.geometry.as_ref().map(|geometry| {
            (
                geometry.positions.iter().copied().map(Vec3::from).collect(),
                geometry.indices.as_slice().into(),
            )
        }),
        _ => None,
    }
}

/// A unit cube with counter-clockwise outward faces tiled by world size.
fn tiled_cube(scale: [f32; 3]) -> (Vec<Vec3>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    // (normal, u, v) with u x v = normal.
    for (normal, u, v) in [(0, 1, 2), (1, 2, 0), (2, 0, 1)]
        .into_iter()
        .flat_map(|(n, u, v)| [(n, u, v, 1.0), (n, v, u, -1.0)])
        .map(|(n, u, v, sign)| {
            let axis = |index: usize| Vec3::AXES[index];
            ((axis(n) * sign, n), (axis(u), u), (axis(v), v))
        })
    {
        let tiles = |axis: usize| (scale[axis].abs().ceil() as u32).clamp(1, 16);
        let (columns, rows) = (tiles(u.1), tiles(v.1));
        let start = vertices.len() as u32;
        for row in 0..=rows {
            for column in 0..=columns {
                let s = column as f32 / columns as f32 - 0.5;
                let t = row as f32 / rows as f32 - 0.5;
                vertices.push(normal.0 * 0.5 + u.0 * s + v.0 * t);
            }
        }
        let at = |column: u32, row: u32| start + row * (columns + 1) + column;
        for row in 0..rows {
            for column in 0..columns {
                let (a, b, c, d) = (
                    at(column, row),
                    at(column + 1, row),
                    at(column, row + 1),
                    at(column + 1, row + 1),
                );
                indices.extend([a, b, d, a, d, c]);
            }
        }
    }
    (vertices, indices)
}

/// Local bounding box corners of a node's geometry (unit cube by default).
pub(crate) fn local_bounds(spatial: &SpatialNode) -> (Vec3, Vec3) {
    spatial
        .geometry
        .as_ref()
        .filter(|_| spatial.role == Role::Mesh)
        .and_then(|geometry| {
            let mut points = geometry.positions.iter().copied().map(Vec3::from);
            let first = points.next()?;
            Some(points.fold((first, first), |(low, high), point| {
                (low.min(point), high.max(point))
            }))
        })
        .unwrap_or((Vec3::splat(-0.5), Vec3::splat(0.5)))
}

fn box_corners((low, high): (Vec3, Vec3)) -> [Vec3; 8] {
    std::array::from_fn(|corner| {
        Vec3::new(
            if corner & 1 == 0 { low.x } else { high.x },
            if corner & 2 == 0 { low.y } else { high.y },
            if corner & 4 == 0 { low.z } else { high.z },
        )
    })
}

/// World-space bounding sphere of a node, used for framing.
pub(crate) fn world_sphere(spatial: &SpatialNode, world: Mat4) -> (Vec3, f32) {
    if !matches!(spatial.role, Role::Cube | Role::Mesh) {
        return (world.transform_point3(Vec3::ZERO), 1.0);
    }
    let corners = box_corners(local_bounds(spatial)).map(|point| world.transform_point3(point));
    let (low, high) = corners
        .iter()
        .fold((corners[0], corners[0]), |(low, high), point| {
            (low.min(*point), high.max(*point))
        });
    ((low + high) * 0.5, (high - low).length() * 0.5)
}

fn srgb(linear: f32) -> u8 {
    (linear.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0).round() as u8
}

fn shade(color: [f32; 3], normal: Vec3, view: &View, highlight: bool) -> Color32 {
    const KEY: Vec3 = Vec3::new(0.38, 0.84, 0.39);
    let key = normal.dot(KEY.normalize()).max(0.0);
    let fill = normal.dot(-view.forward()).max(0.0);
    let sky = normal.y * 0.5 + 0.5;
    let mut light = 0.16 + 0.62 * key + 0.16 * fill + 0.1 * sky;
    if highlight {
        light *= 1.18;
    }
    Color32::from_rgb(
        srgb(color[0] * light),
        srgb(color[1] * light),
        srgb(color[2] * light),
    )
}

/// Paints every visible cube and mesh as flat-shaded, depth-sorted triangles.
pub(crate) fn solid(
    painter: &Painter,
    view: &View,
    nodes: &[NodeState],
    worlds: &BTreeMap<EntityId, Mat4>,
    hovered: Option<usize>,
) {
    let mut triangles: Vec<(f32, [Pos2; 3], Color32)> = Vec::new();
    let mut fallback = Vec::new();
    for node in nodes.iter().filter(|node| node.visible) {
        let (Some(spatial), Some(world)) = (&node.spatial, worlds.get(&node.id)) else {
            continue;
        };
        let Some((vertices, indices)) = shape(spatial) else {
            continue;
        };
        if triangles.len() + indices.len() / 3 > TRIANGLE_BUDGET {
            fallback.push((spatial, *world));
            continue;
        }
        let index = node.id.value() as usize;
        let mirrored = world.determinant() < 0.0;
        let points: Vec<Vec3> = vertices
            .iter()
            .map(|point| world.transform_point3(*point))
            .collect();
        let depths: Vec<f32> = points.iter().map(|point| view.depth(*point)).collect();
        for triangle in indices.as_chunks::<3>().0 {
            let [a, b, c] = [0, 1, 2].map(|corner| triangle[corner] as usize);
            let (Some(pa), Some(pb), Some(pc)) = (points.get(a), points.get(b), points.get(c))
            else {
                continue;
            };
            if depths[a] < super::camera::NEAR
                || depths[b] < super::camera::NEAR
                || depths[c] < super::camera::NEAR
            {
                continue;
            }
            let mut normal = (*pb - *pa).cross(*pc - *pa);
            if mirrored {
                normal = -normal;
            }
            if normal.dot(*pa - view.eye()) >= 0.0 {
                continue;
            }
            let normal = normal.normalize_or_zero();
            let projected = [*pa, *pb, *pc].map(|point| view.project_clamped(point));
            triangles.push((
                (depths[a] + depths[b] + depths[c]) / 3.0,
                projected,
                shade(spatial.color, normal, view, hovered == Some(index)),
            ));
        }
    }
    triangles.sort_by(|left, right| right.0.total_cmp(&left.0));
    let mut mesh = Mesh::default();
    for (_, points, color) in triangles {
        let start = mesh.vertices.len() as u32;
        for point in points {
            mesh.colored_vertex(point, color);
        }
        mesh.add_triangle(start, start + 1, start + 2);
    }
    if !mesh.is_empty() {
        painter.add(egui::Shape::mesh(mesh));
    }
    for (spatial, world) in fallback {
        wire_box(
            painter,
            view,
            world,
            local_bounds(spatial),
            Stroke::new(1.0, theme::TEXT),
        );
    }
}

/// Draws a node's box edges in world space.
pub(crate) fn wire_box(
    painter: &Painter,
    view: &View,
    world: Mat4,
    bounds: (Vec3, Vec3),
    stroke: Stroke,
) {
    let corners = box_corners(bounds).map(|point| world.transform_point3(point));
    for [from, to] in BOX_EDGES {
        if let Some(segment) = view.segment(corners[from], corners[to]) {
            painter.line_segment(segment, stroke);
        }
    }
}

/// Draws point-like nodes (pivots, cameras and lights), collider outlines,
/// and selection or hover outlines on top of the shaded geometry.
pub(crate) fn overlays(
    painter: &Painter,
    view: &View,
    nodes: &[NodeState],
    worlds: &BTreeMap<EntityId, Mat4>,
    selected: Option<usize>,
    hovered: Option<usize>,
) {
    for node in nodes.iter().filter(|node| node.visible) {
        let (Some(spatial), Some(world)) = (&node.spatial, worlds.get(&node.id)) else {
            continue;
        };
        let index = node.id.value() as usize;
        let is_selected = selected == Some(index);
        let position = world.transform_point3(Vec3::ZERO);
        match spatial.role {
            Role::Cube | Role::Mesh => {
                if is_selected {
                    wire_box(
                        painter,
                        view,
                        *world,
                        local_bounds(spatial),
                        Stroke::new(2.0, theme::SELECTION),
                    );
                } else if hovered == Some(index) {
                    wire_box(
                        painter,
                        view,
                        *world,
                        local_bounds(spatial),
                        Stroke::new(1.2, theme::HOVER_OUTLINE),
                    );
                }
            }
            Role::Camera => camera_icon(painter, view, *world, spatial, is_selected),
            Role::DirectionalLight => light_icon(painter, view, *world, is_selected),
            Role::Pivot => {
                if let Some(center) = view.project(position) {
                    let radius = if is_selected { 5.0 } else { 4.0 };
                    painter.circle_filled(center, radius, theme::PANEL);
                    painter.circle_stroke(
                        center,
                        radius,
                        Stroke::new(
                            1.5,
                            if is_selected {
                                theme::SELECTION
                            } else {
                                theme::TEXT
                            },
                        ),
                    );
                }
            }
        }
        if spatial.body != Body::None && spatial.collider != Collider::Cuboid {
            collider_outline(painter, view, *world, spatial);
        }
    }
}

fn camera_icon(painter: &Painter, view: &View, world: Mat4, spatial: &SpatialNode, selected: bool) {
    let tint = if selected {
        theme::SELECTION
    } else {
        theme::ANCHOR
    };
    let (_, rotation, translation) = world.to_scale_rotation_translation();
    let length = 0.7;
    let half = (spatial.fov.to_radians() * 0.5).tan() * length;
    let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
        .map(|(x, y)| translation + rotation * Vec3::new(x * half * 1.4, y * half, -length));
    let stroke = Stroke::new(if selected { 1.8 } else { 1.2 }, tint);
    for corner in corners {
        if let Some(segment) = view.segment(translation, corner) {
            painter.line_segment(segment, stroke);
        }
    }
    for side in 0..4 {
        if let Some(segment) = view.segment(corners[side], corners[(side + 1) % 4]) {
            painter.line_segment(segment, stroke);
        }
    }
    if let Some(center) = view.project(translation) {
        painter.circle_filled(center, 3.5, tint);
    }
}

fn light_icon(painter: &Painter, view: &View, world: Mat4, selected: bool) {
    let tint = if selected {
        theme::SELECTION
    } else {
        theme::WARNING
    };
    let (_, rotation, translation) = world.to_scale_rotation_translation();
    let Some(center) = view.project(translation) else {
        return;
    };
    painter.circle_stroke(center, 6.0, Stroke::new(1.6, tint));
    for ray in 0..8 {
        let angle = ray as f32 * std::f32::consts::FRAC_PI_4;
        let direction = Vec2::angled(angle);
        painter.line_segment(
            [center + direction * 8.5, center + direction * 12.0],
            Stroke::new(1.4, tint),
        );
    }
    let direction = rotation * Vec3::NEG_Z;
    if let Some(segment) = view.segment(translation, translation + direction * 1.5) {
        painter.line_segment(segment, Stroke::new(1.2, tint.gamma_multiply(0.8)));
    }
}

fn collider_outline(painter: &Painter, view: &View, world: Mat4, spatial: &SpatialNode) {
    let tint = if spatial.trigger {
        theme::WARNING
    } else {
        theme::ACCEPT
    };
    let stroke = Stroke::new(1.2, tint);
    let (scale, rotation, translation) = world.to_scale_rotation_translation();
    let radius = scale.x.abs() * 0.5;
    let circle = |center: Vec3, a: Vec3, b: Vec3, radius: f32| {
        let points: Vec<Pos2> = (0..=48)
            .filter_map(|step| {
                let angle = step as f32 / 48.0 * std::f32::consts::TAU;
                view.project(center + (a * angle.cos() + b * angle.sin()) * radius)
            })
            .collect();
        if points.len() > 1 {
            painter.add(egui::Shape::line(points, stroke));
        }
    };
    let [x, y, z] = [Vec3::X, Vec3::Y, Vec3::Z].map(|axis| rotation * axis);
    match spatial.collider {
        Collider::Sphere => {
            circle(translation, x, y, radius);
            circle(translation, x, z, radius);
            circle(translation, y, z, radius);
        }
        Collider::Capsule => {
            let half = (scale.y.abs() * 0.5 - radius).max(0.0);
            for offset in [-half, half] {
                circle(translation + y * offset, x, z, radius);
            }
            for side in [x, z, -x, -z] {
                if let Some(segment) = view.segment(
                    translation + side * radius - y * half,
                    translation + side * radius + y * half,
                ) {
                    painter.line_segment(segment, stroke);
                }
            }
        }
        Collider::Mesh | Collider::ConvexHull => {
            wire_box(painter, view, world, local_bounds(spatial), stroke);
        }
        Collider::Cuboid => {}
    }
}

/// Finds the nearest spatial node under a screen point.
pub(crate) fn pick(
    view: &View,
    nodes: &[NodeState],
    worlds: &BTreeMap<EntityId, Mat4>,
    pointer: Pos2,
) -> Option<Hit> {
    let (origin, direction) = view.ray(pointer);
    let mut best: Option<Hit> = None;
    let mut marker: Option<(f32, Hit)> = None;
    for node in nodes.iter().filter(|node| node.visible) {
        let (Some(spatial), Some(world)) = (&node.spatial, worlds.get(&node.id)) else {
            continue;
        };
        let index = node.id.value() as usize;
        if let Some((vertices, indices)) = shape(spatial) {
            let inverse = world.inverse();
            let local_origin = inverse.transform_point3(origin);
            let local_direction = inverse.transform_vector3(direction);
            if !local_origin.is_finite() || !local_direction.is_finite() {
                continue;
            }
            let Some(entry) = slab(local_origin, local_direction, local_bounds(spatial)) else {
                continue;
            };
            if best.is_some_and(|best| best.distance < entry) {
                continue;
            }
            let nearest = indices
                .as_chunks::<3>()
                .0
                .iter()
                .filter_map(|triangle| {
                    let [a, b, c] = [0, 1, 2].map(|corner| vertices.get(triangle[corner] as usize));
                    intersect(local_origin, local_direction, *a?, *b?, *c?)
                })
                .min_by(f32::total_cmp);
            if let Some(distance) = nearest
                && best.is_none_or(|best| distance < best.distance)
            {
                best = Some(Hit {
                    index,
                    point: origin + direction * distance,
                    distance,
                });
            }
        } else {
            let position = world.transform_point3(Vec3::ZERO);
            if let Some(center) = view.project(position) {
                let gap = center.distance(pointer);
                if gap <= MARKER_PICK && marker.is_none_or(|(best, _)| gap < best) {
                    marker = Some((
                        gap,
                        Hit {
                            index,
                            point: position,
                            distance: view.depth(position),
                        },
                    ));
                }
            }
        }
    }
    // Small icons win over the geometry behind them, like Unity's gizmo icons.
    marker.map(|(_, hit)| hit).or(best)
}

/// Ray versus axis-aligned box; returns the entry parameter.
fn slab(origin: Vec3, direction: Vec3, (low, high): (Vec3, Vec3)) -> Option<f32> {
    let mut near = f32::NEG_INFINITY;
    let mut far = f32::INFINITY;
    for axis in 0..3 {
        if direction[axis].abs() < 1e-9 {
            if origin[axis] < low[axis] || origin[axis] > high[axis] {
                return None;
            }
            continue;
        }
        let a = (low[axis] - origin[axis]) / direction[axis];
        let b = (high[axis] - origin[axis]) / direction[axis];
        near = near.max(a.min(b));
        far = far.min(a.max(b));
    }
    (far >= near.max(0.0)).then_some(near.max(0.0))
}

/// Möller–Trumbore ray/triangle intersection, two-sided.
fn intersect(origin: Vec3, direction: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let edge1 = b - a;
    let edge2 = c - a;
    let p = direction.cross(edge2);
    let determinant = edge1.dot(p);
    if determinant.abs() < 1e-9 {
        return None;
    }
    let inverse = 1.0 / determinant;
    let s = origin - a;
    let u = s.dot(p) * inverse;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(edge1);
    let v = direction.dot(q) * inverse;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = edge2.dot(q) * inverse;
    (t > 1e-5).then_some(t)
}

/// Paints the scene-view sky gradient behind the grid.
pub(crate) fn sky(painter: &Painter, rect: Rect) {
    let mut mesh = Mesh::default();
    for (position, color) in [
        (rect.left_top(), theme::SKY_TOP),
        (rect.right_top(), theme::SKY_TOP),
        (pos2(rect.left(), rect.bottom()), theme::SKY_BOTTOM),
        (rect.right_bottom(), theme::SKY_BOTTOM),
    ] {
        mesh.colored_vertex(position, color);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    painter.add(egui::Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rays_hit_the_near_face_of_a_unit_cube() {
        let distance = slab(
            Vec3::new(0.0, 0.0, 5.0),
            Vec3::NEG_Z,
            (Vec3::splat(-0.5), Vec3::splat(0.5)),
        );
        assert_eq!(distance, Some(4.5));
        let triangle = intersect(
            Vec3::new(0.2, 0.2, 5.0),
            Vec3::NEG_Z,
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        );
        assert_eq!(triangle, Some(5.0));
    }

    #[test]
    fn tiled_cube_triangles_face_outward() {
        let (corners, indices) = tiled_cube([3.0, 1.0, 2.0]);
        assert!(indices.len() > 36);
        for triangle in indices.as_chunks::<3>().0 {
            let [a, b, c] = [0, 1, 2].map(|corner| corners[triangle[corner] as usize]);
            let normal = (b - a).cross(c - a);
            let center = (a + b + c) / 3.0;
            assert!(normal.dot(center) > 0.0, "inward triangle {triangle:?}");
        }
    }
}
