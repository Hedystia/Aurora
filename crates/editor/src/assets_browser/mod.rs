//! Godot-style file system browser, import and asset creation.

mod gltf;

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use egui::{Align, Layout, Rect, Sense, Ui, pos2, vec2};
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
    if editor.asset_paths.is_empty() {
        ui.label(strings::DROP_ASSETS);
        return;
    }
    if !editor.asset_directory.is_dir() {
        editor.asset_directory = editor.project.root.join("assets");
    }
    let root = editor.project.root.clone();
    let directory = editor.asset_directory.clone();
    breadcrumb(ui, editor, &root, &directory);
    ui.separator();
    toolbar(ui, editor);
    ui.separator();
    let height = ui.available_height();
    ui.horizontal_top(|ui| {
        let width = ui.available_width();
        if width >= 190.0 {
            let max_tree_width = (width - 100.0).max(72.0);
            let tree_width = editor.asset_tree_width.clamp(72.0, max_tree_width);
            ui.allocate_ui_with_layout(
                vec2(tree_width, height),
                Layout::top_down(Align::Min),
                |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("asset-tree")
                        .auto_shrink([false, false])
                        .show(ui, |ui| folder_tree(ui, editor, &root, 0));
                },
            );
            let (divider, response) = ui.allocate_exact_size(vec2(7.0, height), Sense::drag());
            if response.hovered() || response.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            let color = if response.dragged() || response.hovered() {
                theme::ACCENT
            } else {
                theme::BORDER
            };
            ui.painter().line_segment(
                [divider.center_top(), divider.center_bottom()],
                egui::Stroke::new(2.0, color),
            );
            if response.dragged() {
                editor.asset_tree_width = (editor.asset_tree_width + response.drag_motion().x)
                    .clamp(72.0, max_tree_width);
            }
            response.on_hover_text(strings::RESIZE_ASSET_TREE);
        }
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), height),
            Layout::top_down(Align::Min),
            |ui| files(ui, editor),
        );
    });
}

fn toolbar(ui: &mut Ui, editor: &mut Editor) {
    ui.horizontal(|ui| {
        let selected = editor.selected_asset.clone().filter(|path| path.exists());
        if ui.available_width() < 460.0 {
            ui.menu_button("☰", |ui| {
                ui.menu_button(strings::ADD_FILE, |ui| creation(ui, editor));
                ui.separator();
                if ui
                    .add_enabled(selected.is_some(), egui::Button::new(strings::DELETE))
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
                selected.is_some(),
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

fn folder_tree(ui: &mut Ui, editor: &mut Editor, directory: &Path, depth: usize) {
    let mut children: Vec<PathBuf> = editor
        .asset_paths
        .iter()
        .filter(|path| path.is_dir() && path.parent() == Some(directory))
        .cloned()
        .collect();
    children.sort();
    for child in children {
        let name = child.file_name().unwrap_or_default().to_string_lossy();
        let selected = editor.asset_directory == child;
        let id = ui.make_persistent_id(&child);
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, depth == 0)
            .show_header(ui, |ui| {
                crate::icons::inline(ui, crate::icons::Icon::Folder, theme::WARNING);
                let response = ui.selectable_label(selected, name.as_ref());
                if response.clicked() {
                    editor.asset_directory = child.clone();
                    editor.selected_asset = None;
                }
                if response.double_clicked() {
                    editor.asset_directory = child.clone();
                }
                response.context_menu(|ui| {
                    if ui.button(strings::EXPLORER).clicked() {
                        let result = crate::show_in_files(&child);
                        editor.report(result);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(strings::DELETE).clicked() {
                        editor.asset_delete = Some(child.clone());
                        ui.close();
                    }
                });
            })
            .body(|ui| folder_tree(ui, editor, &child, depth + 1));
    }
}

fn files(ui: &mut Ui, editor: &mut Editor) {
    let directory = editor.asset_directory.clone();
    let query = editor.asset_search.to_lowercase();
    let mut entries: Vec<PathBuf> = fs::read_dir(&directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.starts_with('.') || (!path.is_dir() && !supported(&path)) {
                return None;
            }
            name.contains(&query).then_some(path)
        })
        .collect();
    entries.sort_by_key(|path| (!path.is_dir(), path.file_name().map(|name| name.to_owned())));
    egui::ScrollArea::vertical()
        .id_salt("asset-files")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if entries.is_empty() {
                ui.label(strings::DROP_ASSETS);
                return;
            }
            if editor.asset_grid {
                ui.horizontal_wrapped(|ui| {
                    for path in &entries {
                        tile(ui, editor, path);
                    }
                });
            } else {
                for path in &entries {
                    file_row(ui, editor, path);
                }
            }
        });
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
            editor.selected_asset = None;
        }
        if ui.small_button("res://").clicked() {
            editor.asset_directory = root.to_path_buf();
            editor.selected_asset = None;
        }
        let mut current = root.to_path_buf();
        for name in breadcrumb_segments(root, directory) {
            ui.label("/");
            current.push(&name);
            if ui.small_button(name.as_str()).clicked() {
                editor.asset_directory = current.clone();
                editor.selected_asset = None;
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

fn tile(ui: &mut Ui, editor: &mut Editor, path: &Path) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(theme::TILE, theme::TILE), Sense::click_and_drag());
    let selected = editor.selected_asset.as_deref() == Some(path);
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
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    ui.put(
        Rect::from_min_max(
            pos2(rect.left() + 3.0, rect.bottom() - 24.0),
            pos2(rect.right() - 3.0, rect.bottom() - 2.0),
        ),
        egui::Label::new(egui::RichText::new(name.as_ref()).color(theme::TEXT))
            .truncate()
            .selectable(false),
    );
    response.dnd_set_drag_payload(Drag::Asset(path.to_path_buf()));
    if response.clicked() {
        editor.selected_asset = Some(path.to_path_buf());
    }
    if response.double_clicked() {
        if path.is_dir() {
            editor.asset_directory = path.to_path_buf();
            editor.selected_asset = None;
        } else {
            editor.pending_open = Some(path.to_path_buf());
        }
    }
    asset_menu(response, editor, path);
}

fn file_row(ui: &mut Ui, editor: &mut Editor, path: &Path) {
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), theme::ROW_HEIGHT),
        Sense::click_and_drag(),
    );
    let selected = editor.selected_asset.as_deref() == Some(path);
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
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    ui.put(
        Rect::from_min_max(
            pos2(icon_rect.right() + 6.0, rect.top() + 3.0),
            pos2(rect.right() - 4.0, rect.bottom() - 3.0),
        ),
        egui::Label::new(name.as_ref()).truncate().selectable(false),
    );
    response.dnd_set_drag_payload(Drag::Asset(path.to_path_buf()));
    if response.clicked() {
        editor.selected_asset = Some(path.to_path_buf());
    }
    if response.double_clicked() {
        if path.is_dir() {
            editor.asset_directory = path.to_path_buf();
            editor.selected_asset = None;
        } else {
            editor.pending_open = Some(path.to_path_buf());
        }
    }
    asset_menu(response, editor, path);
}

