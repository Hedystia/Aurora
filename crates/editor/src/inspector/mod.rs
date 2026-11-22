//! Live transform editing and type-checked script properties.

use std::collections::BTreeMap;

use codegen::PropertyType;
use egui::{DragValue, Ui};
use engine_core::save::{FrameFile, PropertyValue, SensorFile};

use crate::{Editor, node_tree::Drag, project::extension, strings, theme};

pub(super) fn value_matches(kind: PropertyType, value: &PropertyValue) -> bool {
    matches!(
        (kind, value),
        (PropertyType::Number, PropertyValue::Number(_))
            | (PropertyType::Boolean, PropertyValue::Boolean(_))
            | (PropertyType::String, PropertyValue::String(_))
            | (PropertyType::Node, PropertyValue::Node(_))
            | (PropertyType::Prefab, PropertyValue::Prefab(_))
    )
}

fn reference(kind: PropertyType, payload: &Drag) -> Option<PropertyValue> {
    match (kind, payload) {
        (PropertyType::Node, Drag::Node(index)) => Some(PropertyValue::Node(*index)),
        (PropertyType::Prefab, Drag::Asset(path)) if extension(path) == "aurora-prefab" => {
            Some(PropertyValue::Prefab(path.clone()))
        }
        _ => None,
    }
}

pub(super) fn show(ui: &mut Ui, editor: &mut Editor) {
    let Some(selected) = editor.project.selected else {
        ui.label(strings::NO_SELECTION);
        return;
    };
    let names: Vec<_> = editor
        .project
        .document
        .nodes
        .iter()
        .map(|node| node.name.clone())
        .collect();
    let root = editor.project.root.clone();
    let mut changed = false;

    egui::ScrollArea::vertical().show(ui, |ui| {
        let node = &mut editor.project.document.nodes[selected];
        let mut appearance = node.appearance.unwrap_or_default();
        ui.horizontal(|ui| {
            if ui
                .checkbox(&mut appearance.active, "")
                .on_hover_text(strings::ACTIVE)
                .changed()
            {
                node.appearance = Some(appearance);
                changed = true;
            }
            changed |= ui.text_edit_singleline(&mut node.name).changed();
        });
        egui::CollapsingHeader::new(strings::NODE)
            .default_open(true)
            .show(ui, |ui| {
                changed |= pair(ui, strings::POSITION, &mut node.transform.position);
                changed |= ui
                    .add(
                        DragValue::new(&mut node.transform.rotation)
                            .speed(theme::ANGLE_SPEED)
                            .prefix(strings::ROTATION),
                    )
                    .changed();
                changed |= pair(ui, strings::SCALE, &mut node.transform.scale);
                let mut visual_changed = pair(ui, strings::ANCHOR, &mut appearance.anchor);
                visual_changed |= pair(ui, strings::SKEW, &mut appearance.skew);
                ui.horizontal(|ui| {
                    ui.label(strings::COLOR);
                    visual_changed |= ui
                        .color_edit_button_srgba_unmultiplied(&mut appearance.color)
                        .changed();
                });
                visual_changed |= ui
                    .add(DragValue::new(&mut appearance.opacity).prefix(strings::OPACITY))
                    .changed();
                visual_changed |= ui
                    .add(DragValue::new(&mut appearance.layer).prefix(strings::DRAW_ORDER))
                    .changed();
                if visual_changed {
                    node.appearance = Some(appearance);
                    changed = true;
                }
            });
        ui.separator();
        let mut sprite_drop = None;
        let mut remove_sprite = false;
        if let Some(sprite) = &mut editor.project.document.nodes[selected].sprite {
            ui.collapsing(strings::SPRITE_NODE, |ui| {
                let response = ui.button(sprite.png.display().to_string());
                if let Some(payload) = response.dnd_release_payload::<Drag>()
                    && let Drag::Asset(path) = payload.as_ref()
                {
                    sprite_drop = Some(path.clone());
                }
                changed |= pair(ui, strings::SIZE, &mut sprite.size);
                for size in &mut sprite.size {
                    *size = size.max(1.0);
                }
                remove_sprite = ui.button(strings::REMOVE).clicked();
            });
        } else {
            let response = ui.button(strings::DROP_PNG);
            if let Some(payload) = response.dnd_release_payload::<Drag>()
                && let Drag::Asset(path) = payload.as_ref()
            {
                sprite_drop = Some(path.clone());
            }
        }
        if let Some(path) = sprite_drop {
            let result = editor.project.drop_sprite(Some(selected), &path);
            editor.report(result);
        }
        if remove_sprite {
            editor.project.document.nodes[selected].sprite = None;
            if let Some(data) = &mut editor.project.document.gameplay {
                data.frames.retain(|bank| bank.node != selected);
            }
            changed = true;
        }
        changed |= gameplay(ui, editor, selected);
        let mut remove_script = None;
        for (index, script) in editor.project.document.nodes[selected]
            .scripts
            .iter_mut()
            .enumerate()
        {
            let definitions = editor
                .metadata
                .get(&root.join(&script.path))
                .and_then(|map| map.get(&script.component));
            ui.push_id(index, |ui| {
                egui::CollapsingHeader::new(&script.component)
                    .default_open(true)
                    .show(ui, |ui| {
                        if let Some(definitions) = definitions {
                            changed |= fields(ui, definitions, &mut script.values, &names, &root);
                        } else {
                            ui.label(strings::MISSING_METADATA);
                        }
                        if ui.button(strings::REMOVE).clicked() {
                            remove_script = Some(index);
                        }
                    });
            });
        }
        if let Some(index) = remove_script {
            editor.project.document.nodes[selected]
                .scripts
                .remove(index);
            changed = true;
        }
        if ui.button(strings::ADD_COMPONENT).clicked() {
            editor.component_picker = true;
            editor.component_search.clear();
        }
    });
    if changed {
        editor.project.changed();
    }
}

