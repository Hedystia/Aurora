//! Authored 2D collider outlines in the scene viewport.

use egui::{Color32, Painter, Pos2, Stroke, vec2};
use engine_core::{physics2d::Collider2D, scene::NodeState};

pub(super) fn outline(
    painter: &Painter,
    _nodes: &[NodeState],
    node: &NodeState,
    origin: Pos2,
    scale: f32,
    collider: &Collider2D,
) {
    let local_points = match collider {
        Collider2D::Box { size } => vec![
            [-size[0] * 0.5, -size[1] * 0.5],
            [size[0] * 0.5, -size[1] * 0.5],
            [size[0] * 0.5, size[1] * 0.5],
            [-size[0] * 0.5, size[1] * 0.5],
        ],
        Collider2D::Circle { radius } => (0..48)
            .map(|step| {
                let angle = std::f32::consts::TAU * step as f32 / 48.0;
                [radius * angle.cos(), radius * angle.sin()]
            })
            .collect(),
        Collider2D::Polygon { points } => points.clone(),
    };
    let base = node.world.point([0.0; 2]);
    let points = local_points
        .into_iter()
        .map(|point| {
            let world = node.world.point(point);
            origin + vec2((world[0] - base[0]) * scale, -(world[1] - base[1]) * scale)
        })
        .collect();
    painter.add(egui::Shape::closed_line(
        points,
        Stroke::new(1.5, Color32::from_rgb(240, 182, 91)),
    ));
}
