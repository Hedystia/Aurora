//! Godot-style file system browser, import and asset creation.

pub(crate) mod files;
mod gltf;

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use egui::{Rect, Sense, Ui, pos2, vec2};
use engine_core::save::{NodeFile, PrefabFile, SceneFile, SceneKind, VERSION};

use crate::{
    Editor, icons,
    node_tree::Drag,
    preview::identifier,
    project::{extension, supported},
    strings, theme,
};

/// Kinds of assets the browser can create.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AssetKind {
    Folder,
    Script,
    Scene,
    Prefab,
}

impl AssetKind {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Folder => strings::NEW_FOLDER,
            Self::Script => strings::NEW_SCRIPT,
            Self::Scene => strings::NEW_SCENE,
            Self::Prefab => strings::NEW_PREFAB,
        }
    }

    fn extension(self) -> Option<&'static str> {
        match self {
            Self::Folder => None,
            Self::Script => Some("ts"),
            Self::Scene => Some("scene"),
            Self::Prefab => Some("prefab"),
        }
    }
}

pub(super) fn show(ui: &mut Ui, editor: &mut Editor) {
    editor.asset_focus = ui
        .input(|input| input.pointer.latest_pos())
        .is_some_and(|pointer| ui.max_rect().contains(pointer));
    if editor.asset_paths.is_empty() {
        ui.label(strings::DROP_ASSETS);
        return;
    }
    if !editor.asset_directory.is_dir() {
        editor.asset_directory = editor.project.root.clone();
    }
    let root = editor.project.root.clone();
    let directory = editor.asset_directory.clone();
    breadcrumb(ui, editor, &root, &directory);
    ui.separator();
    toolbar(ui, editor);
    ui.separator();
    files(ui, editor);
}

fn toolbar(ui: &mut Ui, editor: &mut Editor) {
    ui.horizontal(|ui| {
        let selected = editor.asset_selection_vec();
        if ui.available_width() < 460.0 {
            ui.menu_button("☰", |ui| {
                ui.menu_button(strings::ADD_FILE, |ui| creation(ui, editor));
                ui.separator();
                if ui
                    .add_enabled(!selected.is_empty(), egui::Button::new(strings::DELETE))
                    .clicked()
                {
                    editor.asset_delete = selected.clone();
                    ui.close();
                }
                if ui.button(strings::REFRESH_ASSETS).clicked() {
                    editor.refresh_assets();
                    ui.close();
                }
                ui.separator();
                if ui
                    .selectable_label(!editor.asset_grid, strings::LIST_VIEW)
                    .clicked()
                {
                    editor.asset_grid = false;
                    ui.close();
                }
                if ui
                    .selectable_label(editor.asset_grid, strings::GRID_VIEW)
                    .clicked()
                {
                    editor.asset_grid = true;
                    ui.close();
                }
            })
            .response
            .on_hover_text(strings::ASSET_ACTIONS);
            ui.add_sized(
                [ui.available_width().max(40.0), theme::ROW_HEIGHT],
                egui::TextEdit::singleline(&mut editor.asset_search)
                    .hint_text(strings::SEARCH_ASSETS)
                    .margin(egui::Margin::symmetric(6, 3)),
            );
        } else {
            ui.menu_button(format!("+ {}", strings::ADD_FILE), |ui| {
                creation(ui, editor)
            });
            if icons::icon_button_enabled(
                ui,
                icons::Icon::Trash,
                strings::DELETE,
                !selected.is_empty(),
            )
            .clicked()
            {
                editor.asset_delete = selected;
            }
            if icons::icon_button(ui, icons::Icon::Refresh, strings::REFRESH_ASSETS).clicked() {
                editor.refresh_assets();
            }
            let search_width = (ui.available_width() - 2.0 * theme::ICON_BUTTON - 18.0).max(60.0);
            ui.add_sized(
                [search_width, theme::ROW_HEIGHT],
                egui::TextEdit::singleline(&mut editor.asset_search)
                    .hint_text(strings::SEARCH_ASSETS)
                    .margin(egui::Margin::symmetric(6, 3)),
            );
            if icons::icon_button(ui, icons::Icon::List, strings::LIST_VIEW).clicked() {
                editor.asset_grid = false;
            }
            if icons::icon_button(ui, icons::Icon::Grid, strings::GRID_VIEW).clicked() {
                editor.asset_grid = true;
            }
        }
    });
}

fn creation(ui: &mut Ui, editor: &mut Editor) {
    for kind in [
        AssetKind::Folder,
        AssetKind::Script,
        AssetKind::Scene,
        AssetKind::Prefab,
    ] {
        if ui.button(kind.label()).clicked() {
            editor.begin_new_asset(kind);
            ui.close();
        }
    }
}

