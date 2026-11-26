//! Folder and scene picker shared by project dialogs.

use std::{
    fs,
    path::{Path, PathBuf},
};

use egui::Context;

use crate::{project, strings, theme};

pub(crate) struct Browser {
    directory: PathBuf,
    address: String,
    files: bool,
    error: Option<String>,
    entries: Vec<(PathBuf, bool)>,
}

impl Browser {
    pub(crate) fn new(directory: PathBuf, files: bool) -> Self {
        let mut browser = Self {
            address: String::new(),
            directory: PathBuf::new(),
            files,
            error: None,
            entries: Vec::new(),
        };
        let location = directory
            .ancestors()
            .find(|path| path.is_dir())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        browser.visit(location);
        browser
    }

    fn visit(&mut self, directory: PathBuf) {
        self.address = directory.display().to_string();
        self.directory = directory;
        self.entries.clear();
        match fs::read_dir(&self.directory) {
            Ok(entries) => {
                self.error = None;
                for entry in entries.flatten() {
                    let path = entry.path();
                    let dir = path.is_dir();
                    if dir || (self.files && project::is_scene(&path)) {
                        self.entries.push((path, dir));
                    }
                }
                self.entries
                    .sort_by(|(a, a_dir), (b, b_dir)| b_dir.cmp(a_dir).then_with(|| a.cmp(b)));
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    pub(crate) fn show(&mut self, ctx: &Context) -> Option<Option<PathBuf>> {
        let mut choice = None;
        let mut navigate = None;
        let modal = super::project_modal("project-browser").show(ctx, |ui| {
            ui.set_width(theme::PROJECT_DIALOG_WIDTH);
            ui.heading(if self.files {
                strings::BROWSE_PROJECT
            } else {
                strings::BROWSE_FOLDER
            });
            ui.horizontal(|ui| {
                if ui.button(strings::PARENT_FOLDER).clicked() {
                    navigate = self.directory.parent().map(Path::to_path_buf);
                }
                let address = ui.add(
                    egui::TextEdit::singleline(&mut self.address)
                        .desired_width(theme::PROJECT_ADDRESS_WIDTH),
                );
                if ui.button(strings::GO).clicked()
                    || (address.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter)))
                {
                    let path = PathBuf::from(self.address.trim());
                    navigate = Some(if path.is_absolute() {
                        path
                    } else {
                        self.directory.join(path)
                    });
                }
            });
            if let Some(error) = &self.error {
                ui.colored_label(theme::REJECT, error);
            }
            egui::ScrollArea::vertical()
                .id_salt(&self.directory)
                .max_height(theme::PROJECT_BROWSER_HEIGHT)
                .show(ui, |ui| {
                    for (path, directory) in &self.entries {
                        let name = path
                            .file_name()
                            .unwrap_or(path.as_os_str())
                            .to_string_lossy();
                        if *directory {
                            if ui.button(format!("{name}/")).clicked() {
                                navigate = Some(path.clone());
                            }
                        } else if ui.button(name).clicked() {
                            choice = Some(Some(path.clone()));
                        }
                    }
                });
            ui.separator();
            ui.horizontal(|ui| {
                let selectable = self.directory.is_dir()
                    && (!self.files || project::main_scene(&self.directory).is_file());
                if ui
                    .add_enabled(selectable, egui::Button::new(strings::USE_CURRENT_FOLDER))
                    .clicked()
                {
                    choice = Some(Some(self.directory.clone()));
                }
                if ui.button(strings::CANCEL).clicked() {
                    choice = Some(None);
                }
            });
        });
        if let Some(directory) = navigate {
            self.visit(directory);
        }
        if modal.should_close() {
            Some(None)
        } else {
            choice
        }
    }
}
