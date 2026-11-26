//! Native Aurora editor shell.

mod assets_browser;
mod console;
mod docks;
mod export;
mod icons;
mod inspector;
mod node_library;
mod node_tree;
mod preview;
mod project;
mod project_manager;
mod project_types;
mod script_assets;
mod strings;
mod theme;
mod timeline;
mod toolbar;
mod viewport;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use clap::Parser;
use egui::{Event, Ui, UserData, ViewportBuilder, ViewportCommand};
use egui_dock::{DockArea, DockState};
use engine_core::{
    animation::Property,
    save::{InstanceFile, PrefabFile, PropertyValue, SceneKind, VERSION},
};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use node_tree::Drag;
use project::Project;

type Capture = std::rc::Rc<std::cell::Cell<Option<image::ImageResult<()>>>>;

#[derive(Parser)]
#[command(version, about = strings::TITLE)]
struct Options {
    /// Capture the editor layout to a PNG and exit.
    #[arg(long)]
    screenshot: Option<PathBuf>,
    /// Open a project directory or authored scene file.
    #[arg(long)]
    project: Option<PathBuf>,
    /// Start the player immediately, including for screenshot capture.
    #[arg(long, requires = "project")]
    play: bool,
    /// Export saved scenes, assets, native scripts, and an integrity manifest to a new directory.
    #[arg(long, requires = "project", conflicts_with_all = ["screenshot", "play"])]
    export: Option<PathBuf>,
    /// Include another saved scene from the project. Repeat to set the switch order.
    #[arg(long, requires = "export", value_name = "PATH")]
    include_scene: Vec<PathBuf>,
    /// Manifest target to stamp on the export, for example windows-x86_64.
    #[arg(long, requires = "export", value_name = "TARGET")]
    export_target: Option<String>,
    /// Player runtime to bundle instead of the one installed beside the editor.
    #[arg(long, requires = "export", value_name = "PATH")]
    player: Option<PathBuf>,
}

#[derive(Clone)]
enum ConfirmAction {
    Open(PathBuf),
    Delete(PathBuf),
    Create {
        directory: PathBuf,
        name: String,
        scene_kind: SceneKind,
    },
}

struct Editor {
    project: Project,
    layout: DockState<docks::Dock>,
    preview: preview::Preview,
    viewport: viewport::Viewport,
    timeline: timeline::Timeline,
    key_property: Property,
    animation_display: Option<(usize, f64)>,
    asset_paths: Vec<PathBuf>,
    metadata: BTreeMap<PathBuf, BTreeMap<String, codegen::Metadata>>,
    watcher: Option<RecommendedWatcher>,
    watch_events: mpsc::Receiver<notify::Result<notify::Event>>,
    watched: Vec<PathBuf>,
    watcher_due: Option<Instant>,
    asset_search: String,
    tree_search: String,
    selected_asset: Option<PathBuf>,
    /// All FileSystem entries selected with click, Ctrl-click or Shift-click.
    asset_selection: BTreeSet<PathBuf>,
    /// Anchor used by Shift-click range selection.
    asset_anchor: Option<PathBuf>,
    asset_directory: PathBuf,
    asset_grid: bool,
    asset_delete: Vec<PathBuf>,
    custom_nodes: bool,
    rename: Option<(usize, String)>,
    focus_rename: bool,
    focus_tree: bool,
    create_node: bool,
    new_node_search: String,
    new_asset: Option<assets_browser::AssetKind>,
    asset_name: String,
    new_scene_kind: SceneKind,
    component_picker: bool,
    component_search: String,
    open_dialog: bool,
    display_dialog: bool,
    project_settings: bool,
    default_scene: String,
    pending_open: Option<PathBuf>,
    confirm_action: Option<ConfirmAction>,
    close_dialog: bool,
    logs: Vec<String>,
    types_installed: bool,
    error_count: usize,
}

impl Editor {
    fn new(project: Project, play: bool) -> Self {
        let (watcher, watch_events) = make_watcher();
        let default_scene = project::load_session(&project.root).default_scene;
        let mut editor = Self {
            layout: load_layout(&project),
            project,
            preview: preview::Preview::default(),
            viewport: viewport::Viewport::default(),
            timeline: timeline::Timeline::default(),
            key_property: Property::X,
            animation_display: None,
            asset_paths: Vec::new(),
            metadata: BTreeMap::new(),
            watcher,
            watch_events,
            watched: Vec::new(),
            watcher_due: None,
            asset_search: String::new(),
            tree_search: String::new(),
            selected_asset: None,
            asset_selection: BTreeSet::new(),
            asset_anchor: None,
            asset_directory: PathBuf::new(),
            asset_grid: true,
            asset_delete: Vec::new(),
            custom_nodes: false,
            rename: None,
            focus_rename: false,
            focus_tree: false,
            create_node: false,
            new_node_search: String::new(),
            new_asset: None,
            asset_name: String::new(),
            new_scene_kind: SceneKind::TwoD,
            component_picker: false,
            component_search: String::new(),
            open_dialog: false,
            display_dialog: false,
            project_settings: false,
            default_scene,
            pending_open: None,
            confirm_action: None,
            close_dialog: false,
            logs: Vec::new(),
            types_installed: false,
            error_count: 0,
        };
        editor.refresh_assets();
        if play {
            editor.play();
        }
        editor
    }

    fn play(&mut self) {
        self.preview.start(&self.project);
    }