fn asset_menu(response: egui::Response, editor: &mut Editor, path: &Path) {
    response.context_menu(|ui| {
        if ui.button(strings::OPEN).clicked() {
            if path.is_dir() {
                editor.asset_directory = path.to_path_buf();
                editor.selected_asset = None;
            } else {
                editor.pending_open = Some(path.to_path_buf());
            }
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
        if ui.button(strings::DELETE).clicked() {
            editor.asset_delete = Some(path.to_path_buf());
            ui.close();
        }
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
    confine(&root, directory)?;
    let directory = directory.canonicalize()?;
    let target = directory.join(match kind.extension() {
        Some(extension) => format!("{name}.{extension}"),
        None => name.to_owned(),
    });
    ensure!(target.starts_with(&root), "Asset path escapes the project");
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
            SceneFile {
                version: VERSION,
                scene_kind,
                nodes: crate::project::default_scene_nodes(scene_kind, name),
                instances: Vec::new(),
                script_prefabs: Vec::new(),
                animations: Vec::new(),
                autoplay: None,
                gameplay: None,
                display: None,
            }
            .save(&target)?;
        }
        AssetKind::Prefab => {
            PrefabFile {
                version: VERSION,
                nodes: vec![node(name, None)],
                animations: Vec::new(),
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
    fn new_scene_modes_create_matching_roots_and_metadata() -> Result<()> {
        let project = tempfile::tempdir()?;
        let assets = project.path().join("assets");
        fs::create_dir(&assets)?;
        for (name, kind, expected_nodes) in [
            ("ThreeD", SceneKind::ThreeD, 4),
            ("Mixed", SceneKind::TwoDThreeD, 5),
        ] {
            let path =
                create_with_scene_kind(project.path(), &assets, name, AssetKind::Scene, kind)?;
            let scene = SceneFile::load(&path)?;
            assert_eq!(scene.scene_kind, kind);
            assert_eq!(scene.nodes.len(), expected_nodes);
            assert!(scene.nodes.iter().any(|node| node.spatial.is_some()));
            if kind == SceneKind::TwoDThreeD {
                assert!(scene.nodes.iter().any(|node| node.spatial.is_none()));
            }
        }
        Ok(())
    }
}