fn files(ui: &mut Ui, editor: &mut Editor) {
    let root = editor.project.root.clone();
    let directory = editor.asset_directory.clone();
    let query = editor.asset_search.trim().to_lowercase();
    let searching = !query.is_empty();
    let mut entries = if searching {
        search_entries(&root, &query)
    } else {
        read_folder(&directory)
    };
    if searching {
        entries.sort();
        ui.label(strings::SEARCH_RESULTS);
    } else {
        entries.sort_by_key(|path| (!path.is_dir(), path.file_name().map(|name| name.to_owned())));
    }
    let content_width = ui.available_width();
    egui::ScrollArea::vertical()
        .id_salt("asset-files")
        .auto_shrink([false, false])
        .show_viewport(ui, |ui, _| {
            ui.set_width(content_width);
            if entries.is_empty() {
                ui.label(if searching {
                    strings::NO_RESULTS
                } else {
                    strings::DROP_ASSETS
                });
                return;
            }
            if editor.asset_grid {
                ui.horizontal_wrapped(|ui| {
                    for path in &entries {
                        tile(ui, editor, path, &root, searching, &entries);
                    }
                });
            } else {
                for path in &entries {
                    file_row(ui, editor, path, &root, searching, &entries);
                }
            }
        });
}

/// Lists one folder, hiding generated dot-directories and unsupported files.
fn read_folder(directory: &Path) -> Vec<PathBuf> {
    fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.starts_with('.') || (!path.is_dir() && !supported(&path)) {
                return None;
            }
            Some(path)
        })
        .collect()
}

/// Searches the whole project from the search box, like Godot's file-system dock.
fn search_entries(root: &Path, query: &str) -> Vec<PathBuf> {
    const LIMIT: usize = 2000;
    let mut results = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_symlink()) {
                continue;
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.starts_with('.') {
                continue;
            }
            if path.is_dir() {
                if name.contains(query) {
                    results.push(path.clone());
                }
                stack.push(path);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_lowercase();
                if supported(&path) && (name.contains(query) || relative.contains(query)) {
                    results.push(path);
                }
            }
            if results.len() >= LIMIT {
                return results;
            }
        }
    }
    results
}

fn display_name(root: &Path, path: &Path, searching: bool) -> String {
    if searching {
        path.strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned()
    } else {
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    }
}

fn breadcrumb(ui: &mut Ui, editor: &mut Editor, root: &Path, directory: &Path) {
    ui.horizontal(|ui| {
        let parent = directory
            .parent()
            .filter(|parent| parent.starts_with(root))
            .map(Path::to_path_buf);
        if icons::action(
            ui,
            icons::Icon::Parent,
            strings::PARENT_FOLDER,
            None,
            parent.is_some(),
        )
        .clicked()
            && let Some(parent) = parent
        {
            editor.asset_directory = parent;
            editor.clear_asset_selection();
        }
        if ui.small_button("res://").clicked() {
            editor.asset_directory = root.to_path_buf();
            editor.clear_asset_selection();
        }
        let mut current = root.to_path_buf();
        for name in breadcrumb_segments(root, directory) {
            ui.label("/");
            current.push(&name);
            if ui.small_button(name.as_str()).clicked() {
                editor.asset_directory = current.clone();
                editor.clear_asset_selection();
            }
        }
    });
}

fn breadcrumb_segments(root: &Path, directory: &Path) -> Vec<String> {
    directory
        .strip_prefix(root)
        .unwrap_or(directory)
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect()
}

fn paint_label(ui: &Ui, rect: Rect, name: &str, align: egui::Align2) {
    ui.painter().with_clip_rect(rect).text(
        match align {
            egui::Align2::CENTER_BOTTOM => rect.center_bottom(),
            _ => rect.left_center(),
        },
        align,
        name,
        egui::FontId::proportional(12.0),
        theme::TEXT,
    );
}

/// Returns the inclusive display-order range between two entries.
pub(super) fn range_selection(entries: &[PathBuf], anchor: &Path, target: &Path) -> Vec<PathBuf> {
    let Some(first) = entries.iter().position(|entry| entry == anchor) else {
        return vec![target.to_path_buf()];
    };
    let Some(last) = entries.iter().position(|entry| entry == target) else {
        return vec![target.to_path_buf()];
    };
    let (start, end) = if first <= last {
        (first, last)
    } else {
        (last, first)
    };
    entries[start..=end].to_vec()
}

