//! Search, import and create project assets.

use std::{fs, io::Write, path::Path};

use anyhow::{Context, Result, ensure};
use egui::{Sense, Ui};

use crate::{
    Editor,
    node_tree::Drag,
    preview::identifier,
    project::{extension, supported},
    strings,
};

pub(super) fn show(ui: &mut Ui, editor: &mut Editor) {
    ui.horizontal(|ui| {
        ui.menu_button(strings::ADD, |ui| creation(ui, editor));
        ui.add(
            egui::TextEdit::singleline(&mut editor.asset_search).hint_text(strings::SEARCH_ASSETS),
        );
    });
    let query = editor.asset_search.to_lowercase();
    let mut tree = AssetTree::default();
    for path in &editor.asset_paths {
        let relative = editor.project.relative(path);
        if !relative.to_string_lossy().to_lowercase().contains(&query) {
            continue;
        }
        let mut pieces = relative.iter().peekable();
        let mut parent = &mut tree;
        while let Some(name) = pieces.next() {
            if path.is_dir() || pieces.peek().is_some() {
                parent = parent.folders.entry(name.to_os_string()).or_default();
            } else {
                parent.files.push(path.clone());
            }
        }
    }
    egui::ScrollArea::vertical().show(ui, |ui| {
        draw_assets(ui, editor, &tree, &mut std::path::PathBuf::new());
        if editor.asset_paths.is_empty() {
            ui.label(strings::DROP_ASSETS);
        }
    });
}

#[derive(Default)]
struct AssetTree {
    folders: std::collections::BTreeMap<std::ffi::OsString, AssetTree>,
    files: Vec<std::path::PathBuf>,
}

fn draw_assets(
    ui: &mut Ui,
    editor: &mut Editor,
    tree: &AssetTree,
    branch: &mut std::path::PathBuf,
) {
    for (name, children) in &tree.folders {
        branch.push(name);
        let header = egui::CollapsingHeader::new(name.to_string_lossy())
            .id_salt(&*branch)
            .default_open(true)
            .show(ui, |ui| draw_assets(ui, editor, children, branch));
        header
            .header_response
            .context_menu(|ui| creation(ui, editor));
        branch.pop();
    }
    for path in &tree.files {
        let caption = path.file_name().unwrap_or_default().to_string_lossy();
        let response = ui.add(
            egui::Button::selectable(editor.selected_asset.as_ref() == Some(path), caption)
                .sense(Sense::click_and_drag()),
        );
        response.dnd_set_drag_payload(Drag::Asset(path.clone()));
        if response.clicked() {
            editor.selected_asset = Some(path.clone());
        }
        if response.double_clicked() {
            editor.pending_open = Some(path.clone());
        }
        response.context_menu(|ui| {
            creation(ui, editor);
            if ui.button(strings::EXPLORER).clicked() {
                let result = crate::open_external(path.parent().unwrap_or(path));
                editor.report(result);
                ui.close();
            }
        });
    }
}

fn creation(ui: &mut Ui, editor: &mut Editor) {
    for (label, script) in [(strings::NEW_SCRIPT, true), (strings::NEW_FOLDER, false)] {
        if ui.button(label).clicked() {
            editor.new_asset = Some(script);
            editor.asset_name.clear();
            ui.close();
        }
    }
}

pub(super) fn create(root: &Path, name: &str, script: bool) -> Result<()> {
    ensure!(
        identifier(name),
        "Use a name beginning with a letter or underscore, followed by letters, digits or underscores"
    );
    let assets = root.join(if script { "assets/scripts" } else { "assets" });
    fs::create_dir_all(&assets)?;
    if script {
        let mut output = tempfile::NamedTempFile::new_in(&assets)?;
        let source = format!(
            "import {{ Component, registerComponent }} from \"aurora-cc\";\n\nexport class {name} extends Component {{\n  speed = 30;\n  target: number | undefined = undefined;\n  prefab: string | undefined = undefined;\n}}\n\nexport const metadata =\nregisterComponent([\"{name}\", {{\n  \"props\": {{\n    \"speed\": {{ \"type\": \"number\", \"tooltip\": \"Movement speed.\" }},\n    \"target\": {{ \"type\": \"node\" }},\n    \"prefab\": {{ \"type\": \"prefab\" }}\n  }}\n}}]);\n"
        );
        output.write_all(source.as_bytes())?;
        output.persist_noclobber(assets.join(format!("{name}.ts")))?;
    } else {
        fs::create_dir(assets.join(name))?;
    }
    Ok(())
}

pub(super) fn import(root: &Path, source: &Path) -> Result<()> {
    ensure!(supported(source), "Unsupported asset type");
    if extension(source) == "png" {
        engine_core::assets::Assets::new(None).import_file(source)?;
    }
    let assets = root.join("assets");
    fs::create_dir_all(&assets)?;
    let target = assets.join(source.file_name().context("Asset filename is missing")?);
    let mut temporary = tempfile::NamedTempFile::new_in(&assets)?;
    std::io::copy(&mut fs::File::open(source)?, &mut temporary)?;
    temporary.persist_noclobber(target)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_component_is_discovered_and_import_validates_before_publication() -> Result<()> {
        let project = tempfile::tempdir()?;
        create(project.path(), "Mover", true)?;
        assert!(create(project.path(), "Mover", true).is_err());
        let model = crate::project::Project::open(project.path())?;
        let script = model
            .assets()
            .into_iter()
            .find(|path| extension(path) == "ts")
            .context("Missing generated script")?;
        let metadata = codegen::extract(&fs::read_to_string(script)?)?;
        assert_eq!(
            metadata["Mover"].props["target"].kind,
            codegen::PropertyType::Node
        );
        let source = tempfile::tempdir()?;
        let invalid = source.path().join("bad.png");
        fs::write(&invalid, b"not a png")?;
        assert!(import(project.path(), &invalid).is_err());
        assert!(!project.path().join("assets/bad.png").exists());
        Ok(())
    }
}
