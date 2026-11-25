//! Live transform editing and type-checked script properties.

mod joints;
mod physics;

pub(crate) mod visual;

pub(crate) mod spatial;

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use codegen::{NodeFilter, PropertyType};
use egui::{DragValue, Ui};
use engine_core::save::{FrameFile, PropertyValue, SensorFile};

use crate::{
    Editor, icons,
    node_tree::Drag,
    project::{extension, is_audio, is_frame, is_prefab},
    strings, theme,
};

pub(super) fn value_matches(kind: PropertyType, value: &PropertyValue) -> bool {
    matches!(
        (kind, value),
        (PropertyType::Number, PropertyValue::Number(_))
            | (PropertyType::Integer, PropertyValue::Integer(_))
            | (PropertyType::Boolean, PropertyValue::Boolean(_))
            | (PropertyType::String, PropertyValue::String(_))
            | (PropertyType::Text, PropertyValue::Text(_))
            | (PropertyType::Vector2, PropertyValue::Vector2(_))
            | (PropertyType::Vector3, PropertyValue::Vector3(_))
            | (PropertyType::Size, PropertyValue::Size(_))
            | (PropertyType::Rect, PropertyValue::Rect(_))
            | (PropertyType::Bitmask, PropertyValue::Bitmask(_))
            | (PropertyType::Color, PropertyValue::Color(_))
            | (PropertyType::Enum, PropertyValue::Enum(_))
            | (PropertyType::Node, PropertyValue::Node(_))
            | (PropertyType::Prefab, PropertyValue::Prefab(_))
            | (PropertyType::AudioClip, PropertyValue::AudioClip(_))
            | (PropertyType::SpriteFrame, PropertyValue::SpriteFrame(_))
            | (PropertyType::Component, PropertyValue::Component { .. })
            | (PropertyType::NumberArray, PropertyValue::Numbers(_))
            | (PropertyType::BooleanArray, PropertyValue::Booleans(_))
            | (PropertyType::StringArray, PropertyValue::Strings(_))
            | (PropertyType::NodeArray, PropertyValue::Nodes(_))
            | (PropertyType::PrefabArray, PropertyValue::Prefabs(_))
            | (PropertyType::AudioClipArray, PropertyValue::AudioClips(_))
            | (
                PropertyType::SpriteFrameArray,
                PropertyValue::SpriteFrames(_)
            )
    )
}

fn reference(kind: PropertyType, payload: &Drag) -> Option<PropertyValue> {
    match (kind, payload) {
        (PropertyType::Node, Drag::Node(index)) => Some(PropertyValue::Node(*index)),
        (PropertyType::Prefab, Drag::Asset(path)) if is_prefab(path) => {
            Some(PropertyValue::Prefab(path.clone()))
        }
        (PropertyType::NodeArray, Drag::Node(index)) => Some(PropertyValue::Nodes(vec![*index])),
        (PropertyType::PrefabArray, Drag::Asset(path)) if is_prefab(path) => {
            Some(PropertyValue::Prefabs(vec![path.clone()]))
        }
        (PropertyType::AudioClipArray, Drag::Asset(path)) if is_audio(path) => {
            Some(PropertyValue::AudioClips(vec![path.clone()]))
        }
        (PropertyType::SpriteFrameArray, Drag::Asset(path)) if is_frame(path) => {
            Some(PropertyValue::SpriteFrames(vec![path.clone()]))
        }
        _ => None,
    }
}

