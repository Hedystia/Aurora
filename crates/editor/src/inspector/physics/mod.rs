//! Authored rigid body and shape fields.

use egui::{DragValue, Ui};
use engine_core::physics2d::{Body2D, BodyMode, Collider2D};

pub(super) fn show(ui: &mut Ui, component: &mut Option<Body2D>) -> bool {
    let mut changed = false;
    ui.collapsing("2D Physics", |ui| {
        let mut enabled = component.is_some();
        if ui.checkbox(&mut enabled, "Rigid body").changed() {
            *component = enabled.then(Body2D::default);
            changed = true;
        }
        let Some(body) = component else { return };
        ui.horizontal_wrapped(|ui| {
            for (mode, label) in [
                (BodyMode::Fixed, "Fixed"),
                (BodyMode::Kinematic, "Kinematic"),
                (BodyMode::Dynamic, "Dynamic"),
            ] {
                changed |= ui.selectable_value(&mut body.mode, mode, label).changed();
            }
        });
        changed |= ui.checkbox(&mut body.trigger, "Trigger").changed();
        let current = match &body.collider {
            Collider2D::Box { .. } => 0,
            Collider2D::Circle { .. } => 1,
            Collider2D::Polygon { .. } => 2,
        };
        let mut shape = current;
        ui.horizontal_wrapped(|ui| {
            for (index, name) in ["Box", "Circle", "Polygon"].iter().enumerate() {
                ui.selectable_value(&mut shape, index, *name);
            }
        });
        if shape != current {
            body.collider = match shape {
                1 => Collider2D::Circle { radius: 16.0 },
                2 => Collider2D::Polygon {
                    points: vec![[-16.0, -16.0], [16.0, -16.0], [0.0, 16.0]],
                },
                _ => Collider2D::default(),
            };
            changed = true;
        }
        match &mut body.collider {
            Collider2D::Box { size } => {
                changed |= super::pair(ui, "Size (world px)", size);
                *size = size.map(|side| side.max(0.01));
            }
            Collider2D::Circle { radius } => {
                ui.horizontal(|ui| {
                    ui.label("Radius (world px)");
                    changed |= ui
                        .add(DragValue::new(radius).range(0.01..=f32::MAX))
                        .changed();
                });
            }
            Collider2D::Polygon { points } => {
                let mut remove = None;
                let can_remove = points.len() > 3;
                for (index, point) in points.iter_mut().enumerate() {
                    ui.push_id(index, |ui| {
                        changed |= super::pair(ui, &format!("Point {}", index + 1), point);
                        if can_remove && ui.small_button("Remove point").clicked() {
                            remove = Some(index);
                        }
                    });
                }
                if let Some(i) = remove {
                    points.remove(i);
                    changed = true;
                }
                if points.len() < 128 && ui.button("Add point").clicked() {
                    let a = points[0];
                    let b = points[points.len() - 1];
                    points.push([(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]);
                    changed = true;
                }
            }
        }
        if let Err(error) = body.validate() {
            ui.label(error.to_string());
        }
    });
    changed
}
