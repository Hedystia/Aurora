//! Searchable Godot-style scene hierarchy and tree drag operations.

use std::path::PathBuf;

use egui::{Key, Rect, Sense, StrokeKind, Ui, pos2, vec2};

use crate::{Editor, icons, strings, theme};

#[derive(Clone)]
pub(super) enum Drag {
    Node(usize),
    Asset(PathBuf),
    New(&'static str),
}

pub(super) fn show(ui: &mut Ui, editor: &mut Editor) {
    ui.horizontal(|ui| {
        if icons::button(ui, icons::Icon::Plus, strings::CREATE_NODE).clicked() {
            editor.create_node = true;
        }
        let search = egui::TextEdit::singleline(&mut editor.tree_search)
            .hint_text(strings::SEARCH_NODES)
            .margin(egui::Margin::symmetric(6, 3));
        ui.add_sized([ui.available_width(), theme::ROW_HEIGHT], search);
    });
    if !editor.project.group_selection.is_empty() {
        ui.horizontal(|ui| {
            if ui
                .button(format!(
                    "Group {} nodes",
                    editor.project.group_selection.len()
                ))
                .clicked()
            {
                let result = editor.project.group_siblings();
                editor.report(result);
            }
            if ui.small_button("Clear selection").clicked() {
                editor.project.group_selection.clear();
            }
        });
    }
    let query = editor.tree_search.to_lowercase();
    let mut depths = Vec::new();
    let mut visible = Vec::new();
    for (index, node) in editor.project.document.nodes.iter().enumerate() {
        depths.push(node.parent.map_or(0, |parent| depths[parent] + 1));
        if node.name.to_lowercase().contains(&query) {
            visible.push(index);
        }
    }
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let revision = editor.project.revision;
            for (position, &index) in visible.iter().enumerate() {
                if editor.rename.as_ref().is_some_and(|(id, _)| *id == index) {
                    rename_row(ui, editor, index, depths[index]);
                } else {
                    row(ui, editor, index, depths[index], position, &visible);
                }
                if editor.project.revision != revision {
                    break;
                }
            }
            instances(ui, editor);
            root_drop(ui, editor);
        });
}