pub(super) fn show(ui: &mut Ui, editor: &mut Editor) {
    let mut changed = joints::show(ui, editor);
    let Some(selected) = editor.project.selected else {
        if changed {
            editor.project.changed();
        }
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
    let node_is_text: Vec<_> = editor
        .project
        .document
        .nodes
        .iter()
        .map(|node| {
            matches!(
                &node.visual,
                Some(engine_core::save::visual::Visual::Text { .. })
            )
        })
        .collect();
    let node_is_image: Vec<_> = editor
        .project
        .document
        .nodes
        .iter()
        .map(|node| {
            node.sprite.is_some()
                || matches!(
                    &node.visual,
                    Some(
                        engine_core::save::visual::Visual::Atlas { .. }
                            | engine_core::save::visual::Visual::TileMap { .. }
                            | engine_core::save::visual::Visual::NineSlice { .. }
                    )
                )
        })
        .collect();
    let components: Vec<_> = editor
        .project
        .document
        .nodes
        .iter()
        .enumerate()
        .flat_map(|(index, node)| {
            node.scripts
                .iter()
                .map(move |script| (index, script.component.clone()))
        })
        .collect();
    let audio_assets: Vec<PathBuf> = editor
        .project
        .document
        .gameplay
        .as_ref()
        .map(|gameplay| gameplay.sounds.clone())
        .unwrap_or_default();
    let prefab_assets: Vec<PathBuf> = editor
        .asset_paths
        .iter()
        .filter(|path| is_prefab(path))
        .cloned()
        .collect();
    let frame_assets: Vec<PathBuf> = editor
        .asset_paths
        .iter()
        .filter(|path| extension(path) == "png")
        .cloned()
        .collect();
    let root = editor.project.root.clone();
    let parent = editor.project.document.nodes[selected].parent;
    let parent_name = parent
        .and_then(|index| names.get(index).cloned())
        .unwrap_or_else(|| strings::ROOT_PARENT.to_owned());
    let parent_valid: Vec<bool> = (0..editor.project.document.nodes.len())
        .map(|target| editor.reparent_allowed(selected, Some(target)))
        .collect();
    let mut reparent = None;

    egui::ScrollArea::vertical().show(ui, |ui| {
        let node = &mut editor.project.document.nodes[selected];
        if let Some(spatial) = &mut node.spatial {
            changed |= spatial::show(ui, spatial);
        }
        let mut appearance = node.appearance.unwrap_or_default();
        let node_icon = if node.name == strings::CANVAS {
            icons::Icon::Canvas
        } else if matches!(
            node.visual.as_ref(),
            Some(engine_core::save::visual::Visual::Text { .. })
        ) {
            icons::Icon::Text
        } else if node.sprite.is_some() {
            icons::Icon::Sprite
        } else {
            icons::Icon::Node
        };
        ui.horizontal(|ui| {
            icons::inline(ui, node_icon, theme::ACCENT);
            let width = (ui.available_width() - theme::ICON_BUTTON - 8.0).max(40.0);
            changed |= ui
                .add_sized(
                    [width, theme::ROW_HEIGHT],
                    egui::TextEdit::singleline(&mut node.name),
                )
                .changed();
            if icons::icon_button(
                ui,
                if appearance.active {
                    icons::Icon::Eye
                } else {
                    icons::Icon::EyeOff
                },
                strings::ACTIVE,
            )
            .clicked()
            {
                appearance.active = !appearance.active;
                node.appearance = Some(appearance);
                changed = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label(strings::PARENT);
            let response = ui
                .add(egui::Button::new(parent_name.clone()).min_size(egui::vec2(
                    (ui.available_width() - 30.0).max(40.0),
                    theme::ROW_HEIGHT,
                )))
                .on_hover_text(strings::SET_PARENT);
            if let Some(payload) = response.dnd_hover_payload::<Drag>() {
                let valid = match payload.as_ref() {
                    Drag::Node(target) => parent_valid.get(*target).copied().unwrap_or(false),
                    _ => false,
                };
                icons::paint_drop_frame(ui.painter(), response.rect, valid);
            }
            if parent.is_some()
                && icons::icon_button(ui, icons::Icon::Parent, strings::MOVE_TO_ROOT).clicked()
            {
                reparent = Some(None);
            }
            if let Some(payload) = response.dnd_release_payload::<Drag>()
                && let Drag::Node(target) = payload.as_ref()
                && parent_valid.get(*target).copied().unwrap_or(false)
            {
                reparent = Some(Some(*target));
            }
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
        } else if editor.project.document.nodes[selected].visual.is_none() {
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
        changed |= physics::show(ui, &mut editor.project.document.nodes[selected].physics2d);
        changed |= visual::show(
            ui,
            &mut editor.project.document.nodes[selected].visual,
            &root,
        );
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
                            changed |= fields(
                                ui,
                                definitions,
                                &mut script.values,
                                &names,
                                &node_is_text,
                                &node_is_image,
                                &components,
                                &prefab_assets,
                                &audio_assets,
                                &frame_assets,
                                &root,
                            );
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
        if !editor.project.prefab {
            let images: Vec<_> = editor.project.document.nodes[selected]
                .scripts
                .iter()
                .flat_map(|script| script.values.values())
                .flat_map(|value| match value {
                    PropertyValue::SpriteFrame(path) => vec![path.clone()],
                    PropertyValue::SpriteFrames(paths) => paths.clone(),
                    _ => Vec::new(),
                })
                .collect();
            if !images.is_empty() {
                let gameplay = editor
                    .project
                    .document
                    .gameplay
                    .get_or_insert_with(Default::default);
                let bank_index = gameplay
                    .frames
                    .iter()
                    .position(|bank| bank.node == selected);
                let bank_index = bank_index.unwrap_or_else(|| {
                    gameplay.frames.push(FrameFile {
                        node: selected,
                        png: Vec::new(),
                    });
                    gameplay.frames.len() - 1
                });
                let bank = &mut gameplay.frames[bank_index];
                let previous = bank.png.len();
                for path in images {
                    if bank.png.len() >= 256 {
                        break;
                    }
                    if !bank.png.contains(&path) {
                        bank.png.push(path);
                    }
                }
                changed |= previous != bank.png.len();
                if bank.png.len() == 256 && previous == 256 {
                    ui.colored_label(theme::WARNING, "Sprite frame bank is full (256 images).");
                }
            }
        }
        if ui.button(strings::ADD_COMPONENT).clicked() {
            editor.component_picker = true;
            editor.component_search.clear();
        }
    });
    if let Some(parent) = reparent {
        let result = editor.project.reparent(selected, parent);
        editor.report(result);
        changed = true;
    }
    if changed {
        editor.project.changed();
    }
}

fn node_filter_matches(
    filter: Option<NodeFilter>,
    value: &PropertyValue,
    node_is_text: &[bool],
    node_is_image: &[bool],
) -> bool {
    match (filter, value) {
        (None, PropertyValue::Node(_)) => true,
        (Some(NodeFilter::Text), PropertyValue::Node(index)) => {
            node_is_text.get(*index).copied().unwrap_or(false)
        }
        (Some(NodeFilter::Image), PropertyValue::Node(index)) => {
            node_is_image.get(*index).copied().unwrap_or(false)
        }
        (_, PropertyValue::Prefab(_)) => true,
        _ => false,
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

#[allow(clippy::too_many_arguments)]
fn fields(
    ui: &mut Ui,
    metadata: &codegen::Metadata,
    values: &mut BTreeMap<String, PropertyValue>,
    names: &[String],
    node_is_text: &[bool],
    node_is_image: &[bool],
    components: &[(usize, String)],
    prefab_assets: &[std::path::PathBuf],
    audio_assets: &[std::path::PathBuf],
    frame_assets: &[std::path::PathBuf],
    root: &std::path::Path,
) -> bool {
    let mut changed = false;
    let mut group = None;
    let mut properties: Vec<_> = metadata.props.iter().collect();
    properties.sort_by(|(left_name, left), (right_name, right)| {
        left.group
            .cmp(&right.group)
            .then_with(|| left_name.cmp(right_name))
    });
    for (name, property) in properties {
        if property.group != group {
            if let Some(label) = &property.group {
                ui.strong(label);
            }
            group.clone_from(&property.group);
        }
        ui.push_id(name, |ui| {
            ui.horizontal(|ui| {
                let label = ui.label(name);
                if let Some(tooltip) = &property.tooltip {
                    label.on_hover_text(tooltip);
                }
                ui.add_enabled_ui(!property.readonly, |ui| {
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
                            let response = if property.slider {
                                ui.add(
                                    egui::Slider::new(
                                        &mut value,
                                        property.min.unwrap_or(0.0)..=property.max.unwrap_or(1.0),
                                    )
                                    .step_by(property.step.unwrap_or(1.0)),
                                )
                            } else {
                                let mut input = DragValue::new(&mut value);
                                if property.min.is_some() || property.max.is_some() {
                                    input = input.range(
                                        property.min.unwrap_or(f64::MIN)
                                            ..=property.max.unwrap_or(f64::MAX),
                                    );
                                }
                                if let Some(step) = property.step {
                                    input = input.speed(step);
                                }
                                ui.add(input)
                            };
                            response.changed().then_some(PropertyValue::Number(value))
                        }
                        PropertyType::Integer => {
                            let mut value = if let Some(PropertyValue::Integer(value)) = current {
                                *value
                            } else {
                                0
                            };
                            let response = if property.slider {
                                ui.add(
                                    egui::Slider::new(
                                        &mut value,
                                        property.min.unwrap_or(i64::MIN as f64).ceil() as i64
                                            ..=property.max.unwrap_or(i64::MAX as f64).floor()
                                                as i64,
                                    )
                                    .step_by(property.step.unwrap_or(1.0)),
                                )
                            } else {
                                let mut input = DragValue::new(&mut value);
                                if property.min.is_some() || property.max.is_some() {
                                    input = input.range(
                                        property.min.unwrap_or(i64::MIN as f64).ceil() as i64
                                            ..=property.max.unwrap_or(i64::MAX as f64).floor()
                                                as i64,
                                    );
                                }
                                if let Some(step) = property.step {
                                    input = input.speed(step);
                                }
                                ui.add(input)
                            };
                            response.changed().then_some(PropertyValue::Integer(value))
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
                        PropertyType::Text => {
                            let mut value = if let Some(PropertyValue::Text(value)) = current {
                                value.clone()
                            } else {
                                String::new()
                            };
                            ui.add(egui::TextEdit::multiline(&mut value).desired_rows(3))
                                .changed()
                                .then_some(PropertyValue::Text(value))
                        }
                        PropertyType::Vector2 => {
                            let mut value = if let Some(PropertyValue::Vector2(value)) = current {
                                *value
                            } else {
                                [0.0; 2]
                            };
                            let changed = ui
                                .horizontal(|ui| {
                                    ui.add(DragValue::new(&mut value[0]).prefix("X ")).changed()
                                        | ui.add(DragValue::new(&mut value[1]).prefix("Y "))
                                            .changed()
                                })
                                .inner;
                            changed.then_some(PropertyValue::Vector2(value))
                        }
                        PropertyType::Vector3 => {
                            let mut value = if let Some(PropertyValue::Vector3(value)) = current {
                                *value
                            } else {
                                [0.0; 3]
                            };
                            let changed = ui
                                .horizontal(|ui| {
                                    ui.add(DragValue::new(&mut value[0]).prefix("X ")).changed()
                                        | ui.add(DragValue::new(&mut value[1]).prefix("Y "))
                                            .changed()
                                        | ui.add(DragValue::new(&mut value[2]).prefix("Z "))
                                            .changed()
                                })
                                .inner;
                            changed.then_some(PropertyValue::Vector3(value))
                        }
                        PropertyType::Size => {
                            let mut value = if let Some(PropertyValue::Size(value)) = current {
                                *value
                            } else {
                                [0.0; 2]
                            };
                            let changed = ui
                                .horizontal(|ui| {
                                    ui.add(DragValue::new(&mut value[0]).prefix("W ")).changed()
                                        | ui.add(DragValue::new(&mut value[1]).prefix("H "))
                                            .changed()
                                })
                                .inner;
                            changed.then_some(PropertyValue::Size(value))
                        }
                        PropertyType::Rect => {
                            let mut value = if let Some(PropertyValue::Rect(value)) = current {
                                *value
                            } else {
                                [0.0; 4]
                            };
                            let changed = ui
                                .horizontal(|ui| {
                                    ui.add(DragValue::new(&mut value[0]).prefix("X ")).changed()
                                        | ui.add(DragValue::new(&mut value[1]).prefix("Y "))
                                            .changed()
                                        | ui.add(DragValue::new(&mut value[2]).prefix("W "))
                                            .changed()
                                        | ui.add(DragValue::new(&mut value[3]).prefix("H "))
                                            .changed()
                                })
                                .inner;
                            changed.then_some(PropertyValue::Rect(value))
                        }
                        PropertyType::Bitmask => {
                            let mut value = if let Some(PropertyValue::Bitmask(value)) = current {
                                *value
                            } else {
                                0
                            };
                            let changed = ui
                                .add(DragValue::new(&mut value))
                                .on_hover_text(format!("0x{value:08X}"))
                                .changed();
                            changed.then_some(PropertyValue::Bitmask(value))
                        }
                        PropertyType::Color => {
                            let mut value = if let Some(PropertyValue::Color(value)) = current {
                                *value
                            } else {
                                [255; 4]
                            };
                            ui.color_edit_button_srgba_unmultiplied(&mut value)
                                .changed()
                                .then_some(PropertyValue::Color(value))
                        }
                        PropertyType::Enum => {
                            let choices = property.choices.as_deref().unwrap_or_default();
                            let mut value = match current {
                                Some(PropertyValue::Enum(value)) => value.clone(),
                                _ => choices.first().cloned().unwrap_or_default(),
                            };
                            let mut changed_enum = false;
                            egui::ComboBox::from_id_salt(("enum", name))
                                .selected_text(&value)
                                .show_ui(ui, |ui| {
                                    for choice in choices {
                                        if ui.selectable_label(value == *choice, choice).clicked() {
                                            value.clone_from(choice);
                                            changed_enum = true;
                                        }
                                    }
                                });
                            changed_enum.then_some(PropertyValue::Enum(value))
                        }
                        PropertyType::Node | PropertyType::Prefab => {
                            let compatible = match current {
                                Some(value @ PropertyValue::Node(_)) => node_filter_matches(
                                    property.node_filter,
                                    value,
                                    node_is_text,
                                    node_is_image,
                                ),
                                _ => true,
                            };
                            let caption = match current {
                                Some(PropertyValue::Node(index)) => names.get(*index).cloned(),
                                Some(PropertyValue::Prefab(path)) => {
                                    Some(path.display().to_string())
                                }
                                _ => None,
                            };
                            let caption =
                                caption.unwrap_or_else(|| strings::EMPTY_REFERENCE.into());
                            let response = ui.button(if compatible {
                                caption.clone()
                            } else {
                                format!("{caption} (incompatible)")
                            });
                            if let Some(payload) = response.dnd_hover_payload::<Drag>() {
                                let valid = reference(property.kind, payload.as_ref()).is_some_and(
                                    |value| {
                                        node_filter_matches(
                                            property.node_filter,
                                            &value,
                                            node_is_text,
                                            node_is_image,
                                        )
                                    },
                                );
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
                                .filter(|value| {
                                    node_filter_matches(
                                        property.node_filter,
                                        value,
                                        node_is_text,
                                        node_is_image,
                                    )
                                })
                                .map(|value| match value {
                                    PropertyValue::Prefab(path) => PropertyValue::Prefab(
                                        path.strip_prefix(root).unwrap_or(&path).to_path_buf(),
                                    ),
                                    _ => value,
                                })
                        }
                        PropertyType::AudioClip => {
                            let current = match current {
                                Some(PropertyValue::AudioClip(path)) => Some(path.as_path()),
                                _ => None,
                            };
                            match asset_picker(ui, ("audio", name), audio_assets, current, root) {
                                Some(None) => {
                                    changed |= values.remove(name).is_some();
                                    None
                                }
                                Some(Some(path)) => Some(PropertyValue::AudioClip(path)),
                                None => None,
                            }
                        }
                        PropertyType::SpriteFrame => {
                            let current = match current {
                                Some(PropertyValue::SpriteFrame(path)) => Some(path.as_path()),
                                _ => None,
                            };
                            match asset_picker(ui, ("frame", name), frame_assets, current, root) {
                                Some(None) => {
                                    changed |= values.remove(name).is_some();
                                    None
                                }
                                Some(Some(path)) => Some(PropertyValue::SpriteFrame(path)),
                                None => None,
                            }
                        }
                        PropertyType::Component => {
                            let expected = property.component.as_deref();
                            let selected = match current {
                                Some(PropertyValue::Component { node, component }) => {
                                    Some((*node, component.clone()))
                                }
                                _ => None,
                            };
                            let caption = selected.as_ref().map_or_else(
                                || strings::EMPTY_REFERENCE.to_owned(),
                                |(node, component)| {
                                    format!(
                                        "{} · {component}",
                                        names.get(*node).map_or("?", String::as_str)
                                    )
                                },
                            );
                            let mut picked = None;
                            egui::ComboBox::from_id_salt(("component", name))
                                .selected_text(caption)
                                .show_ui(ui, |ui| {
                                    for (node, component) in components {
                                        if expected.is_some_and(|expected| expected != component) {
                                            continue;
                                        }
                                        let is_selected =
                                            selected.as_ref().is_some_and(|selected| {
                                                selected.0 == *node && selected.1 == *component
                                            });
                                        let label = format!(
                                            "{} · {component}",
                                            names.get(*node).map_or("?", String::as_str)
                                        );
                                        if ui.selectable_label(is_selected, label).clicked() {
                                            picked = Some((*node, component.clone()));
                                        }
                                    }
                                });
                            if ui.small_button(strings::CLEAR).clicked() {
                                changed |= values.remove(name).is_some();
                            }
                            picked.map(|(node, component)| PropertyValue::Component {
                                node,
                                component,
                            })
                        }
                        PropertyType::NumberArray => {
                            let mut items = match current {
                                Some(PropertyValue::Numbers(items)) => items.clone(),
                                _ => Vec::new(),
                            };
                            number_array(ui, &mut items).then_some(PropertyValue::Numbers(items))
                        }
                        PropertyType::BooleanArray => {
                            let mut items = match current {
                                Some(PropertyValue::Booleans(items)) => items.clone(),
                                _ => Vec::new(),
                            };
                            boolean_array(ui, &mut items).then_some(PropertyValue::Booleans(items))
                        }
                        PropertyType::StringArray => {
                            let mut items = match current {
                                Some(PropertyValue::Strings(items)) => items.clone(),
                                _ => Vec::new(),
                            };
                            string_array(ui, &mut items).then_some(PropertyValue::Strings(items))
                        }
                        PropertyType::NodeArray => {
                            let mut items = match current {
                                Some(PropertyValue::Nodes(items)) => items.clone(),
                                _ => Vec::new(),
                            };
                            node_array(ui, &mut items, names).then_some(PropertyValue::Nodes(items))
                        }
                        PropertyType::PrefabArray => {
                            let mut items = match current {
                                Some(PropertyValue::Prefabs(items)) => items.clone(),
                                _ => Vec::new(),
                            };
                            asset_array(ui, ("prefabs", name), prefab_assets, &mut items, root)
                                .then_some(PropertyValue::Prefabs(items))
                        }
                        PropertyType::AudioClipArray => {
                            let mut items = match current {
                                Some(PropertyValue::AudioClips(items)) => items.clone(),
                                _ => Vec::new(),
                            };
                            asset_array(ui, ("audio", name), audio_assets, &mut items, root)
                                .then_some(PropertyValue::AudioClips(items))
                        }
                        PropertyType::SpriteFrameArray => {
                            let mut items = match current {
                                Some(PropertyValue::SpriteFrames(items)) => items.clone(),
                                _ => Vec::new(),
                            };
                            asset_array(ui, ("frames", name), frame_assets, &mut items, root)
                                .then_some(PropertyValue::SpriteFrames(items))
                        }
                    };
                    if let Some(next) = next {
                        values.insert(name.clone(), next);
                        changed = true;
                    }
                });
            });
        });
    }
    changed
}

fn asset_array(
    ui: &mut Ui,
    id: (&str, &String),
    options: &[PathBuf],
    items: &mut Vec<PathBuf>,
    root: &Path,
) -> bool {
    let mut changed = false;
    ui.vertical(|ui| {
        let mut remove = None;
        for (index, item) in items.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(item.display().to_string());
                if ui.small_button("-").clicked() {
                    remove = Some(index);
                }
            });
        }
        if let Some(index) = remove {
            items.remove(index);
            changed = true;
        }
        if items.len() >= 128 {
            return;
        }
        let mut picked = None;
        egui::ComboBox::from_id_salt(id)
            .selected_text("+")
            .show_ui(ui, |ui| {
                for option in options {
                    let relative = option.strip_prefix(root).unwrap_or(option);
                    if items.iter().any(|item| item == relative) {
                        continue;
                    }
                    if ui
                        .selectable_label(false, relative.display().to_string())
                        .clicked()
                    {
                        picked = Some(relative.to_path_buf());
                    }
                }
            });
        if let Some(path) = picked {
            items.push(path);
            changed = true;
        }
    });
    changed
}

/// Picks a scene-registered asset path. Outer `None` means unchanged,
/// `Some(None)` clears the value and `Some(Some(path))` selects it.
fn asset_picker(
    ui: &mut Ui,
    id: (&str, &String),
    options: &[PathBuf],
    current: Option<&Path>,
    root: &Path,
) -> Option<Option<PathBuf>> {
    let current = current.map(|path| path.strip_prefix(root).unwrap_or(path));
    let caption = current.map_or_else(
        || strings::EMPTY_REFERENCE.to_owned(),
        |path| path.display().to_string(),
    );
    let mut picked = None;
    egui::ComboBox::from_id_salt(id)
        .selected_text(caption)
        .show_ui(ui, |ui| {
            for option in options {
                let relative = option.strip_prefix(root).unwrap_or(option);
                if ui
                    .selectable_label(current == Some(relative), relative.display().to_string())
                    .clicked()
                {
                    picked = Some(relative.to_path_buf());
                }
            }
        });
    if picked.is_some() {
        return picked.map(Some);
    }
    ui.small_button(strings::CLEAR).clicked().then_some(None)
}

fn number_array(ui: &mut Ui, items: &mut Vec<f64>) -> bool {
    let mut changed = false;
    ui.vertical(|ui| {
        let mut remove = None;
        for (index, value) in items.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                changed |= ui
                    .add(DragValue::new(value).speed(theme::VALUE_SPEED))
                    .changed();
                if ui.small_button("-").clicked() {
                    remove = Some(index);
                }
            });
        }
        if let Some(index) = remove {
            items.remove(index);
            changed = true;
        }
        if ui.small_button("+").clicked() {
            items.push(0.0);
            changed = true;
        }
    });
    changed
}