    fn stop_play(&mut self) {
        self.preview.stop();
        self.preview.game_view = false;
    }

    fn report(&mut self, result: Result<()>) {
        if let Err(error) = result {
            self.error_count = self.error_count.saturating_add(1);
            self.logs.push(format!("Error: {error:#}"));
        }
    }

    /// Opens the asset-creation dialog for one asset kind.
    pub(crate) fn begin_new_asset(&mut self, kind: assets_browser::AssetKind) {
        self.new_asset = Some(kind);
        self.asset_name.clear();
    }

    fn create_scene(&mut self, directory: &Path, name: &str, scene_kind: SceneKind) -> Result<()> {
        let root = self.project.root.clone();
        let path = assets_browser::create_with_scene_kind(
            &root,
            directory,
            name,
            assets_browser::AssetKind::Scene,
            scene_kind,
        )?;
        self.selected_asset = Some(path.clone());
        self.pending_open = Some(path);
        self.new_asset = None;
        self.refresh_assets();
        Ok(())
    }

    /// Selects one FileSystem entry, extending the selection with Ctrl or by range with Shift.
    fn select_asset(&mut self, path: &Path, ctrl: bool, shift: bool, entries: &[PathBuf]) {
        if shift {
            let anchor = self
                .asset_anchor
                .clone()
                .or_else(|| self.selected_asset.clone())
                .unwrap_or_else(|| path.to_path_buf());
            self.asset_selection.clear();
            for entry in assets_browser::range_selection(entries, &anchor, path) {
                self.asset_selection.insert(entry);
            }
        } else if ctrl {
            if !self.asset_selection.remove(path) {
                self.asset_selection.insert(path.to_path_buf());
            }
            self.asset_anchor = Some(path.to_path_buf());
        } else {
            self.asset_selection.clear();
            self.asset_selection.insert(path.to_path_buf());
            self.asset_anchor = Some(path.to_path_buf());
        }
        self.selected_asset = if self.asset_selection.contains(path) {
            Some(path.to_path_buf())
        } else {
            self.asset_selection.iter().next_back().cloned()
        };
    }

    fn clear_asset_selection(&mut self) {
        self.asset_selection.clear();
        self.asset_anchor = None;
        self.selected_asset = None;
    }

    fn asset_selection_vec(&self) -> Vec<PathBuf> {
        self.asset_selection
            .iter()
            .filter(|path| path.exists())
            .cloned()
            .collect()
    }

    fn request_asset_delete(&mut self, path: &Path) {
        let targets = if self.asset_selection.contains(path) {
            self.asset_selection_vec()
        } else {
            self.clear_asset_selection();
            self.asset_selection.insert(path.to_path_buf());
            self.asset_anchor = Some(path.to_path_buf());
            self.selected_asset = Some(path.to_path_buf());
            vec![path.to_path_buf()]
        };
        self.asset_delete = targets;
    }

    fn delete_asset(&mut self, path: &Path) -> Result<()> {
        let deleted_dir = path.is_dir();
        assets_browser::remove(&self.project.root, path)?;
        self.asset_selection
            .retain(|selected| !selected.starts_with(path));
        if self
            .selected_asset
            .as_ref()
            .is_some_and(|selected| selected.starts_with(path))
        {
            self.selected_asset = self.asset_selection.iter().next_back().cloned();
        }
        if self
            .asset_anchor
            .as_ref()
            .is_some_and(|anchor| anchor.starts_with(path))
        {
            self.asset_anchor = self.selected_asset.clone();
        }
        if deleted_dir && self.asset_directory.starts_with(path) {
            self.asset_directory = path.parent().unwrap_or(&self.project.root).to_path_buf();
        }
        self.refresh_assets();
        Ok(())
    }

    fn begin_rename(&mut self, index: usize) {
        self.rename = self
            .project
            .document
            .nodes
            .get(index)
            .map(|node| (index, node.name.clone()));
        self.focus_rename = self.rename.is_some();
    }

    fn refresh_assets(&mut self) {
        self.asset_paths = self.project.assets();
        if !self.asset_directory.starts_with(&self.project.root) || !self.asset_directory.is_dir() {
            self.asset_directory = self.project.root.clone();
        }
        self.asset_selection.retain(|path| path.exists());
        if self
            .selected_asset
            .as_ref()
            .is_some_and(|path| !path.exists())
        {
            self.selected_asset = self.asset_selection.iter().next_back().cloned();
            self.asset_anchor = self.selected_asset.clone();
        }
        self.metadata.clear();
        for path in self
            .asset_paths
            .iter()
            .filter(|path| project::extension(path) == "ts")
        {
            if let Ok(source) = fs::read_to_string(path)
                && let Ok(metadata) = codegen::extract(&source)
            {
                self.metadata.insert(path.clone(), metadata);
            }
        }
        if let Some(watcher) = &mut self.watcher {
            for (dir, mode) in
                std::iter::once((self.project.root.clone(), RecursiveMode::NonRecursive)).chain(
                    [
                        "assets",
                        "resources",
                        "scenes",
                        "prefabs",
                        "scripts",
                        "animations",
                        "audio",
                    ]
                    .map(|dir| (self.project.root.join(dir), RecursiveMode::Recursive)),
                )
            {
                if dir.is_dir() && !self.watched.contains(&dir) && watcher.watch(&dir, mode).is_ok()
                {
                    self.watched.push(dir);
                }
            }
        }
        self.project.revision = self.project.revision.wrapping_add(1);
    }

