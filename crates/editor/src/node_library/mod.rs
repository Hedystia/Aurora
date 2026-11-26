//! Built-in nodes and project prefab tiles.

use egui::{Rect, Sense, Ui, pos2, vec2};

use crate::{Editor, icons, node_tree::Drag, project::is_prefab, strings, theme};

pub(super) fn show(ui: &mut Ui, editor: &mut Editor) {
    ui.horizontal(|ui| {
        ui.selectable_value(&mut editor.custom_nodes, false, strings::BUILTIN);
        ui.selectable_value(&mut editor.custom_nodes, true, strings::CUSTOM);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(
                egui::Label::new(egui::RichText::new(strings::DRAG_HINT).color(theme::TEXT))
                    .truncate(),
            );
        });
    });
    ui.separator();
    let content_width = ui.available_width();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_viewport(ui, |ui, _| {
            ui.set_width(content_width);
            if editor.custom_nodes {
                let prefabs: Vec<_> = editor
                    .asset_paths
                    .iter()
                    .filter(|path| is_prefab(path))
                    .cloned()
                    .collect();
                ui.horizontal_wrapped(|ui| {
                    for path in &prefabs {
                        let name = path
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned();
                        let response = tile(ui, icons::Icon::Prefab, &name, theme::ANIMATION);
                        response.dnd_set_drag_payload(Drag::Asset(path.clone()));
                        if response.clicked() {
                            editor.drop_into(&Drag::Asset(path.clone()), None);
                        }
                    }
                    if prefabs.is_empty() {
                        ui.label(strings::NO_PREFABS);
                    }
                });
            } else {
                let tiles: Vec<_> = [
                    (strings::EMPTY_NODE, icons::Icon::Node, theme::TEXT),
                    (strings::SPRITE_NODE, icons::Icon::Sprite, theme::ACCENT),
                ]
                .into_iter()
                .chain(
                    crate::inspector::spatial::PRESETS
                        .map(|(name, _)| (name, icons::Icon::Node, theme::ANIMATION)),
                )
                .chain(crate::inspector::visual::NAMES.map(|name| {
                    let icon = if matches!(name, "Text" | "Rich Text") {
                        icons::Icon::Text
                    } else {
                        icons::Icon::Sprite
                    };
                    (name, icon, theme::ACCENT)
                }))
                .collect();
                ui.horizontal_wrapped(|ui| {
                    for (name, icon, tint) in tiles {
                        let response = tile(ui, icon, name, tint);
                        response.dnd_set_drag_payload(Drag::New(name));
                        if response.clicked() {
                            let index = editor.project.add_to_selection(name);
                            crate::viewport::place_new(editor, index);
                        }
                    }
                });
            }
        });
}

fn tile(ui: &mut Ui, icon: icons::Icon, name: &str, tint: egui::Color32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        vec2(theme::TILE + 12.0, theme::TILE),
        Sense::click_and_drag(),
    );
    icons::row_background(ui, rect, false, response.hovered());
    let icon_rect =
        Rect::from_center_size(pos2(rect.center().x, rect.top() + 27.0), vec2(32.0, 32.0));
    icons::paint(ui.painter(), icon_rect, icon, tint);
    let label_rect = Rect::from_min_max(
        pos2(rect.left() + 3.0, rect.bottom() - 24.0),
        pos2(rect.right() - 3.0, rect.bottom() - 2.0),
    );
    ui.painter().with_clip_rect(label_rect).text(
        label_rect.center_bottom(),
        egui::Align2::CENTER_BOTTOM,
        name,
        egui::FontId::proportional(12.0),
        theme::TEXT,
    );
    response
}
