//! Built-in nodes and project prefab tiles.

use egui::{Sense, Ui};

use crate::{Editor, node_tree::Drag, project::extension, strings};

pub(super) fn show(ui: &mut Ui, editor: &mut Editor) {
    ui.horizontal(|ui| {
        ui.selectable_value(&mut editor.custom_nodes, false, strings::BUILTIN);
        ui.selectable_value(&mut editor.custom_nodes, true, strings::CUSTOM);
    });
    egui::ScrollArea::vertical().show(ui, |ui| {
        if editor.custom_nodes {
            for path in editor
                .asset_paths
                .iter()
                .filter(|path| extension(path) == "aurora-prefab")
            {
                ui.add(
                    egui::Button::new(
                        path.file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .as_ref(),
                    )
                    .sense(Sense::drag()),
                )
                .dnd_set_drag_payload(Drag::Asset(path.clone()));
            }
        } else {
            ui.horizontal_wrapped(|ui| {
                for name in [strings::EMPTY_NODE, strings::SPRITE_NODE] {
                    let response = ui.add(egui::Button::new(name).sense(Sense::click_and_drag()));
                    response.dnd_set_drag_payload(Drag::New(name));
                    if response.clicked() {
                        editor.project.add(name, editor.project.selected);
                    }
                }
            });
        }
    });
}