    fn watch(&mut self, ctx: &egui::Context) {
        for event in self.watch_events.try_iter().flatten() {
            if !matches!(event.kind, notify::EventKind::Access(_))
                && event
                    .paths
                    .iter()
                    .any(|path| project::supported(path) || path.is_dir())
            {
                self.watcher_due = Some(Instant::now() + Duration::from_millis(160));
                if self.preview.is_playing()
                    && event
                        .paths
                        .iter()
                        .any(|path| project::extension(path) == "ts" || project::is_prefab(path))
                {
                    self.preview.reload_at = self.watcher_due;
                }
            }
        }
        let now = Instant::now();
        if self.watcher_due.is_some_and(|due| now >= due) {
            self.watcher_due = None;
            self.refresh_assets();
        }
        if self.preview.reload_at.is_some_and(|due| now >= due) {
            self.preview.reload_at = None;
            self.logs.push(strings::RELOADING.into());
            self.play();
        }
        if self.watcher.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn open_asset(&mut self, path: &Path) -> Result<()> {
        if (project::is_scene(path) || project::is_prefab(path)) || path.is_dir() {
            let in_project = path.starts_with(&self.project.root)
                && (project::is_scene(path) || project::is_prefab(path));
            let previous_directory = self.asset_directory.clone();
            let project = if in_project {
                Project::open_in_root(path, &self.project.root)?
            } else {
                Project::open(path)?
            };
            self.save_layout()?;
            self.stop_play();
            self.timeline = timeline::Timeline::default();
            self.animation_display = None;
            self.viewport = viewport::Viewport::default();
            self.clear_asset_selection();
            self.asset_directory = if in_project {
                previous_directory
            } else {
                PathBuf::new()
            };
            self.asset_delete.clear();
            self.rename = None;
            self.layout = load_layout(&project);
            self.default_scene = project::load_session(&project.root).default_scene;
            self.project = project;
            (self.watcher, self.watch_events) = make_watcher();
            self.watched.clear();
            self.refresh_assets();
            Ok(())
        } else {
            open_external(path)
        }
    }

    fn save_prefab(&mut self, index: usize) -> Result<()> {
        let mut remap = BTreeMap::new();
        let mut nodes = Vec::new();
        for (source, node) in self.project.document.nodes.iter().enumerate().skip(index) {
            if source != index
                && node
                    .parent
                    .is_none_or(|parent| !remap.contains_key(&parent))
            {
                continue;
            }
            let mut node = node.clone();
            node.parent = node.parent.and_then(|parent| remap.get(&parent).copied());
            remap.insert(source, nodes.len());
            if let Some(sprite) = &mut node.sprite {
                sprite.png = self.project.root.join(&sprite.png);
            }
            nodes.push(node);
        }
        ensure!(!nodes.is_empty(), "Select a prefab root");
        for node in &mut nodes {
            for script in &mut node.scripts {
                script.path = self.project.root.join(&script.path);
                script.values.retain(|_, value| {
                    match value {
                        PropertyValue::Node(target) => {
                            if let Some(mapped) = remap.get(target) {
                                *target = *mapped;
                            } else {
                                return false;
                            }
                        }
                        PropertyValue::Prefab(path) => *path = self.project.root.join(&*path),
                        _ => {}
                    }
                    true
                });
            }
        }
        let name: String = nodes[0]
            .name
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect();
        let directory = self.project.root.join("prefabs");
        fs::create_dir_all(&directory)?;
        let path = directory.join(format!("{name}.prefab"));
        ensure!(!path.exists(), "Prefab {} already exists", path.display());
        PrefabFile {
            version: VERSION,
            nodes,
            animations: Vec::new(),
            joints: Vec::new(),
        }
        .save(&path)?;
        self.refresh_assets();
        Ok(())
    }

    fn reparent_allowed(&self, id: usize, parent: Option<usize>) -> bool {
        let nodes = &self.project.document.nodes;
        if id >= nodes.len() || (self.project.prefab && id == 0) {
            return false;
        }
        let mut cursor = parent;
        while let Some(anchor) = cursor {
            if anchor == id {
                return false;
            }
            let Some(node) = nodes.get(anchor) else {
                return false;
            };
            cursor = node.parent;
        }
        true
    }

    fn drop_allowed(&self, payload: &Drag, parent: Option<usize>) -> bool {
        match payload {
            Drag::Node(index) => self.reparent_allowed(*index, parent),
            Drag::New(name) => {
                *name == strings::EMPTY_NODE
                    || *name == strings::SPRITE_NODE
                    || crate::inspector::spatial::preset(name).is_some()
                    || crate::inspector::visual::NAMES.contains(name)
            }
            Drag::Asset(path) => match project::extension(path) {
                "png" | "ts" => true,
                _ => project::is_prefab(path) && !self.project.prefab,
            },
        }
    }

    fn drag_preview(&self, ctx: &egui::Context) {
        if !ctx.input(|input| input.pointer.any_down()) {
            return;
        }
        let Some(payload) = egui::DragAndDrop::payload::<Drag>(ctx) else {
            return;
        };
        let Some(position) = ctx.input(|input| input.pointer.hover_pos()) else {
            return;
        };
        let label = node_tree::drag_label(self, payload.as_ref());
        let icon = match payload.as_ref() {
            Drag::Node(index) => {
                let sprite = self
                    .project
                    .document
                    .nodes
                    .get(*index)
                    .is_some_and(|node| node.sprite.is_some());
                icons::node(sprite)
            }
            Drag::Asset(path) => icons::file(path),
            Drag::New(name) => {
                if *name == strings::SPRITE_NODE {
                    icons::Icon::Sprite
                } else {
                    icons::Icon::Node
                }
            }
        };
        egui::Area::new(egui::Id::new("editor-drag-preview"))
            .order(egui::Order::Tooltip)
            .fixed_pos(position + egui::vec2(14.0, 12.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(theme::SHELL)
                    .stroke(egui::Stroke::new(theme::LINE, theme::ACCENT))
                    .corner_radius(theme::ROUND)
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            icons::inline(ui, icon, theme::ACCENT);
                            ui.label(egui::RichText::new(label).color(theme::FOREGROUND));
                        });
                    });
            });
    }

    fn drop_into(&mut self, payload: &Drag, parent: Option<usize>) {
        let result = match payload {
            Drag::Node(index) => self.project.reparent(*index, parent),
            Drag::New(name) => {
                self.project.add(name, parent);
                Ok(())
            }
            Drag::Asset(path) if project::extension(path) == "png" => {
                self.project.drop_sprite(parent, path)
            }
            Drag::Asset(path) if project::extension(path) == "ts" => {
                self.attach_default(path, parent)
            }
            Drag::Asset(path) if project::is_prefab(path) => {
                if self.project.prefab {
                    Err(anyhow::anyhow!(
                        "Prefab documents contain a single inline subtree"
                    ))
                } else {
                    self.project.document.instances.push(InstanceFile {
                        prefab: self.project.relative(path),
                        parent,
                    });
                    self.project.changed();
                    Ok(())
                }
            }
            Drag::Asset(_) => Err(anyhow::anyhow!("Drop a PNG, prefab or component here")),
        };
        self.report(result);
    }

    fn attach_default(&mut self, path: &Path, parent: Option<usize>) -> Result<()> {
        let index = parent
            .or(self.project.selected)
            .context("Select a node first")?;
        let component = self
            .metadata
            .get(path)
            .and_then(|entries| entries.keys().next())
            .context("No metadata for this script")?
            .clone();
        self.project.attach(index, path, &component)
    }

    fn save_layout(&self) -> Result<()> {
        let directory = self.project.root.join(".aurora");
        fs::create_dir_all(&directory)?;
        let mut file = tempfile::NamedTempFile::new_in(&directory)?;
        file.write_all(&serde_json::to_vec(&self.layout)?)?;
        file.persist(directory.join("layout.json"))?;
        let mut session = project::load_session(&self.project.root);
        if !self.project.prefab
            && project::is_scene(&self.project.path)
            && self.project.path.is_file()
            && let Ok(relative) = self.project.path.strip_prefix(&self.project.root)
        {
            session.last_scene = relative.to_string_lossy().into_owned();
        }
        project::save_session(&self.project.root, &session)
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let save = ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::S));
        if save {
            let result = self.project.save();
            self.report(result);
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::O)) {
            self.open_dialog = true;
        }
        if self.rename.is_none()
            && let Some(index) = self.project.selected
        {
            if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F2)) {
                self.begin_rename(index);
            }
            if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::D)) {
                let result = self.project.duplicate(index);
                self.report(result);
            }
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F5))
            && !self.preview.is_playing()
        {
            self.play();
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F8)) {
            self.stop_play();
        }
        let dropped = ctx.input(|input| input.raw.dropped_files.clone());
        for path in dropped.into_iter().filter_map(|file| file.path) {
            let result = assets_browser::import(&self.project.root, &path);
            self.report(result);
            self.refresh_assets();
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if self.display_dialog {
            use engine_core::scene::{Display2D, Fit};
            egui::Window::new(strings::DISPLAY_SETTINGS)
                .open(&mut self.display_dialog)
                .show(ctx, |ui| {
                    let mut display = self.project.document.display.unwrap_or_default();
                    let previous = display;
                    ui.label(strings::DESIGN_RESOLUTION);
                    for (axis, label) in [strings::WIDTH, strings::HEIGHT].into_iter().enumerate() {
                        ui.add(
                            egui::DragValue::new(&mut display.resolution[axis])
                                .prefix(label)
                                .range(1..=Display2D::MAX_DIMENSION),
                        );
                    }
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut display.fit, Fit::Contain, strings::CONTAIN);
                        ui.selectable_value(&mut display.fit, Fit::Cover, strings::COVER);
                        ui.selectable_value(&mut display.fit, Fit::Stretch, strings::STRETCH);
                    });
                    if display != previous {
                        self.project.document.display = Some(display);
                        self.project.changed();
                    }
                });
        }
        if self.project_settings {
            let mut open = true;
            let mut close = false;
            let root = self.project.root.clone();
            let mut result = None;
            egui::Window::new(strings::PROJECT_SETTINGS)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(strings::DEFAULT_SCENE);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.default_scene).hint_text("main.scene"),
                    );
                    if ui.button(strings::SAVE).clicked() {
                        let reference = self.default_scene.trim();
                        if reference.is_empty()
                            || project::valid_scene_ref(&root, reference).is_some()
                        {
                            let mut session = project::load_session(&root);
                            session.default_scene = reference.to_owned();
                            result = Some(project::save_session(&root, &session));
                        } else {
                            result = Some(Err(anyhow::anyhow!(strings::INVALID_DEFAULT_SCENE)));
                        }
                    }
                    if ui.button(strings::CANCEL).clicked() {
                        close = true;
                    }
                });
            if close || result.as_ref().is_some_and(Result::is_ok) {
                open = false;
            }
            self.project_settings = open;
            if let Some(result) = result {
                self.report(result);
            }
        }
        if self.create_node {
            egui::Window::new(strings::CREATE_NODE).show(ctx, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_node_search)
                        .hint_text(strings::SEARCH_NODES),
                );
                for name in [strings::EMPTY_NODE, strings::SPRITE_NODE] {
                    if name
                        .to_lowercase()
                        .contains(&self.new_node_search.to_lowercase())
                        && ui.button(name).clicked()
                    {
                        self.project.add_to_selection(name);
                        self.create_node = false;
                    }
                }
                if ui.button(strings::CANCEL).clicked() {
                    self.create_node = false;
                }
            });
        }
        if let Some(kind) = self.new_asset {
            let root = self.project.root.clone();
            let directory = self.asset_directory.clone();
            egui::Window::new(kind.label()).show(ctx, |ui| {
                let relative = directory.strip_prefix(&root).unwrap_or(&directory);
                ui.label(
                    egui::RichText::new(format!("in {}", relative.display())).color(theme::TEXT),
                );
                if kind == assets_browser::AssetKind::Scene {
                    ui.label(egui::RichText::new(strings::SCENE_TYPE).strong());
                    ui.horizontal(|ui| {
                        for (scene_kind, label) in [
                            (SceneKind::TwoD, "2D"),
                            (SceneKind::ThreeD, "3D"),
                            (SceneKind::TwoDThreeD, "2D + 3D"),
                        ] {
                            ui.selectable_value(&mut self.new_scene_kind, scene_kind, label);
                        }
                    });
                    ui.label(match self.new_scene_kind {
                        SceneKind::TwoD => strings::SCENE_2D_DETAIL,
                        SceneKind::ThreeD => strings::SCENE_3D_DETAIL,
                        SceneKind::TwoDThreeD => strings::SCENE_MIXED_DETAIL,
                    });
                }
                ui.add(
                    egui::TextEdit::singleline(&mut self.asset_name)
                        .hint_text(strings::NAME)
                        .margin(egui::Margin::symmetric(6, 3)),
                );
                if ui.button(strings::CREATE).clicked() {
                    if kind == assets_browser::AssetKind::Scene && self.project.dirty {
                        self.confirm_action = Some(ConfirmAction::Create {
                            directory: directory.clone(),
                            name: self.asset_name.trim().to_owned(),
                            scene_kind: self.new_scene_kind,
                        });
                        self.new_asset = None;
                    } else if kind == assets_browser::AssetKind::Scene {
                        let name = self.asset_name.trim().to_owned();
                        let result = self.create_scene(&directory, &name, self.new_scene_kind);
                        self.report(result);
                    } else {
                        let result = assets_browser::create_with_scene_kind(
                            &root,
                            &directory,
                            self.asset_name.trim(),
                            kind,
                            self.new_scene_kind,
                        );
                        match result {
                            Ok(path) => {
                                self.selected_asset = Some(path);
                                self.new_asset = None;
                                self.refresh_assets();
                            }
                            Err(error) => self.report(Err(error)),
                        }
                    }
                }
                if ui.button(strings::CANCEL).clicked() {
                    self.new_asset = None;
                }
            });
        }
        if !self.asset_delete.is_empty() {
            self.asset_delete.retain(|path| path.exists());
            if self.asset_delete.is_empty() {
                self.asset_delete.clear();
            } else {
                egui::Window::new(strings::DELETE_CONFIRM).show(ctx, |ui| {
                    if self.asset_delete.len() == 1 {
                        let path = &self.asset_delete[0];
                        ui.label(
                            egui::RichText::new(
                                path.file_name().unwrap_or_default().to_string_lossy(),
                            )
                            .strong(),
                        );
                        ui.colored_label(
                            theme::REJECT,
                            if path.is_dir() {
                                strings::DELETE_FOLDER_WARNING
                            } else {
                                strings::DELETE_WARNING
                            },
                        );
                    } else {
                        ui.label(
                            egui::RichText::new(format!(
                                "{} {}",
                                self.asset_delete.len(),
                                strings::ASSETS_SELECTED
                            ))
                            .strong(),
                        );
                        ui.colored_label(theme::REJECT, strings::DELETE_FOLDER_WARNING);
                    }
                    ui.horizontal(|ui| {
                        if ui
                            .add(egui::Button::new(strings::DELETE).fill(theme::PRIMARY))
                            .clicked()
                        {
                            let targets = std::mem::take(&mut self.asset_delete);
                            for path in targets {
                                if path == self.project.path && self.project.dirty {
                                    self.confirm_action = Some(ConfirmAction::Delete(path));
                                } else {
                                    let result = self.delete_asset(&path);
                                    self.report(result);
                                }
                            }
                        }
                        if ui.button(strings::CANCEL).clicked() {
                            self.asset_delete.clear();
                        }
                    });
                });
            }
        }
        if self.component_picker {
            egui::Window::new(strings::ADD_COMPONENT).show(ctx, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.component_search)
                        .hint_text(strings::COMPONENT_QUERY),
                );
                let search = self.component_search.to_lowercase();
                let mut selection = None;
                for (path, entries) in &self.metadata {
                    for name in entries.keys() {
                        if name.to_lowercase().contains(&search) && ui.button(name).clicked() {
                            selection = Some((path.clone(), name.clone()));
                        }
                    }
                }
                if let (Some(index), Some((path, name))) = (self.project.selected, selection) {
                    let result = self.project.attach(index, &path, &name);
                    self.component_picker = result.is_err();
                    self.report(result);
                }
                if ui.button(strings::CANCEL).clicked() {
                    self.component_picker = false;
                }
            });
        }
        if let Some(action) = self.confirm_action.clone() {
            let mut open = true;
            let mut close = false;
            let mut resolved = false;
            let mut save_result = None;
            let mut delete_result = None;
            let mut create_result = None;
            egui::Window::new(strings::UNSAVED_CHANGES)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(strings::UNSAVED_OPEN);
                    if ui.button(strings::SAVE).clicked() {
                        save_result = Some(self.project.save());
                        resolved = save_result.as_ref().is_some_and(Result::is_ok);
                    }
                    if ui.button(strings::DISCARD).clicked() {
                        resolved = true;
                    }
                    if ui.button(strings::CANCEL).clicked() {
                        close = true;
                    }
                });
            if close {
                open = false;
            }
            if resolved && save_result.as_ref().is_none_or(Result::is_err) {
                self.project.dirty = false;
            }
            if !open {
                match &action {
                    ConfirmAction::Open(path) => {
                        self.pending_open = resolved.then(|| path.clone());
                    }
                    ConfirmAction::Delete(path) => {
                        if resolved {
                            delete_result = Some(self.delete_asset(path));
                        }
                    }
                    ConfirmAction::Create {
                        directory,
                        name,
                        scene_kind,
                    } => {
                        if resolved {
                            create_result = Some(self.create_scene(directory, name, *scene_kind));
                        } else {
                            self.new_asset = Some(assets_browser::AssetKind::Scene);
                            self.asset_name = name.clone();
                            self.new_scene_kind = *scene_kind;
                            self.asset_directory = directory.clone();
                        }
                    }
                }
                self.confirm_action = None;
            }
            if let Some(result) = save_result {
                self.report(result);
            }
            if let Some(result) = delete_result {
                self.report(result);
            }
            if let Some(result) = create_result {
                self.report(result);
            }
        }
        if self.close_dialog {
            egui::Window::new(strings::SAVE_FIRST).show(ctx, |ui| {
                if ui.button(strings::SAVE).clicked() {
                    let result = self.project.save();
                    if result.is_ok() {
                        ctx.send_viewport_cmd(ViewportCommand::Close);
                        self.close_dialog = false;
                    }
                    self.report(result);
                }
                if ui.button(strings::DISCARD).clicked() {
                    self.project.dirty = false;
                    self.close_dialog = false;
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
                if ui.button(strings::CANCEL).clicked() {
                    self.close_dialog = false;
                }
            });
        }
    }
}