fn selection_modifiers(ui: &Ui) -> (bool, bool) {
    ui.input(|input| {
        (
            input.modifiers.ctrl || input.modifiers.command,
            input.modifiers.shift,
        )
    })
}

fn tile(
    ui: &mut Ui,
    editor: &mut Editor,
    path: &Path,
    root: &Path,
    searching: bool,
    entries: &[PathBuf],
) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(theme::TILE, theme::TILE), Sense::click_and_drag());
    let selected = editor.asset_selection.contains(path);
    crate::icons::row_background(ui, rect, selected, response.hovered());
    let icon_rect =
        Rect::from_center_size(pos2(rect.center().x, rect.top() + 27.0), vec2(32.0, 32.0));
    crate::icons::paint(
        ui.painter(),
        icon_rect,
        crate::icons::file(path),
        if path.is_dir() {
            theme::WARNING
        } else {
            theme::ACCENT
        },
    );
    let name = display_name(root, path, searching);
    paint_label(
        ui,
        Rect::from_min_max(
            pos2(rect.left() + 3.0, rect.bottom() - 24.0),
            pos2(rect.right() - 3.0, rect.bottom() - 2.0),
        ),
        &name,
        egui::Align2::CENTER_BOTTOM,
    );
    response.dnd_set_drag_payload(Drag::Asset(path.to_path_buf()));
    if response.clicked() {
        let (ctrl, shift) = selection_modifiers(ui);
        editor.select_asset(path, ctrl, shift, entries);
    }
    if response.double_clicked() {
        if path.is_dir() {
            editor.asset_directory = path.to_path_buf();
            editor.clear_asset_selection();
        } else {
            editor.pending_open = Some(path.to_path_buf());
        }
    }
    asset_menu(response, editor, path);
}

fn file_row(
    ui: &mut Ui,
    editor: &mut Editor,
    path: &Path,
    root: &Path,
    searching: bool,
    entries: &[PathBuf],
) {
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), theme::ROW_HEIGHT),
        Sense::click_and_drag(),
    );
    let selected = editor.asset_selection.contains(path);
    crate::icons::row_background(ui, rect, selected, response.hovered());
    let icon_rect = Rect::from_center_size(
        pos2(rect.left() + 10.0, rect.center().y),
        vec2(theme::ICON, theme::ICON),
    );
    crate::icons::paint(
        ui.painter(),
        icon_rect,
        crate::icons::file(path),
        if path.is_dir() {
            theme::WARNING
        } else {
            theme::ACCENT
        },
    );
    let name = display_name(root, path, searching);
    paint_label(
        ui,
        Rect::from_min_max(
            pos2(icon_rect.right() + 6.0, rect.top() + 3.0),
            pos2(rect.right() - 4.0, rect.bottom() - 3.0),
        ),
        &name,
        egui::Align2::LEFT_CENTER,
    );
    response.dnd_set_drag_payload(Drag::Asset(path.to_path_buf()));
    if response.clicked() {
        let (ctrl, shift) = selection_modifiers(ui);
        editor.select_asset(path, ctrl, shift, entries);
    }
    if response.double_clicked() {
        if path.is_dir() {
            editor.asset_directory = path.to_path_buf();
            editor.clear_asset_selection();
        } else {
            editor.pending_open = Some(path.to_path_buf());
        }
    }
    asset_menu(response, editor, path);
}

fn asset_menu(response: egui::Response, editor: &mut Editor, path: &Path) {
    response.context_menu(|ui| {
        // Open mirrors double-click; Delete is the primary action so folders
        // can still be removed directly from here.
        if !path.is_dir() && ui.button(strings::OPEN).clicked() {
            editor.pending_open = Some(path.to_path_buf());
            ui.close();
        }
        if ui.button(strings::DELETE).clicked() {
            editor.request_asset_delete(path);
            ui.close();
        }
        if ui.button(strings::EXPLORER).clicked() {
            let folder = if path.is_dir() {
                path
            } else {
                path.parent().unwrap_or(path)
            };
            let result = crate::show_in_files(folder);
            editor.report(result);
            ui.close();
        }
        ui.separator();
        let target = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent().unwrap_or(path).to_path_buf()
        };
        ui.menu_button(strings::ADD_FILE, |ui| {
            editor.asset_directory = target.clone();
            creation(ui, editor);
        });
    });
}