fn gameplay(ui: &mut Ui, editor: &mut Editor, selected: usize) -> bool {
    if editor.project.prefab {
        return false;
    }
    let document = &mut editor.project.document;
    let node = &document.nodes[selected];
    let root = &editor.project.root;
    let mut changed = false;
    ui.collapsing(strings::GAMEPLAY, |ui| {
        let mut resource = document.gameplay.take();
        let mut sensor = resource
            .as_ref()
            .is_some_and(|data| data.sensors.iter().any(|sensor| sensor.node == selected));
        if ui.checkbox(&mut sensor, strings::CONTACT_SENSOR).changed() {
            let data = resource.get_or_insert_with(Default::default);
            if sensor {
                let size = node.sprite.as_ref().map_or([24.0; 2], |sprite| sprite.size);
                data.sensors.push(SensorFile {
                    node: selected,
                    size,
                });
            } else {
                data.sensors.retain(|area| area.node != selected);
            }
            changed = true;
        }
        if let Some(sensor) = resource.as_mut().and_then(|data| {
            data.sensors
                .iter_mut()
                .find(|sensor| sensor.node == selected)
        }) && pair(ui, strings::SENSOR_SIZE, &mut sensor.size)
        {
            sensor.size = sensor.size.map(|side| side.max(1.0));
            changed = true;
        }
        if node.sprite.is_some() {
            ui.label(strings::SPRITE_FRAMES);
            if let Some(path) = asset_drop(ui, strings::ADD_FRAME, &["png"], root) {
                let data = resource.get_or_insert_with(Default::default);
                if let Some(bank) = data.frames.iter_mut().find(|bank| bank.node == selected) {
                    if bank.png.len() < 256 {
                        bank.png.push(path);
                        changed = true;
                    }
                } else {
                    data.frames.push(FrameFile {
                        node: selected,
                        png: vec![path],
                    });
                    changed = true;
                }
            }
            if let Some(data) = &mut resource {
                if let Some(bank) = data.frames.iter_mut().find(|bank| bank.node == selected) {
                    changed |= entry_list(ui, &mut bank.png);
                }
                data.frames.retain(|bank| !bank.png.is_empty());
            }
        }
        if node.parent.is_none() {
            ui.label(strings::ONE_SHOT_BANK);
            if let Some(path) = asset_drop(ui, strings::ADD_SOUND, &["wav", "mp3"], root) {
                let data = resource.get_or_insert_with(Default::default);
                if data.sounds.len() < 128 {
                    data.sounds.push(path);
                    changed = true;
                }
            }
            if let Some(data) = &mut resource {
                changed |= entry_list(ui, &mut data.sounds);
            }
        }
        document.gameplay = if changed {
            resource.filter(|data| {
                !data.frames.is_empty() || !data.sensors.is_empty() || !data.sounds.is_empty()
            })
        } else {
            resource
        };
    });
    changed
}

fn asset_drop(
    ui: &mut Ui,
    label: &str,
    extensions: &[&str],
    root: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let response = ui.button(label);
    let valid = |payload: &Drag| match payload {
        Drag::Asset(path) => extensions.contains(&extension(path)),
        _ => false,
    };
    if let Some(payload) = response.dnd_hover_payload::<Drag>() {
        let tint = if valid(payload.as_ref()) {
            theme::ACCEPT
        } else {
            theme::REJECT
        };
        ui.painter().rect_stroke(
            response.rect,
            theme::ROUND,
            (theme::OUTLINE, tint),
            egui::StrokeKind::Inside,
        );
    }
    let payload = response.dnd_release_payload::<Drag>()?;
    match payload.as_ref() {
        Drag::Asset(path) if valid(payload.as_ref()) => {
            Some(path.strip_prefix(root).unwrap_or(path).to_path_buf())
        }
        _ => None,
    }
}