struct PendingUpgrade {
    project: Project,
    version: String,
    play: bool,
}

struct Workbench {
    editor: Option<Box<Editor>>,
    manager: project_manager::Manager,
    upgrade: Option<PendingUpgrade>,
    pending_open: Option<(PathBuf, Option<project::ProjectKind>)>,
    managing: bool,
    screenshot: Option<PathBuf>,
    requested_size: Option<[usize; 2]>,
    capture_result: Capture,
    capture_game: bool,
    start_deadline: Instant,
}

impl Workbench {
    fn new(project: Option<Project>, options: Options, capture_result: Capture) -> Self {
        let mut workbench = Self {
            managing: true,
            editor: None,
            manager: project_manager::Manager::new(),
            upgrade: None,
            pending_open: None,
            screenshot: options.screenshot,
            requested_size: None,
            capture_result,
            capture_game: options.play,
            start_deadline: Instant::now() + Duration::from_secs(20),
        };
        if let Some(project) = project
            && let Err(error) = workbench.prepare_project(project, options.play)
        {
            workbench.manager.error = Some(format!("{error:#}"));
        }
        workbench
    }

    fn capture(&mut self, ui: &Ui, ctx: &egui::Context) {
        if self.screenshot.is_none() {
            return;
        }
        if self.capture_game && self.editor.is_none() {
            let reason = if self.upgrade.is_some() {
                "The project requires an SDK update before Play".into()
            } else {
                self.manager
                    .error
                    .clone()
                    .unwrap_or_else(|| "Open a project before capturing Game view".into())
            };
            self.capture_result.set(Some(Err(image::ImageError::IoError(
                std::io::Error::other(reason),
            ))));
            self.screenshot = None;
            ctx.send_viewport_cmd(ViewportCommand::Close);
            return;
        }
        if self.capture_game
            && let Some(editor) = &self.editor
            && editor.preview.texture.is_none()
        {
            if Instant::now() >= self.start_deadline || !editor.preview.is_playing() {
                self.capture_result.set(Some(Err(image::ImageError::IoError(
                    std::io::Error::other(editor.logs.join("\n")),
                ))));
                self.screenshot = None;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            return;
        }
        let Some(path) = &self.screenshot else {
            return;
        };
        let pixels = ctx.viewport_rect().size() * ctx.pixels_per_point();
        let size = [pixels.x.round() as usize, pixels.y.round() as usize];
        let frame = ui.input(|input| {
            input.events.iter().find_map(|event| match event {
                Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(frame) = &frame
            && self.requested_size == Some(size)
            && frame
                .size
                .iter()
                .zip(size)
                .all(|(actual, expected)| actual.abs_diff(expected) <= 1)
        {
            let [width, height] = frame.size;
            let pixels: Vec<_> = frame
                .pixels
                .iter()
                .flat_map(|color| color.to_srgba_unmultiplied())
                .collect();
            self.capture_result.set(Some(image::save_buffer_with_format(
                path,
                &pixels,
                width as u32,
                height as u32,
                image::ColorType::Rgba8,
                image::ImageFormat::Png,
            )));
            self.screenshot = None;
            ctx.send_viewport_cmd(ViewportCommand::Close);
        } else if frame.is_some() || self.requested_size.is_none() {
            // A compositor resize may change the swapchain while a capture is in flight.
            self.requested_size = Some(size);
            ctx.send_viewport_cmd(ViewportCommand::Screenshot(UserData::default()));
        }
    }
}

impl eframe::App for Workbench {
    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // An active dirty document still owns the close confirmation.
        if self.managing
            && ctx.input(|input| input.viewport().close_requested())
            && self
                .editor
                .as_ref()
                .is_some_and(|editor| editor.project.dirty)
        {
            self.managing = false;
        }
        if self.managing {
            let dirty = self
                .editor
                .as_ref()
                .is_some_and(|editor| editor.project.dirty);
            let upgrade = self
                .upgrade
                .as_ref()
                .map(|pending| (pending.project.path.as_path(), pending.version.as_str()));
            if let Some(action) = self.manager.show(ui, self.editor.is_some(), dirty, upgrade) {
                match action {
                    project_manager::Action::Back => self.managing = false,
                    project_manager::Action::Open(path) => self.open(path, None),
                    project_manager::Action::Create(path, kind) => self.open(path, Some(kind)),
                    project_manager::Action::ApproveUpgrade => self.approve_upgrade(),
                    project_manager::Action::CancelUpgrade => {
                        self.upgrade = None;
                    }
                }
                ctx.request_repaint();
            }
        } else if let Some(editor) = &mut self.editor {
            editor.ui(ui, frame);
            if std::mem::take(&mut editor.open_dialog) {
                self.managing = true;
                ctx.request_repaint();
            }
        }
        self.pending_open_dialog(&ctx);
        self.capture(ui, &ctx);
    }
}

impl Workbench {
    fn open(&mut self, path: PathBuf, create: Option<project::ProjectKind>) {
        if self
            .editor
            .as_ref()
            .is_some_and(|editor| editor.project.dirty)
        {
            self.pending_open = Some((path, create));
            return;
        }
        let result = (|| {
            let project = if let Some(kind) = create {
                Project::create(&path, kind)?
            } else {
                Project::open_existing(&path)?
            };
            self.prepare_project(project, false)
        })();
        if let Err(error) = result {
            self.manager.error = Some(format!("{error:#}"));
        }
    }

    fn pending_open_dialog(&mut self, ctx: &egui::Context) {
        let Some((path, create)) = self.pending_open.clone() else {
            return;
        };
        let mut open = true;
        let mut close = false;
        let mut resolved = false;
        let mut save_result = None;
        egui::Window::new(strings::UNSAVED_CHANGES)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(strings::UNSAVED_OPEN);
                if ui.button(strings::SAVE).clicked()
                    && let Some(editor) = &mut self.editor
                {
                    save_result = Some(editor.project.save());
                    resolved = save_result.as_ref().is_some_and(Result::is_ok);
                }
                if ui.button(strings::DISCARD).clicked() {
                    resolved = true;
                }
                if ui.button(strings::CANCEL).clicked() {
                    close = true;
                }
            });
        if close {
            open = false;
        }
        if resolved
            && save_result.as_ref().is_none_or(Result::is_err)
            && let Some(editor) = &mut self.editor
        {
            editor.project.dirty = false;
        }
        if !open {
            self.pending_open = None;
            if resolved {
                self.open(path, create);
            }
        }
        if let Some(Err(error)) = save_result {
            self.manager.error = Some(format!("{error:#}"));
        }
    }

    fn prepare_project(&mut self, project: Project, play: bool) -> Result<()> {
        if project.path.is_file() && !project.prefab {
            let root = project_types::root(&project.root);
            let missing = match project_types::status(root)? {
                project_types::Status::Different(version) => {
                    self.upgrade = Some(PendingUpgrade {
                        project,
                        version,
                        play,
                    });
                    self.manager.prepare_upgrade();
                    return Ok(());
                }
                project_types::Status::Missing => true,
                project_types::Status::Current => false,
            };
            project_types::refresh(root, None)?;
            self.activate(project, play, missing);
        } else {
            self.activate(project, play, false);
        }
        Ok(())
    }

    fn approve_upgrade(&mut self) {
        if let Some(pending) = self.upgrade.take() {
            let root = project_types::root(&pending.project.root);
            match project_types::refresh(root, Some(&pending.version)) {
                Ok(()) => self.activate(pending.project, pending.play, false),
                Err(error) => {
                    self.manager.error = Some(format!("{error:#}"));
                    self.upgrade = Some(pending);
                }
            }
        }
    }

    fn activate(&mut self, project: Project, play: bool, types_installed: bool) {
        self.manager.remember(&project);
        let mut editor = Editor::new(project, play);
        editor.types_installed = types_installed;
        self.editor = Some(Box::new(editor));
        self.start_deadline = Instant::now() + Duration::from_secs(20);
        self.managing = false;
    }
}

impl eframe::App for Editor {
    fn ui(&mut self, ui: &mut Ui, _: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if ctx.input(|input| input.viewport().close_requested()) && self.project.dirty {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.close_dialog = true;
        }
        self.watch(&ctx);
        let before = self.logs.len();
        self.preview.poll(&ctx, &mut self.logs);
        self.error_count = self.error_count.saturating_add(
            self.logs[before..]
                .iter()
                .filter(|line| {
                    let line = line.to_lowercase();
                    line.contains("error") || line.contains("failed")
                })
                .count(),
        );
        if self.logs.len() > 500 {
            let excess = self.logs.len() - 500;
            self.logs.drain(..excess);
        }
        self.shortcuts(&ctx);
        let chrome = egui::Frame::new()
            .fill(theme::SHELL)
            .inner_margin(theme::TOOLBAR_MARGIN);
        egui::Panel::top("editor-menu")
            .frame(chrome)
            .show(ui, |ui| {
                toolbar::menus(ui, self);
                ui.separator();
                toolbar::show(ui, self);
                if self.types_installed {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(theme::WARNING, strings::TYPES_INSTALLED);
                        if ui.small_button(strings::DISMISS).clicked() {
                            self.types_installed = false;
                        }
                    });
                }
            });
        egui::Panel::bottom("editor-status")
            .frame(chrome)
            .show(ui, |ui| toolbar::status(ui, self));
        let style = theme::dock_style(ui.style());
        let mut layout = std::mem::replace(&mut self.layout, DockState::new(Vec::new()));
        DockArea::new(&mut layout)
            .style(style)
            .show_leaf_close_all_buttons(false)
            .show_inside(ui, &mut docks::Viewer(self));
        self.layout = layout;
        self.drag_preview(&ctx);
        if self.confirm_action.is_none()
            && let Some(path) = self.pending_open.clone()
        {
            if self.project.dirty {
                self.confirm_action = Some(ConfirmAction::Open(path));
            } else {
                self.pending_open = None;
                let result = self.open_asset(&path);
                self.report(result);
            }
        }
        self.dialogs(&ctx);
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        if let Err(error) = self.save_layout() {
            eprintln!("Saving editor layout: {error:#}");
        }
    }
}

