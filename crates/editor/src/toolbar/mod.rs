//! Menus, project controls and Play session commands.

use egui::Ui;

use crate::{
    Editor,
    docks::{self, Dock},
    icons, strings, theme,
};

pub(super) fn menus(ui: &mut Ui, editor: &mut Editor) {
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button(strings::FILE, |ui| {
            if ui.button(strings::NEW_SCENE).clicked() {
                editor.begin_new_asset(crate::assets_browser::AssetKind::Scene);
                ui.close();
            }
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
        if ui.button(strings::PROJECTS).clicked() {
            editor.open_dialog = true;
        }
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
                    editor.project.add_to_selection(node);
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
            if ui
                .add_enabled(
                    !editor.project.prefab,
                    egui::Button::new(strings::DISPLAY_SETTINGS),
                )
                .clicked()
            {
                editor.display_dialog = true;
                ui.close();
            }
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
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let running = editor.preview.is_playing();
            if icons::action(ui, icons::Icon::Stop, strings::STOP_SHORTCUT, None, running).clicked()
            {
                editor.stop_play();
            }
            let (icon, tip) = if editor.preview.paused {
                (icons::Icon::Play, strings::RESUME)
            } else {
                (icons::Icon::Pause, strings::PAUSE)
            };
            if icons::action(ui, icon, tip, None, running).clicked() {
                editor.preview.pause();
            }
            if icons::action(
                ui,
                icons::Icon::Play,
                strings::PLAY_SHORTCUT,
                Some(theme::PRIMARY),
                !running,
            )
            .clicked()
            {
                editor.play();
            }
            ui.separator();
        });
    });
}

pub(super) fn status(ui: &mut Ui, editor: &Editor) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.label(egui::RichText::new(env!("CARGO_PKG_VERSION")).weak());
        if editor.error_count != 0 {
            ui.colored_label(
                theme::REJECT,
                format!("{} {}", editor.error_count, strings::ERRORS),
            );
        }
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
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
            ui.add(egui::Label::new(format!("{}{dirty}", path.display())).truncate());
        });
    });
}
