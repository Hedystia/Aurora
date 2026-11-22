//! Searchable hierarchy and tree drag operations.

use std::path::PathBuf;

use egui::{Key, Sense, Ui};

use crate::{Editor, strings};

#[derive(Clone)]
pub(super) enum Drag {
    Node(usize),
    Asset(PathBuf),
    New(&'static str),
}

pub(super) fn show(ui: &mut Ui, editor: &mut Editor) {
    ui.horizontal(|ui| {
        if ui
            .button(strings::ADD)
            .on_hover_text(strings::CREATE_NODE)
            .clicked()
        {
            editor.create_node = true;
        }
        ui.add(
            egui::TextEdit::singleline(&mut editor.tree_search).hint_text(strings::SEARCH_NODES),
        );
    });
    let query = editor.tree_search.to_lowercase();
    let mut depths = Vec::new();
    let mut visible = Vec::new();
    for (index, node) in editor.project.document.nodes.iter().enumerate() {
        depths.push(node.parent.map_or(0, |parent| depths[parent] + 1));
        if node.name.to_lowercase().contains(&query) {
            visible.push(index);
        }
    }
    egui::ScrollArea::vertical().show(ui, |ui| {
        let revision = editor.project.revision;
        for index in visible {
            ui.push_id(index, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space((depths[index].min(16) as f32) * crate::theme::INDENT);
                    if editor.rename.as_ref().is_some_and(|(id, _)| *id == index) {
                        if let Some((_, name)) = &mut editor.rename {
                            let edit = ui.text_edit_singleline(name);
                            if editor.focus_rename {
                                edit.request_focus();
                                editor.focus_rename = false;
                            }
                            if ui.input(|input| input.key_pressed(Key::Enter)) {
                                let name = name.trim().to_owned();
                                if !name.is_empty() {
                                    editor.project.document.nodes[index].name = name;
                                    editor.project.changed();
                                }
                                editor.rename = None;
                            } else if ui.input(|input| input.key_pressed(Key::Escape)) {
                                editor.rename = None;
                            }
                        }
                        return;
                    }
                    let node = &editor.project.document.nodes[index];
                    let icon = if node.sprite.is_some() {
                        strings::SPRITE_ICON
                    } else {
                        strings::NODE_ICON
                    };
                    let response = ui.add(
                        egui::Button::selectable(
                            editor.project.selected == Some(index),
                            format!("{icon} {}", node.name),
                        )
                        .sense(Sense::click_and_drag()),
                    );
                    if response.clicked() {
                        editor.project.selected = Some(index);
                    }
                    if response.double_clicked() {
                        editor.begin_rename(index);
                    }
                    response.dnd_set_drag_payload(Drag::Node(index));
                    if let Some(payload) = response.dnd_release_payload::<Drag>() {
                        editor.drop_into(payload.as_ref(), Some(index));
                    }
                    response.context_menu(|ui| {
                        if ui.button(strings::CREATE_NODE).clicked() {
                            editor.project.selected = Some(index);
                            editor.create_node = true;
                            ui.close();
                        }
                        if ui.button(strings::RENAME).clicked() {
                            editor.begin_rename(index);
                            ui.close();
                        }
                        if ui.button(strings::DUPLICATE).clicked() {
                            let result = editor.project.duplicate(index);
                            editor.report(result);
                            ui.close();
                        }
                        if ui.button(strings::REMOVE).clicked() {
                            let result = editor.project.remove(index);
                            editor.report(result);
                            ui.close();
                        }
                        if ui.button(strings::SAVE_PREFAB).clicked() {
                            let result = editor.save_prefab(index);
                            editor.report(result);
                            ui.close();
                        }
                    });
                });
            });
            if editor.project.revision != revision {
                break;
            }
        }
        let mut remove = None;
        for (index, instance) in editor.project.document.instances.iter().enumerate() {
            let name = instance
                .prefab
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy();
            let response = ui.label(format!("◇ {name}"));
            response.context_menu(|ui| {
                if ui.button(strings::OPEN).clicked() {
                    editor.pending_open = Some(editor.project.root.join(&instance.prefab));
                    ui.close();
                }
                let inline = editor.project.document.nodes.len();
                let has_instance_tracks = editor
                    .project
                    .document
                    .animations
                    .iter()
                    .any(|clip| clip.tracks.iter().any(|track| track.node >= inline));
                let has_instance_bindings =
                    editor
                        .project
                        .document
                        .gameplay
                        .as_ref()
                        .is_some_and(|game| {
                            game.sensors.iter().any(|sensor| sensor.node >= inline)
                                || game.frames.iter().any(|frame| frame.node >= inline)
                        });
                if ui
                    .add_enabled(
                        !has_instance_tracks && !has_instance_bindings,
                        egui::Button::new(strings::REMOVE),
                    )
                    .on_disabled_hover_text(strings::INSTANCE_TRACKS)
                    .clicked()
                {
                    remove = Some(index);
                    ui.close();
                }
            });
        }
        if let Some(index) = remove {
            editor.project.document.instances.remove(index);
            editor.project.changed();
        }
        let root = ui.label(strings::DROP_AT_ROOT);
        if let Some(payload) = root.dnd_release_payload::<Drag>() {
            editor.drop_into(payload.as_ref(), None);
        }
    });
}