fn row(
    ui: &mut Ui,
    editor: &mut Editor,
    index: usize,
    depth: usize,
    position: usize,
    visible: &[usize],
) {
    let node = &editor.project.document.nodes[index];
    let name = node.name.clone();
    let sprite = node.sprite.is_some();
    let scripted = !node.scripts.is_empty();
    let active = node.appearance.unwrap_or_default().active;
    let selected = editor.project.selected == Some(index);
    let id = ui.id().with(("node-row", index));
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), theme::ROW_HEIGHT),
        Sense::click_and_drag(),
    );
    let painter = ui.painter().clone();
    icons::row_background(
        ui,
        rect,
        selected || editor.project.group_selection.contains(&index),
        response.hovered(),
    );
    if selected {
        painter.rect_filled(
            Rect::from_min_size(rect.min, vec2(2.0, rect.height())),
            0.0,
            theme::ACCENT,
        );
    }
    for guide in 0..depth {
        let x = rect.left() + (guide as f32 + 1.0) * theme::INDENT - 6.0;
        painter.line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            (theme::LINE, theme::GRID),
        );
    }
    if let Some(payload) = response.dnd_hover_payload::<Drag>() {
        let valid = editor.drop_allowed(payload.as_ref(), Some(index));
        let tint = if valid { theme::ACCEPT } else { theme::REJECT };
        painter.rect_filled(rect, theme::ROUND, tint.gamma_multiply(0.10));
        painter.rect_stroke(
            rect,
            theme::ROUND,
            (theme::OUTLINE, tint),
            StrokeKind::Inside,
        );
    }
    let icon_rect = Rect::from_center_size(
        pos2(
            rect.left() + (depth as f32 + 1.0) * theme::INDENT + theme::ICON / 2.0,
            rect.center().y,
        ),
        vec2(theme::ICON, theme::ICON),
    );
    let icon_color = if !active {
        theme::TEXT.gamma_multiply(0.55)
    } else if sprite {
        theme::ACCENT
    } else {
        theme::TEXT
    };
    icons::paint(&painter, icon_rect, icons::node(sprite), icon_color);

    let eye_rect = Rect::from_center_size(
        pos2(rect.right() - theme::ROW_HEIGHT / 2.0, rect.center().y),
        vec2(16.0, 16.0),
    );
    let eye = ui.interact(eye_rect, id.with("visible"), Sense::click());
    let eye_color = if eye.hovered() {
        theme::FOREGROUND
    } else if active {
        theme::TEXT
    } else {
        theme::TEXT.gamma_multiply(0.45)
    };
    icons::paint(
        &painter,
        eye_rect.shrink(1.0),
        if active {
            icons::Icon::Eye
        } else {
            icons::Icon::EyeOff
        },
        eye_color,
    );
    let script_left = if scripted {
        let script_rect = Rect::from_center_size(
            pos2(eye_rect.left() - 8.0, rect.center().y),
            vec2(12.0, 12.0),
        );
        icons::paint(&painter, script_rect, icons::Icon::Script, theme::WARNING);
        script_rect.left()
    } else {
        eye_rect.left()
    };
    let text_rect = Rect::from_min_max(
        pos2(icon_rect.right() + 6.0, rect.top() + 3.0),
        pos2(script_left - 4.0, rect.bottom() - 3.0),
    );
    ui.put(
        text_rect,
        egui::Label::new(egui::RichText::new(&name).color(if active {
            theme::FOREGROUND
        } else {
            theme::TEXT
        }))
        .truncate()
        .selectable(false),
    );

    if editor.focus_tree && selected {
        response.request_focus();
        response.scroll_to_me(None);
        editor.focus_tree = false;
    }
    if let Some(target) = response
        .has_focus()
        .then(|| move_focus(ui, position, visible))
        .flatten()
    {
        editor.project.selected = Some(target);
        editor.focus_tree = true;
        ui.ctx().request_repaint();
    } else if response.has_focus() {
        ui.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                response.id,
                egui::EventFilter {
                    vertical_arrows: true,
                    ..Default::default()
                },
            );
        });
    }
    response.dnd_set_drag_payload(Drag::Node(index));
    if let Some(payload) = response.dnd_release_payload::<Drag>() {
        editor.drop_into(payload.as_ref(), Some(index));
    }
    if eye.clicked() {
        toggle_active(editor, index);
    } else if response.clicked() {
        if ui.input(|input| input.modifiers.command) {
            if editor.project.group_selection.is_empty() {
                editor
                    .project
                    .group_selection
                    .extend(editor.project.selected);
            }
            if !editor.project.group_selection.insert(index) {
                editor.project.group_selection.remove(&index);
            }
        } else {
            editor.project.group_selection.clear();
        }
        editor.project.selected = Some(index);
    }
    if response.double_clicked() {
        editor.begin_rename(index);
    }
    response.context_menu(|ui| {
        if editor.project.document.nodes[index]
            .spatial
            .as_ref()
            .is_some_and(|spatial| spatial.role == engine_core::scene::spatial::Role::Camera)
            && ui.button(strings::CAMERA_VIEW).clicked()
        {
            editor.viewport.look_through_camera(index);
            ui.close();
        }
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
        if editor.project.document.nodes[index].parent.is_some()
            && ui.button(strings::MOVE_TO_ROOT).clicked()
        {
            let result = editor.project.reparent(index, None);
            editor.report(result);
            ui.close();
        }
        if ui.button(strings::SAVE_PREFAB).clicked() {
            let result = editor.save_prefab(index);
            editor.report(result);
            ui.close();
        }
        ui.separator();
        if ui.button(strings::REMOVE).clicked() {
            let result = editor.project.remove(index);
            editor.report(result);
            ui.close();
        }
    });
}