/// Creates a new asset in `directory`, which must already be inside `root`.
pub(super) fn create_with_scene_kind(
    root: &Path,
    directory: &Path,
    name: &str,
    kind: AssetKind,
    scene_kind: SceneKind,
) -> Result<PathBuf> {
    let name = name.trim();
    ensure!(valid_name(name), "{}", strings::INVALID_ASSET_NAME);
    if kind == AssetKind::Script {
        ensure!(identifier(name), "{}", strings::INVALID_SCRIPT_NAME);
    }
    let root = root
        .canonicalize()
        .context("Project directory is missing")?;
    let directory = directory.canonicalize()?;
    let target = directory.join(match kind.extension() {
        Some(extension) => format!("{name}.{extension}"),
        None => name.to_owned(),
    });
    confine(&root, &target)?;
    match kind {
        AssetKind::Folder => {
            fs::create_dir(&target)?;
        }
        AssetKind::Script => {
            fs::create_dir_all(&directory)?;
            let mut output = tempfile::NamedTempFile::new_in(directory)?;
            let source = format!(
                "import {{ Component, NodeRef, Prefab, ccclass, property }} from \"aurora\";\n\n@ccclass('{name}')\nexport class {name} extends Component {{\n  @property({{ type: Number, tooltip: 'Movement speed.' }})\n  speed = 30;\n\n  @property(NodeRef)\n  target: NodeRef | null = null;\n\n  @property(Prefab)\n  prefab: Prefab | null = null;\n}}\n"
            );
            output.write_all(source.as_bytes())?;
            output.persist_noclobber(&target)?;
        }
        AssetKind::Scene => {
            let nodes = crate::project::default_scene_nodes(scene_kind, name);
            SceneFile {
                version: VERSION,
                camera: crate::project::default_camera(&nodes),
                scene_kind,
                nodes,
                instances: Vec::new(),
                script_prefabs: Vec::new(),
                animations: Vec::new(),
                autoplay: None,
                gameplay: None,
                display: None,
                joints: Vec::new(),
            }
            .save(&target)?;
        }
        AssetKind::Prefab => {
            PrefabFile {
                version: VERSION,
                nodes: vec![node(name, None)],
                animations: Vec::new(),
                joints: Vec::new(),
            }
            .save(&target)?;
        }
    }
    Ok(target)
}

/// Removes a file or directory after the editor has confirmed the action.
pub(super) fn remove(root: &Path, path: &Path) -> Result<()> {
    confine(root, path)?;
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}

/// Imports an external file into the project's `assets` folder.
pub(super) fn import(root: &Path, source: &Path) -> Result<()> {
    if matches!(extension(source), "gltf" | "glb") {
        return gltf::import(source, &root.join("assets"));
    }
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

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && !name.contains(['/', '\\'])
        && !name.chars().any(char::is_control)
}

fn confine(root: &Path, path: &Path) -> Result<()> {
    let root = root
        .canonicalize()
        .context("Project directory is missing")?;
    let parent = path
        .parent()
        .context("Asset path has no parent")?
        .canonicalize()?;
    ensure!(parent.starts_with(&root), "Asset path escapes the project");
    Ok(())
}