fn boolean_array(ui: &mut Ui, items: &mut Vec<bool>) -> bool {
    let mut changed = false;
    ui.vertical(|ui| {
        let mut remove = None;
        for (index, value) in items.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                changed |= ui.checkbox(value, "").changed();
                if ui.small_button("-").clicked() {
                    remove = Some(index);
                }
            });
        }
        if let Some(index) = remove {
            items.remove(index);
            changed = true;
        }
        if ui.small_button("+").clicked() {
            items.push(false);
            changed = true;
        }
    });
    changed
}

fn string_array(ui: &mut Ui, items: &mut Vec<String>) -> bool {
    let mut changed = false;
    ui.vertical(|ui| {
        let mut remove = None;
        for (index, value) in items.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                changed |= ui.text_edit_singleline(value).changed();
                if ui.small_button("-").clicked() {
                    remove = Some(index);
                }
            });
        }
        if let Some(index) = remove {
            items.remove(index);
            changed = true;
        }
        if ui.small_button("+").clicked() {
            items.push(String::new());
            changed = true;
        }
    });
    changed
}

fn node_array(ui: &mut Ui, items: &mut Vec<usize>, names: &[String]) -> bool {
    let mut changed = false;
    ui.vertical(|ui| {
        let mut remove = None;
        for (index, value) in items.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(
                    names
                        .get(*value)
                        .map_or(strings::EMPTY_REFERENCE, String::as_str),
                );
                if ui.small_button("-").clicked() {
                    remove = Some(index);
                }
            });
        }
        if let Some(index) = remove {
            items.remove(index);
            changed = true;
        }
        let response = ui.button("+ node");
        if let Some(payload) = response.dnd_hover_payload::<Drag>() {
            let valid = matches!(payload.as_ref(), Drag::Node(_));
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
        if let Some(payload) = response.dnd_release_payload::<Drag>()
            && let Drag::Node(index) = payload.as_ref()
        {
            items.push(*index);
            changed = true;
        }
    });
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
    fn asset_property_values_match_only_their_kind() {
        assert!(value_matches(
            PropertyType::AudioClip,
            &PropertyValue::AudioClip("audio/confirm.wav".into())
        ));
        assert!(value_matches(
            PropertyType::SpriteFrame,
            &PropertyValue::SpriteFrame("art/hero.png".into())
        ));
        assert!(!value_matches(
            PropertyType::AudioClip,
            &PropertyValue::SpriteFrame("art/hero.png".into())
        ));
        assert!(!value_matches(
            PropertyType::SpriteFrame,
            &PropertyValue::Prefab("Marker.prefab".into())
        ));
        assert!(value_matches(
            PropertyType::PrefabArray,
            &PropertyValue::Prefabs(vec!["Marker.prefab".into()])
        ));
        assert!(value_matches(
            PropertyType::AudioClipArray,
            &PropertyValue::AudioClips(vec!["audio/confirm.wav".into()])
        ));
        assert!(value_matches(
            PropertyType::SpriteFrameArray,
            &PropertyValue::SpriteFrames(vec!["art/hero.png".into()])
        ));
        assert!(!value_matches(
            PropertyType::AudioClipArray,
            &PropertyValue::AudioClip("audio/confirm.wav".into())
        ));
        assert!(value_matches(
            PropertyType::Vector3,
            &PropertyValue::Vector3([1.0, 2.0, 3.0])
        ));
        assert!(value_matches(
            PropertyType::Size,
            &PropertyValue::Size([640.0, 360.0])
        ));
        assert!(value_matches(
            PropertyType::Rect,
            &PropertyValue::Rect([1.0, 2.0, 3.0, 4.0])
        ));
        assert!(value_matches(
            PropertyType::Bitmask,
            &PropertyValue::Bitmask(0x8000_0001)
        ));
        assert!(!value_matches(
            PropertyType::Vector3,
            &PropertyValue::Vector2([1.0, 2.0])
        ));
    }

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
        assert!(
            reference(
                PropertyType::AudioClipArray,
                &Drag::Asset("audio/confirm.wav".into())
            )
            .is_some()
        );
        assert!(
            reference(
                PropertyType::AudioClipArray,
                &Drag::Asset("art/hero.png".into())
            )
            .is_none()
        );
        assert!(
            reference(
                PropertyType::SpriteFrameArray,
                &Drag::Asset("art/hero.png".into())
            )
            .is_some()
        );
    }

    #[test]
    fn text_node_references_reject_non_text_nodes() {
        let text = PropertyValue::Node(1);
        let sprite = PropertyValue::Node(0);
        let node_is_text = [false, true];

        let node_is_image = [false, false];
        assert!(node_filter_matches(
            Some(NodeFilter::Text),
            &text,
            &node_is_text,
            &node_is_image
        ));
        assert!(!node_filter_matches(
            Some(NodeFilter::Text),
            &sprite,
            &node_is_text,
            &node_is_image
        ));
        assert!(node_filter_matches(
            Some(NodeFilter::Image),
            &sprite,
            &node_is_text,
            &[true, false]
        ));
        assert!(node_filter_matches(
            None,
            &sprite,
            &node_is_text,
            &node_is_image
        ));
    }
}