fn move_focus(ui: &mut Ui, position: usize, visible: &[usize]) -> Option<usize> {
    let target = ui.input_mut(|input| {
        if input.consume_key(egui::Modifiers::NONE, Key::ArrowUp) {
            position.checked_sub(1)
        } else if input.consume_key(egui::Modifiers::NONE, Key::ArrowDown) {
            Some(position + 1)
        } else {
            None
        }
    })?;
    visible.get(target).copied()
}

fn rename_row(ui: &mut Ui, editor: &mut Editor, index: usize, depth: usize) {
    ui.horizontal(|ui| {
        ui.add_space((depth.min(16) as f32 + 1.0) * theme::INDENT);
        if let Some((_, name)) = &mut editor.rename {
            let edit = ui.add_sized(
                [ui.available_width(), theme::ROW_HEIGHT],
                egui::TextEdit::singleline(name),
            );
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
    });
}

fn toggle_active(editor: &mut Editor, index: usize) {
    let node = &mut editor.project.document.nodes[index];
    let mut appearance = node.appearance.unwrap_or_default();
    appearance.active = !appearance.active;
    node.appearance = Some(appearance);
    editor.project.changed();
}

fn instances(ui: &mut Ui, editor: &mut Editor) {
    let mut remove = None;
    let items: Vec<_> = editor
        .project
        .document
        .instances
        .iter()
        .enumerate()
        .map(|(index, instance)| (index, instance.prefab.clone()))
        .collect();
    for (index, prefab) in items {
        let name = prefab
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let (rect, response) = ui.allocate_exact_size(
            vec2(ui.available_width(), theme::ROW_HEIGHT),
            Sense::click(),
        );
        icons::row_background(ui, rect, false, response.hovered());
        let icon_rect = Rect::from_center_size(
            pos2(
                rect.left() + theme::INDENT + theme::ICON / 2.0,
                rect.center().y,
            ),
            vec2(theme::ICON, theme::ICON),
        );
        icons::paint(
            ui.painter(),
            icon_rect,
            icons::Icon::Prefab,
            theme::ANIMATION,
        );
        ui.put(
            Rect::from_min_max(
                pos2(icon_rect.right() + 6.0, rect.top() + 3.0),
                pos2(rect.right() - 4.0, rect.bottom() - 3.0),
            ),
            egui::Label::new(format!("{name} (prefab)")).truncate(),
        );
        response.context_menu(|ui| {
            ui.menu_button("Contact sensors", |ui| {
                match instance_nodes(&editor.project, index) {
                    Ok((base, nodes)) => {
                        for (offset, node) in nodes.iter().enumerate() {
                            let target = base + offset;
                            let data = &mut editor.project.document.gameplay;
                            let mut enabled = data.as_ref().is_some_and(|game| {
                                game.sensors.iter().any(|sensor| sensor.node == target)
                            });
                            if ui.checkbox(&mut enabled, &node.name).changed() {
                                let game = data.get_or_insert_with(Default::default);
                                if enabled {
                                    game.sensors.push(engine_core::save::SensorFile {
                                        node: target,
                                        size: node
                                            .sprite
                                            .as_ref()
                                            .map_or([24.0; 2], |sprite| sprite.size),
                                    });
                                } else {
                                    game.sensors.retain(|sensor| sensor.node != target);
                                }
                                editor.project.changed();
                            }
                            if let Some(sensor) =
                                editor.project.document.gameplay.as_mut().and_then(|game| {
                                    game.sensors.iter_mut().find(|sensor| sensor.node == target)
                                })
                            {
                                let mut changed = false;
                                ui.horizontal(|ui| {
                                    for (side, label) in sensor.size.iter_mut().zip(["W ", "H "]) {
                                        changed |= ui
                                            .add(
                                                egui::DragValue::new(side)
                                                    .prefix(label)
                                                    .range(1.0..=f32::MAX),
                                            )
                                            .changed();
                                    }
                                });
                                if changed {
                                    editor.project.changed();
                                }
                            }
                        }
                    }
                    Err(error) => {
                        ui.label(error.to_string());
                    }
                }
            });
            if ui.button(strings::OPEN).clicked() {
                editor.pending_open = Some(editor.project.root.join(&prefab));
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
}

fn instance_nodes(
    project: &crate::project::Project,
    index: usize,
) -> anyhow::Result<(usize, Vec<engine_core::save::NodeFile>)> {
    let mut base = project.document.nodes.len();
    for (number, instance) in project.document.instances.iter().enumerate() {
        let prefab = engine_core::save::PrefabFile::load(&project.root.join(&instance.prefab))?;
        if number == index {
            return Ok((base, prefab.nodes));
        }
        base += prefab.nodes.len();
    }
    anyhow::bail!("Unknown prefab instance")
}

fn root_drop(ui: &mut Ui, editor: &mut Editor) {
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), theme::ROW_HEIGHT),
        Sense::hover(),
    );
    if !egui::DragAndDrop::has_any_payload(ui.ctx()) {
        return;
    }
    let hover = response.dnd_hover_payload::<Drag>();
    let valid = hover
        .as_ref()
        .is_some_and(|payload| editor.drop_allowed(payload.as_ref(), None));
    let stroke = if valid {
        theme::ACCEPT
    } else if hover.is_some() {
        theme::REJECT
    } else {
        theme::FRAME.gamma_multiply(0.7)
    };
    let painter = ui.painter().clone();
    painter.rect_stroke(
        rect.shrink(1.0),
        theme::ROUND,
        (theme::LINE, stroke),
        StrokeKind::Inside,
    );
    let icon_rect = Rect::from_center_size(
        pos2(
            rect.left() + theme::INDENT + theme::ICON / 2.0,
            rect.center().y,
        ),
        vec2(theme::ICON, theme::ICON),
    );
    icons::paint(&painter, icon_rect, icons::Icon::Parent, stroke);
    ui.put(
        Rect::from_min_max(
            pos2(icon_rect.right() + 6.0, rect.top() + 3.0),
            pos2(rect.right() - 4.0, rect.bottom() - 3.0),
        ),
        egui::Label::new(egui::RichText::new(strings::DROP_AT_ROOT).color(theme::TEXT)).truncate(),
    );
    if let Some(payload) = response.dnd_release_payload::<Drag>() {
        editor.drop_into(payload.as_ref(), None);
    }
}

/// Placement hint used by the viewport while an asset or node is dragged.
pub(super) fn drop_hint(payload: &Drag) -> &'static str {
    match payload {
        Drag::Node(_) => strings::MOVE_NODE,
        Drag::Asset(path) if crate::project::extension(path) == "png" => strings::PLACE_SPRITE,
        Drag::Asset(path) if crate::project::extension(path) == "ts" => strings::ATTACH_COMPONENT,
        Drag::Asset(_) => strings::PLACE_PREFAB,
        Drag::New(name) => name,
    }
}

/// Returns a short caption for the drag preview.
pub(super) fn drag_label(editor: &Editor, payload: &Drag) -> String {
    match payload {
        Drag::Node(index) => editor
            .project
            .document
            .nodes
            .get(*index)
            .map_or_else(|| strings::NODE.into(), |node| node.name.clone()),
        Drag::Asset(path) => path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        Drag::New(name) => (*name).to_owned(),
    }
}

/// Screen-space drop indicator used by the viewport is painted by `icons`.
/// Anchor used by tests to keep the hierarchy indentation stable.
#[cfg(test)]
pub(super) fn row_anchor(left: f32, depth: usize) -> f32 {
    left + (depth as f32 + 1.0) * theme::INDENT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_indents_rows_without_running_off_screen() {
        assert_eq!(row_anchor(0.0, 0), theme::INDENT);
        assert_eq!(row_anchor(0.0, 3), 4.0 * theme::INDENT);
    }
}