fn entry_list(ui: &mut Ui, paths: &mut Vec<std::path::PathBuf>) -> bool {
    let mut remove = None;
    for (index, path) in paths.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(format!("{index} · {}", path.display()));
            if ui
                .small_button(strings::CLEAR)
                .on_hover_text(strings::REMOVE)
                .clicked()
            {
                remove = Some(index);
            }
        });
    }
    if let Some(index) = remove {
        paths.remove(index);
        return true;
    }
    false
}

fn fields(
    ui: &mut Ui,
    metadata: &codegen::Metadata,
    values: &mut BTreeMap<String, PropertyValue>,
    names: &[String],
    root: &std::path::Path,
) -> bool {
    let mut changed = false;
    for (name, property) in &metadata.props {
        ui.push_id(name, |ui| {
            ui.horizontal(|ui| {
                let label = ui.label(name);
                if let Some(tooltip) = &property.tooltip {
                    label.on_hover_text(tooltip);
                }
                let current = values
                    .get(name)
                    .filter(|value| value_matches(property.kind, value));
                let next = match property.kind {
                    PropertyType::Number => {
                        let mut value = if let Some(PropertyValue::Number(value)) = current {
                            *value
                        } else {
                            0.0
                        };
                        ui.add(DragValue::new(&mut value))
                            .changed()
                            .then_some(PropertyValue::Number(value))
                    }
                    PropertyType::Boolean => {
                        let mut value = matches!(current, Some(PropertyValue::Boolean(true)));
                        ui.checkbox(&mut value, "")
                            .changed()
                            .then_some(PropertyValue::Boolean(value))
                    }
                    PropertyType::String => {
                        let mut value = if let Some(PropertyValue::String(value)) = current {
                            value.clone()
                        } else {
                            String::new()
                        };
                        ui.text_edit_singleline(&mut value)
                            .changed()
                            .then_some(PropertyValue::String(value))
                    }
                    PropertyType::Node | PropertyType::Prefab => {
                        let caption = match current {
                            Some(PropertyValue::Node(index)) => names.get(*index).cloned(),
                            Some(PropertyValue::Prefab(path)) => Some(path.display().to_string()),
                            _ => None,
                        };
                        let response =
                            ui.button(caption.as_deref().unwrap_or(strings::EMPTY_REFERENCE));
                        if let Some(payload) = response.dnd_hover_payload::<Drag>() {
                            let valid = reference(property.kind, payload.as_ref()).is_some();
                            ui.painter().rect_stroke(
                                response.rect,
                                theme::ROUND,
                                (
                                    theme::OUTLINE,
                                    if valid { theme::ACCEPT } else { theme::REJECT },
                                ),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if ui.small_button(strings::CLEAR).clicked() {
                            changed |= values.remove(name).is_some();
                        }
                        response
                            .dnd_release_payload::<Drag>()
                            .and_then(|payload| reference(property.kind, payload.as_ref()))
                            .map(|value| match value {
                                PropertyValue::Prefab(path) => PropertyValue::Prefab(
                                    path.strip_prefix(root).unwrap_or(&path).to_path_buf(),
                                ),
                                _ => value,
                            })
                    }
                };
                if let Some(next) = next {
                    values.insert(name.clone(), next);
                    changed = true;
                }
            });
        });
    }
    changed
}

fn pair(ui: &mut Ui, label: &str, values: &mut [f32; 2]) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        let first = ui
            .add(DragValue::new(&mut values[0]).speed(theme::VALUE_SPEED))
            .changed();
        let second = ui
            .add(DragValue::new(&mut values[1]).speed(theme::VALUE_SPEED))
            .changed();
        first || second
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_reject_wrong_drag_types() {
        assert!(reference(PropertyType::Node, &Drag::Node(4)).is_some());
        assert!(
            reference(
                PropertyType::Node,
                &Drag::Asset("marker.aurora-prefab".into())
            )
            .is_none()
        );
        assert!(reference(PropertyType::Prefab, &Drag::Asset("sprite.png".into())).is_none());
        assert!(
            reference(
                PropertyType::Prefab,
                &Drag::Asset("marker.aurora-prefab".into())
            )
            .is_some()
        );
    }
}