fn load_layout(project: &Project) -> DockState<docks::Dock> {
    fs::read(project.root.join(".aurora/layout.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_else(docks::initial_layout)
}

fn make_watcher() -> (
    Option<RecommendedWatcher>,
    mpsc::Receiver<notify::Result<notify::Event>>,
) {
    let (send, receive) = mpsc::sync_channel(256);
    (
        notify::recommended_watcher(move |event| {
            let _ = send.try_send(event);
        })
        .ok(),
        receive,
    )
}

pub(crate) fn show_in_files(path: &Path) -> Result<()> {
    open_external(&folder_to_show(path)?)
}

pub(crate) fn folder_to_show(path: &Path) -> Result<PathBuf> {
    let directory = if path.is_dir() {
        path
    } else {
        path.parent().context("Path has no containing folder")?
    };
    ensure!(
        directory.is_dir(),
        "Folder does not exist: {}",
        directory.display()
    );
    directory
        .canonicalize()
        .context("Resolving folder to show in files")
}

fn open_external(path: &Path) -> Result<()> {
    let mut command = Command::new("xdg-open");
    command.arg(path);
    let mut child = command.spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Finds the SDK shipped beside the editor, or the workspace SDK in a development build.
fn engine_resources() -> anyhow::Result<std::path::PathBuf> {
    let packaged = std::env::current_exe()?.with_file_name("sdk");
    if packaged.is_dir() {
        Ok(packaged)
    } else {
        Ok(std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()?)
    }
}

fn main() -> Result<()> {
    let options = Options::parse();
    if let Some(output) = &options.export {
        let source = options
            .project
            .as_deref()
            .context("Export requires a project")?;
        let project = Project::open_existing(source)?;
        return match (options.export_target.as_deref(), options.player.as_deref()) {
            (None, None) => export::export(&project, &options.include_scene, output),
            (target, player) => {
                export::export_to(&project, &options.include_scene, output, target, player)
            }
        };
    }
    let project = options.project.as_deref().map(Project::open).transpose()?;
    let capture_requested = options.screenshot.is_some();
    let capture_result = Capture::default();
    let result = capture_result.clone();
    eframe::run_native(
        strings::TITLE,
        eframe::NativeOptions {
            viewport: ViewportBuilder::default()
                .with_inner_size(theme::WINDOW_SIZE)
                .with_maximized(capture_requested),
            ..Default::default()
        },
        Box::new(move |context| {
            theme::apply(&context.egui_ctx);
            Ok(Box::new(Workbench::new(project, options, result)))
        }),
    )
    .map_err(|error| anyhow::anyhow!("{error}"))?;
    if capture_requested {
        capture_result
            .take()
            .context("Editor closed before screenshot capture")??;
    }
    Ok(())
}
