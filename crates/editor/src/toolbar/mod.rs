//! Menus, project controls and Play session commands.

use egui::Ui;

use crate::{
    Editor,
    docks::{self, Dock},
    strings, theme,
};

pub(super) fn menus(ui: &mut Ui, editor: &mut Editor) {
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button(strings::FILE, |ui| {
            if ui.button(strings::OPEN_PROJECT).clicked() {
                editor.open_dialog = true;
                ui.close();
            }
            if ui.button(strings::SAVE).clicked() {
                let result = editor.project.save();
                editor.report(result);
                ui.close();
            }
        });
        ui.menu_button(strings::EDIT, |ui| {
            if let Some(index) = editor.project.selected {
                if ui.button(strings::RENAME).clicked() {
                    editor.begin_rename(index);
                    ui.close();
                }
                if ui.button(strings::DUPLICATE).clicked() {
                    let result = editor.project.duplicate(index);
                    editor.report(result);
                    ui.close();
                }
            }
        });
        ui.menu_button(strings::NODE_PRESETS, |ui| {
            for node in [strings::EMPTY_NODE, strings::SPRITE_NODE] {
                if ui.button(node).clicked() {
                    editor.project.add(node, editor.project.selected);
                    ui.close();
                }
            }
        });
        ui.menu_button(strings::COMPONENT, |ui| {
            if ui
                .add_enabled(
                    editor.project.selected.is_some(),
                    egui::Button::new(strings::ADD_COMPONENT),
                )
                .clicked()
            {
                editor.component_picker = true;
                ui.close();
            }
        });
        ui.menu_button(strings::PROJECT, |ui| {
            if ui.button(strings::EXPLORER).clicked() {
                let result = crate::open_external(&editor.project.root);
                editor.report(result);
                ui.close();
            }
        });
        ui.menu_button(strings::PANEL, |ui| {
            for tab in Dock::ALL {
                let found = editor.layout.find_tab(&tab);
                let mut visible = found.is_some();
                if ui.checkbox(&mut visible, tab.label()).changed() {
                    if let Some(path) = found {
                        editor.layout.remove_tab(path);
                    } else {
                        editor.layout.push_to_focused_leaf(tab);
                    }
                }
            }
        });
        ui.menu_button(strings::LAYOUT, |ui| {
            if ui.button(strings::RESET_LAYOUT).clicked() {
                editor.layout = docks::initial_layout();
                ui.close();
            }
            if ui.button(strings::SAVE_LAYOUT).clicked() {
                let result = editor.save_layout();
                editor.report(result);
                ui.close();
            }
        });
        ui.menu_button(strings::PACKAGES, |ui| {
            ui.label(strings::NO_PACKAGES);
        });
        ui.menu_button(strings::DEVELOPER, |ui| {
            if ui.button(strings::REFRESH_ASSETS).clicked() {
                editor.refresh_assets();
                ui.close();
            }
        });
        ui.menu_button(strings::HELP, |ui| {
            ui.label(strings::SHORTCUT_HELP);
        });
    });
}

pub(super) fn show(ui: &mut Ui, editor: &mut Editor) {
    ui.horizontal_wrapped(|ui| {
        ui.menu_button(strings::BROWSER, |ui| {
            if ui.button(strings::RESET_LAYOUT).clicked() {
                editor.layout = docks::initial_layout();
                ui.close();
            }
            if ui.button(strings::SAVE_LAYOUT).clicked() {
                let result = editor.save_layout();
                editor.report(result);
                ui.close();
            }
        });
        if ui.button(strings::PLAY).clicked() {
            editor.play();
        }
        if ui
            .add_enabled(
                editor.preview.is_playing(),
                egui::Button::new(if editor.preview.paused {
                    strings::RESUME
                } else {
                    strings::PAUSE
                }),
            )
            .clicked()
        {
            editor.preview.pause();
        }
        if ui
            .add_enabled(
                editor.preview.is_playing(),
                egui::Button::new(strings::STOP),
            )
            .clicked()
        {
            editor.preview.stop();
            editor.preview.game_view = false;
        }
        if ui.button(strings::OPEN_PROJECT).clicked() {
            editor.open_dialog = true;
        }
        if ui.button(strings::OPEN_APP).clicked() {
            let result = crate::open_external(&editor.project.root);
            editor.report(result);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.colored_label(theme::ACCEPT, env!("CARGO_PKG_VERSION"));
        });
    });
}

pub(super) fn status(ui: &mut Ui, editor: &Editor) {
    ui.horizontal(|ui| {
        let asset = editor
            .selected_asset
            .as_ref()
            .unwrap_or(&editor.project.path);
        let path = editor.project.relative(asset);
        let dirty = if editor.project.dirty {
            strings::DIRTY
        } else {
            ""
        };
        ui.label(format!("{}{}{}", strings::DATABASE, path.display(), dirty));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(env!("CARGO_PKG_VERSION"));
            if editor.error_count != 0 {
                ui.colored_label(
                    theme::REJECT,
                    format!("{} {}", editor.error_count, strings::ERRORS),
                );
            }
        });
    });
}
