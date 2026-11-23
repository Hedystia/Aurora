//! Built-in nodes and project prefab tiles.

use egui::{Rect, Sense, Ui, pos2, vec2};

use crate::{Editor, icons, node_tree::Drag, project::is_prefab, strings, theme};

pub(super) fn show(ui: &mut Ui, editor: &mut Editor) {
    ui.horizontal(|ui| {
        ui.selectable_value(&mut editor.custom_nodes, false, strings::BUILTIN);
        ui.selectable_value(&mut editor.custom_nodes, true, strings::CUSTOM);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(strings::DRAG_HINT).color(theme::TEXT));
        });
    });
    ui.separator();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
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
                ui.horizontal_wrapped(|ui| {
                    for (name, icon, tint) in [
                        (strings::EMPTY_NODE, icons::Icon::Node, theme::TEXT),
                        (strings::SPRITE_NODE, icons::Icon::Sprite, theme::ACCENT),
                    ]
                    .into_iter()
                    .chain(
                        crate::inspector::spatial::PRESETS
                            .map(|(name, _)| (name, icons::Icon::Node, theme::ANIMATION)),
                    )
                    .chain(
                        crate::inspector::visual::NAMES
                            .map(|name| (name, icons::Icon::Sprite, theme::ACCENT)),
                    ) {
                        let response = tile(ui, icon, name, tint);
                        response.dnd_set_drag_payload(Drag::New(name));
                        if response.clicked() {
                            editor.project.add(name, editor.project.selected);
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
    ui.put(
        Rect::from_min_max(
            pos2(rect.left() + 3.0, rect.bottom() - 24.0),
            pos2(rect.right() - 3.0, rect.bottom() - 2.0),
        ),
        egui::Label::new(egui::RichText::new(name).color(theme::TEXT))
            .truncate()
            .selectable(false),
    );
    response
}
