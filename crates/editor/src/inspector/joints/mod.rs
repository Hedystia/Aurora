//! Scene-level spatial joint authoring.

use egui::{DragValue, Ui};
use engine_core::scene::spatial::{JointFile, JointKind};

use crate::{Editor, strings, theme};

/// Shows the scene's spatial constraints. Prefab documents do not own scene joints.
pub(super) fn show(ui: &mut Ui, editor: &mut Editor) -> bool {
    if editor.project.prefab {
        return false;
    }
    let mut changed = false;
    ui.collapsing(strings::PHYSICS_3D_JOINTS, |ui| {
        let (bodies, node_count) = {
            let nodes = &editor.project.document.nodes;
            let bodies: Vec<(usize, String)> = nodes
                .iter()
                .enumerate()
                .filter(|(_, node)| {
                    node.spatial.as_ref().is_some_and(|spatial| {
                        spatial.body != engine_core::scene::spatial::Body::None
                    })
                })
                .map(|(index, node)| (index, node.name.clone()))
                .collect();
            (bodies, nodes.len())
        };
        let mut remove = None;
        for (joint_index, joint) in editor.project.document.joints.iter_mut().enumerate() {
            ui.push_id(joint_index, |ui| {
                ui.horizontal(|ui| {
                    ui.label((joint_index + 1).to_string());
                    if ui.small_button(strings::REMOVE).clicked() {
                        remove = Some(joint_index);
                    }
                });
                changed |= node_picker(
                    ui,
                    ("joint-a", joint_index),
                    strings::BODY_A,
                    &mut joint.a,
                    joint.b,
                    &bodies,
                );
                changed |= node_picker(
                    ui,
                    ("joint-b", joint_index),
                    strings::BODY_B,
                    &mut joint.b,
                    joint.a,
                    &bodies,
                );
                let kinds = [
                    (JointKind::Fixed, strings::JOINT_FIXED),
                    (JointKind::Ball, strings::JOINT_BALL),
                    (JointKind::Hinge, strings::JOINT_HINGE),
                    (JointKind::Slider, strings::JOINT_SLIDER),
                ];
                let caption = kinds
                    .iter()
                    .find(|(kind, _)| *kind == joint.kind)
                    .map_or(strings::JOINT_FIXED, |(_, label)| *label);
                let mut picked = None;
                ui.horizontal(|ui| {
                    ui.label(strings::JOINT_KIND);
                    egui::ComboBox::from_id_salt(("joint-kind", joint_index))
                        .selected_text(caption)
                        .show_ui(ui, |ui| {
                            for (kind, label) in kinds {
                                if ui.selectable_label(joint.kind == kind, label).clicked() {
                                    picked = Some(kind);
                                }
                            }
                        });
                });
                if let Some(kind) = picked
                    && kind != joint.kind
                {
                    joint.kind = kind;
                    changed = true;
                }
                changed |= vector(ui, strings::JOINT_ANCHOR_A, &mut joint.anchor_a, false);
                changed |= vector(ui, strings::JOINT_ANCHOR_B, &mut joint.anchor_b, false);
                changed |= vector(ui, strings::JOINT_AXIS, &mut joint.axis, true);
                if !valid(joint, node_count) {
                    ui.colored_label(theme::WARNING, strings::INVALID_JOINT);
                }
            });
        }
        if let Some(index) = remove {
            editor.project.document.joints.remove(index);
            changed = true;
        }
        let add = ui.add_enabled(bodies.len() >= 2, egui::Button::new(strings::ADD_JOINT));
        if add.clicked() && editor.project.add_joint().is_some() {
            changed = true;
        }
        if bodies.len() < 2 {
            ui.label(strings::NO_JOINT_BODIES);
        }
    });
    changed
}

/// Mirrors `JointFile::validate` and `SceneFile::validate` for live UI feedback.
fn valid(joint: &JointFile, node_count: usize) -> bool {
    joint.a < node_count
        && joint.b < node_count
        && joint.a != joint.b
        && joint
            .anchor_a
            .iter()
            .chain(&joint.anchor_b)
            .chain(&joint.axis)
            .all(|value| value.is_finite())
        && joint.axis.iter().any(|value| *value != 0.0)
}

fn node_picker(
    ui: &mut Ui,
    salt: (&str, usize),
    label: &str,
    endpoint: &mut usize,
    other: usize,
    bodies: &[(usize, String)],
) -> bool {
    let caption = bodies
        .iter()
        .find(|(index, _)| *index == *endpoint)
        .map_or_else(
            || strings::MISSING_BODY.to_owned(),
            |(_, name)| name.clone(),
        );
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(salt)
            .selected_text(caption)
            .show_ui(ui, |ui| {
                for (index, name) in bodies {
                    if *index == other {
                        continue;
                    }
                    if ui.selectable_label(*index == *endpoint, name).clicked() {
                        picked = Some(*index);
                    }
                }
            });
    });
    if let Some(next) = picked
        && next != *endpoint
    {
        *endpoint = next;
        return true;
    }
    false
}

/// Edits a local-space vector, rejecting values the scene validator would refuse.
fn vector(ui: &mut Ui, label: &str, values: &mut [f32; 3], nonzero: bool) -> bool {
    let previous = *values;
    ui.horizontal(|ui| {
        ui.label(label);
        for (axis, value) in [strings::AXIS_X, strings::AXIS_Y, strings::AXIS_Z]
            .into_iter()
            .zip(&mut *values)
        {
            ui.add(DragValue::new(value).speed(theme::VALUE_SPEED).prefix(axis));
        }
    });
    let acceptable = values.iter().all(|value| value.is_finite())
        && (!nonzero || values.iter().any(|value| *value != 0.0));
    if !acceptable {
        *values = previous;
        return false;
    }
    *values != previous
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joint() -> JointFile {
        JointFile {
            a: 0,
            b: 1,
            kind: JointKind::Hinge,
            anchor_a: [0.; 3],
            anchor_b: [0.; 3],
            axis: [0., 1., 0.],
        }
    }

    #[test]
    fn joint_validation_matches_scene_rules() {
        assert!(valid(&joint(), 2));
        assert!(!valid(&joint(), 1));
        assert!(!valid(&JointFile { b: 0, ..joint() }, 2));
        assert!(!valid(
            &JointFile {
                anchor_a: [f32::NAN, 0., 0.],
                ..joint()
            },
            2
        ));
        assert!(!valid(
            &JointFile {
                axis: [0.; 3],
                ..joint()
            },
            2
        ));
    }
}