fn node(name: &str, parent: Option<usize>) -> NodeFile {
    NodeFile {
        name: name.into(),
        parent,
        transform: Default::default(),
        spatial: None,
        visual: None,
        sprite: None,
        appearance: None,
        physics2d: None,
        scripts: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assets_can_be_created_at_the_project_root() -> Result<()> {
        let project = tempfile::tempdir()?;
        let scene = create_with_scene_kind(
            project.path(),
            project.path(),
            "Arena",
            AssetKind::Scene,
            SceneKind::ThreeD,
        )?;
        assert_eq!(scene, project.path().join("Arena.scene"));
        assert!(SceneFile::load(&scene).is_ok());
        Ok(())
    }

    #[test]
    fn search_finds_assets_in_nested_folders_and_skips_hidden_ones() -> Result<()> {
        let root = tempfile::tempdir()?;
        let nested = root.path().join("assets/ui");
        fs::create_dir_all(&nested)?;
        fs::create_dir_all(root.path().join(".git"))?;
        fs::write(nested.join("Panel.ts"), "")?;
        fs::write(root.path().join("assets/Hero.ts"), "")?;
        fs::write(root.path().join(".git/Secret.ts"), "")?;
        assert_eq!(
            search_entries(root.path(), "panel"),
            [nested.join("Panel.ts")]
        );
        assert_eq!(
            search_entries(root.path(), "ui"),
            [nested.clone(), nested.join("Panel.ts")]
        );
        assert!(search_entries(root.path(), "secret").is_empty());
        Ok(())
    }

    #[test]
    fn breadcrumb_segments_stay_inside_the_project() {
        let root = Path::new("/project");
        assert!(breadcrumb_segments(root, root).is_empty());
        assert_eq!(
            breadcrumb_segments(root, &root.join("assets").join("ui")),
            ["assets", "ui"]
        );
    }

    #[test]
    fn assets_can_be_created_and_deleted_inside_the_project() -> Result<()> {
        let project = tempfile::tempdir()?;
        let assets = project.path().join("assets");
        fs::create_dir(&assets)?;
        let script = create_with_scene_kind(
            project.path(),
            &assets,
            "Mover",
            AssetKind::Script,
            SceneKind::TwoD,
        )?;
        assert_eq!(
            script.extension().and_then(|part| part.to_str()),
            Some("ts")
        );
        assert!(
            create_with_scene_kind(
                project.path(),
                &assets,
                "Mover",
                AssetKind::Script,
                SceneKind::TwoD,
            )
            .is_err()
        );
        let folder = create_with_scene_kind(
            project.path(),
            &assets,
            "Levels",
            AssetKind::Folder,
            SceneKind::TwoD,
        )?;
        let scene = create_with_scene_kind(
            project.path(),
            &folder,
            "Level1",
            AssetKind::Scene,
            SceneKind::TwoD,
        )?;
        assert!(SceneFile::load(&scene).is_ok());
        let prefab = create_with_scene_kind(
            project.path(),
            &folder,
            "Pickup",
            AssetKind::Prefab,
            SceneKind::TwoD,
        )?;
        assert!(PrefabFile::load(&prefab).is_ok());
        remove(project.path(), &script)?;
        assert!(!script.exists());
        remove(project.path(), &folder)?;
        assert!(!folder.exists());
        Ok(())
    }

    #[test]
    fn deletion_refuses_to_escape_the_project() -> Result<()> {
        let project = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        let file = outside.path().join("secret.png");
        fs::write(&file, b"not an image")?;
        assert!(remove(project.path(), &file).is_err());
        assert!(file.exists());
        Ok(())
    }

    #[test]
    fn asset_names_reject_paths_and_hidden_files() {
        assert!(valid_name("Hero"));
        assert!(valid_name("Uses Spaces"));
        assert!(!valid_name(""));
        assert!(!valid_name("../Hero"));
        assert!(!valid_name("nested/name"));
        assert!(!valid_name(".hidden"));
    }

    #[test]
    fn creation_stays_in_the_requested_folder() -> Result<()> {
        let project = tempfile::tempdir()?;
        let assets = project.path().join("assets");
        fs::create_dir(&assets)?;
        let folder = create_with_scene_kind(
            project.path(),
            &assets,
            "Sprites",
            AssetKind::Folder,
            SceneKind::TwoD,
        )?;
        let scene = create_with_scene_kind(
            project.path(),
            &folder,
            "Arena",
            AssetKind::Scene,
            SceneKind::TwoD,
        )?;
        assert_eq!(scene, folder.join("Arena.scene"));
        let outside = tempfile::tempdir()?;
        assert!(
            create_with_scene_kind(
                project.path(),
                outside.path(),
                "Escapes",
                AssetKind::Scene,
                SceneKind::TwoD,
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn shift_range_selects_entries_between_anchor_and_target_in_display_order() {
        let entries = [
            PathBuf::from("a"),
            PathBuf::from("b"),
            PathBuf::from("c"),
            PathBuf::from("d"),
        ];
        assert_eq!(
            range_selection(&entries, Path::new("b"), Path::new("d")),
            vec![PathBuf::from("b"), PathBuf::from("c"), PathBuf::from("d")]
        );
        assert_eq!(
            range_selection(&entries, Path::new("d"), Path::new("b")),
            vec![PathBuf::from("b"), PathBuf::from("c"), PathBuf::from("d")]
        );
        assert_eq!(
            range_selection(&entries, Path::new("missing"), Path::new("c")),
            vec![PathBuf::from("c")]
        );
    }

    #[test]
    fn new_scene_modes_create_matching_roots_and_metadata() -> Result<()> {
        let project = tempfile::tempdir()?;
        let assets = project.path().join("assets");
        fs::create_dir(&assets)?;
        for (name, kind, expected_nodes) in [
            ("TwoD", SceneKind::TwoD, 1),
            ("ThreeD", SceneKind::ThreeD, 4),
        ] {
            let path =
                create_with_scene_kind(project.path(), &assets, name, AssetKind::Scene, kind)?;
            let scene = SceneFile::load(&path)?;
            assert_eq!(scene.scene_kind, kind);
            assert_eq!(scene.nodes.len(), expected_nodes);
            assert_eq!(
                scene.nodes.iter().any(|node| node.spatial.is_some()),
                kind == SceneKind::ThreeD
            );
        }
        Ok(())
    }
}
