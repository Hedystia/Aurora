//! Player log display and source-file navigation.

use std::path::Path;

use egui::Ui;

use crate::{Editor, strings, theme};

pub(super) fn show(ui: &mut Ui, editor: &mut Editor) {
    if ui.button(strings::CLEAR_LOGS).clicked() {
        editor.logs.clear();
        editor.error_count = 0;
    }
    let mut open = None;
    egui::ScrollArea::vertical()
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for entry in &editor.logs {
                let lower = entry.to_lowercase();
                let error = lower.contains("error") || lower.contains("failed");
                let color = if error {
                    theme::REJECT
                } else if lower.contains("warning") {
                    theme::WARNING
                } else {
                    theme::TEXT
                };
                let message = egui::RichText::new(entry).color(color).monospace();
                if error {
                    if ui
                        .add(egui::Label::new(message).sense(egui::Sense::click()))
                        .clicked()
                    {
                        open = entry
                            .split_whitespace()
                            .find_map(|token| {
                                let (path, _) = token.split_once(".ts")?;
                                let path = format!("{}.ts", path.trim_start_matches(['(', '"']));
                                let path = editor.project.root.join(path);
                                path.is_file().then_some(path)
                            })
                            .or_else(|| {
                                editor.project.document.nodes.iter().find_map(|node| {
                                    node.scripts
                                        .first()
                                        .map(|script| editor.project.root.join(&script.path))
                                })
                            });
                    }
                } else {
                    ui.label(message);
                }
            }
        });
    if let Some(path) = &open {
        let result = crate::open_external(Path::new(path));
        editor.report(result);
    }
}
