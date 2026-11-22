//! Native Aurora editor shell.

mod assets_browser;
mod console;
mod docks;
mod inspector;
mod node_library;
mod node_tree;
mod preview;
mod project;
mod strings;
mod theme;
mod timeline;
mod toolbar;
mod viewport;

use std::{
    collections::BTreeMap,
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
use egui_dock::{DockArea, DockState, Style};
use engine_core::{
    animation::Property,
    save::{InstanceFile, PrefabFile, PropertyValue, VERSION},
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
    #[arg(long, default_value = ".")]
    project: PathBuf,
    /// Start the player immediately, including for screenshot capture.
    #[arg(long)]
    play: bool,
}

struct Editor {
    project: Project,
    layout: DockState<docks::Dock>,
    screenshot: Option<PathBuf>,
    requested_size: Option<[usize; 2]>,
    capture_result: Capture,
    capture_game: bool,
    start_deadline: Instant,
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
    custom_nodes: bool,
    rename: Option<(usize, String)>,
    focus_rename: bool,
    create_node: bool,
    new_node_search: String,
    new_asset: Option<bool>,
    asset_name: String,
    component_picker: bool,
    component_search: String,
    open_dialog: bool,
    pending_open: Option<PathBuf>,
    open_path: String,
    close_dialog: bool,
    logs: Vec<String>,
    error_count: usize,
}

impl Editor {
    fn new(project: Project, options: Options, capture_result: Capture) -> Self {
        let (watcher, watch_events) = make_watcher();
        let mut editor = Self {
            layout: load_layout(&project),
            project,
            screenshot: options.screenshot,
            requested_size: None,
            capture_result,
            capture_game: options.play,
            start_deadline: Instant::now() + Duration::from_secs(20),
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
            custom_nodes: false,
            rename: None,
            focus_rename: false,
            create_node: false,
            new_node_search: String::new(),
            new_asset: None,
            asset_name: String::new(),
            component_picker: false,
            component_search: String::new(),
            open_dialog: false,
            pending_open: None,
            open_path: String::new(),
            close_dialog: false,
            logs: Vec::new(),
            error_count: 0,
        };
        editor.refresh_assets();
        if options.play {
            editor.play();
        }
        editor
    }

    fn play(&mut self) {
        self.preview.start(&self.project);
    }

    fn report(&mut self, result: Result<()>) {
        if let Err(error) = result {
            self.error_count = self.error_count.saturating_add(1);
            self.logs.push(format!("Error: {error:#}"));
        }
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
                        .any(|path| project::extension(path) == "ts")
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
        if matches!(project::extension(path), "aurora-scene" | "aurora-prefab") || path.is_dir() {
            ensure!(!self.project.dirty, "{}", strings::SAVE_FIRST);
            let project = Project::open(path)?;
            self.save_layout()?;
            self.preview.stop();
            self.preview.game_view = false;
            self.timeline = timeline::Timeline::default();
            self.animation_display = None;
            self.viewport = viewport::Viewport::default();
            self.selected_asset = None;
            self.rename = None;
            self.layout = load_layout(&project);
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
        let path = directory.join(format!("{name}.aurora-prefab"));
        ensure!(!path.exists(), "Prefab {} already exists", path.display());
        PrefabFile {
            version: VERSION,
            nodes,
        }
        .save(&path)?;
        self.refresh_assets();
        Ok(())
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
            Drag::Asset(path) if project::extension(path) == "aurora-prefab" => {
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
        Ok(())
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let save = ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::S));
        if save {
            let result = self.project.save();
            self.report(result);
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
        let dropped = ctx.input(|input| input.raw.dropped_files.clone());
        for path in dropped.into_iter().filter_map(|file| file.path) {
            let result = assets_browser::import(&self.project.root, &path);
            self.report(result);
            self.refresh_assets();
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if self.open_dialog {
            egui::Window::new(strings::OPEN_PROJECT).show(ctx, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.open_path)
                        .hint_text(strings::PROJECT_PATH),
                );
                if ui.button(strings::OPEN).clicked() {
                    let path = PathBuf::from(self.open_path.trim());
                    let result = self.open_asset(&path);
                    self.open_dialog = result.is_err();
                    self.report(result);
                }
                if ui.button(strings::CANCEL).clicked() {
                    self.open_dialog = false;
                }
            });
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
                        self.project.add(name, self.project.selected);
                        self.create_node = false;
                    }
                }
                if ui.button(strings::CANCEL).clicked() {
                    self.create_node = false;
                }
            });
        }
        if let Some(script) = self.new_asset {
            egui::Window::new(if script {
                strings::NEW_SCRIPT
            } else {
                strings::NEW_FOLDER
            })
            .show(ctx, |ui| {
                ui.text_edit_singleline(&mut self.asset_name);
                if ui.button(strings::CREATE).clicked() {
                    let result =
                        assets_browser::create(&self.project.root, self.asset_name.trim(), script);
                    if result.is_ok() {
                        self.new_asset = None;
                        self.refresh_assets();
                    }
                    self.report(result);
                }
                if ui.button(strings::CANCEL).clicked() {
                    self.new_asset = None;
                }
            });
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

    fn capture(&mut self, ui: &Ui, ctx: &egui::Context) {
        if self.capture_game && self.preview.texture.is_none() {
            if Instant::now() >= self.start_deadline || !self.preview.is_playing() {
                self.capture_result.set(Some(Err(image::ImageError::IoError(
                    std::io::Error::other(self.logs.join("\n")),
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
        egui::Panel::top("editor-menu").show(ui, |ui| {
            toolbar::menus(ui, self);
            toolbar::show(ui, self);
        });
        egui::Panel::bottom("editor-status").show(ui, |ui| toolbar::status(ui, self));
        let style = Style::from_egui(ui.style().as_ref());
        let mut layout = std::mem::replace(&mut self.layout, DockState::new(Vec::new()));
        DockArea::new(&mut layout)
            .style(style)
            .show_leaf_close_all_buttons(false)
            .show_inside(ui, &mut docks::Viewer(self));
        self.layout = layout;
        if let Some(path) = self.pending_open.take() {
            let result = self.open_asset(&path);
            self.report(result);
        }
        self.dialogs(&ctx);
        self.capture(ui, &ctx);
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

fn open_external(path: &Path) -> Result<()> {
    let mut command = Command::new("xdg-open");
    command.arg(path);
    let mut child = command.spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn main() -> Result<()> {
    let options = Options::parse();
    let project = Project::open(&options.project)?;
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
            Ok(Box::new(Editor::new(project, options, result)))
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
