//! Startup project browser and persistent recent-project catalog.

mod browser;
pub(crate) mod settings;

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use egui::{Align, Button, Context as EguiContext, Layout, Modal, RichText, TextEdit, Ui};

use crate::{
    project::{Project, ProjectKind},
    strings, theme,
};

use self::{browser::Browser, settings::Settings};

const RECENT_LIMIT: usize = 30;

pub(super) enum Action {
    Open(PathBuf),
    Create(PathBuf, ProjectKind),
    Back,
    ApproveUpgrade,
    CancelUpgrade,
}

#[derive(Default)]
struct Catalog {
    file: Option<PathBuf>,
    paths: Vec<PathBuf>,
}

impl Catalog {
    fn open(file: Option<PathBuf>) -> Result<Self> {
        let paths = match &file {
            Some(file) => match fs::read(file) {
                Ok(bytes) => serde_json::from_slice(&bytes).context("Reading recent projects")?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                Err(error) => return Err(error.into()),
            },
            None => Vec::new(),
        };
        Ok(Self { file, paths })
    }

    fn touch(&mut self, path: PathBuf) -> Result<()> {
        self.update(move |paths| {
            paths.retain(|entry| *entry != path);
            paths.insert(0, path);
            paths.truncate(RECENT_LIMIT);
        })
    }

    fn forget(&mut self, path: &Path) -> Result<()> {
        self.update(|paths| paths.retain(|entry| entry != path))
    }

    fn update(&mut self, apply: impl FnOnce(&mut Vec<PathBuf>)) -> Result<()> {
        let Some(path) = &self.file else {
            apply(&mut self.paths);
            return Ok(());
        };
        let parent = path.parent().context("Missing project catalog directory")?;
        fs::create_dir_all(parent)?;
        let lock = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.with_extension("lock"))?;
        lock.lock()?;
        let mut paths = Self::open(Some(path.clone()))?.paths;
        apply(&mut paths);
        let mut staged = tempfile::NamedTempFile::new_in(parent)?;
        staged.write_all(&serde_json::to_vec_pretty(&paths)?)?;
        staged.persist(path)?;
        self.paths = paths;
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Dialog {
    Settings,
    Open,
    Create,
    Upgrade,
}

pub(super) struct Manager {
    dialog: Option<Dialog>,
    browser: Option<Browser>,
    catalog: Catalog,
    config: Option<PathBuf>,
    settings: Option<Settings>,
    suggested: PathBuf,
    path: String,
    name: String,
    kind: ProjectKind,
    filter: String,
    selected: Option<PathBuf>,
    pub error: Option<String>,
}

impl Manager {
    pub fn new() -> Self {
        let (catalog, mut error) = match Catalog::open(settings::catalog_file()) {
            Ok(catalog) => (catalog, None),
            Err(error) => (Catalog::default(), Some(format!("{error:#}"))),
        };
        let config = settings::config_file();
        let settings = config
            .as_deref()
            .and_then(|path| match Settings::load(path) {
                Ok(value) => value,
                Err(problem) => {
                    error = Some(format!("{} {problem:#}", strings::SETTINGS_READ_ERROR));
                    None
                }
            });
        Self {
            dialog: None,
            browser: None,
            catalog,
            config,
            settings,
            suggested: settings::suggested_directory(),
            path: String::new(),
            name: String::new(),
            kind: ProjectKind::TwoD,
            filter: String::new(),
            selected: None,
            error,
        }
    }

    fn default_directory(&self) -> &Path {
        self.settings.as_ref().map_or(&self.suggested, |settings| {
            &settings.default_project_directory
        })
    }

    pub fn prepare_upgrade(&mut self) {
        self.begin(Dialog::Upgrade);
    }

    fn begin(&mut self, dialog: Dialog) {
        self.path = self.default_directory().display().to_string();
        self.error = None;
        self.dialog = Some(dialog);
        self.name.clear();
    }

    pub fn remember(&mut self, project: &Project) {
        if project.path.is_file() && !project.prefab {
            self.selected = Some(project.path.clone());
            if let Err(error) = self.catalog.touch(project.path.clone()) {
                self.error = Some(format!("{error:#}"));
            }
        }
        self.browser = None;
        self.dialog = None;
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        can_return: bool,
        dirty: bool,
        upgrade: Option<(&Path, &str)>,
    ) -> Option<Action> {
        let first_run = self.settings.is_none() && self.config.is_some();
        if first_run && self.dialog.is_none() {
            self.path = self.default_directory().display().to_string();
            self.dialog = Some(Dialog::Settings);
        }
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, theme::VIEWPORT);
        let frame = egui::Frame::new().inner_margin(theme::PROJECT_MARGIN);
        let mut action = frame
            .show(ui, |ui| self.dashboard(ui, can_return, dirty))
            .inner;

        if let Some(dialog) = self.dialog {
            action = self.show_dialog(ui.ctx(), dialog, first_run, upgrade);
        } else if !dirty && action.is_none() {
            action = ui
                .input(|input| {
                    input
                        .raw
                        .dropped_files
                        .iter()
                        .find_map(|file| file.path.clone())
                })
                .map(Action::Open);
        }
        action
    }

    fn dashboard(&mut self, ui: &mut Ui, can_return: bool, dirty: bool) -> Option<Action> {
        let back = ui
            .horizontal(|ui| {
                ui.label(
                    RichText::new(strings::PROJECTS)
                        .size(theme::PROJECT_TITLE)
                        .strong(),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(strings::SETTINGS).clicked() {
                        self.begin(Dialog::Settings);
                    }
                    can_return && ui.button(strings::BACK_TO_EDITOR).clicked()
                })
                .inner
            })
            .inner;
        ui.separator();
        let project = ui
            .horizontal_top(|ui| {
                let edit = ui.vertical(|ui| self.actions(ui, dirty)).inner;
                let open = ui.vertical(|ui| self.recent(ui, dirty)).inner;
                edit.or(open)
            })
            .inner;
        if back {
            Some(Action::Back)
        } else {
            project.map(Action::Open)
        }
    }

    fn actions(&mut self, ui: &mut Ui, dirty: bool) -> Option<PathBuf> {
        ui.set_width(theme::PROJECT_ACTION_WIDTH);
        for (label, dialog) in [
            (strings::NEW_PROJECT, Dialog::Create),
            (strings::IMPORT_PROJECT, Dialog::Open),
        ] {
            let button = Button::new(label).min_size(egui::vec2(
                theme::PROJECT_ACTION_WIDTH,
                theme::PROJECT_BUTTON_HEIGHT,
            ));
            if ui.add_enabled(!dirty, button).clicked() {
                self.begin(dialog);
            }
        }
        ui.separator();
        let selected = self.selected.as_ref().filter(|path| path.is_file());
        if ui
            .add_enabled(
                !dirty && selected.is_some(),
                Button::new(strings::EDIT_PROJECT),
            )
            .clicked()
        {
            return selected.cloned();
        }
        if ui
            .add_enabled(
                self.selected.is_some(),
                Button::new(strings::FORGET_PROJECT),
            )
            .clicked()
            && let Some(path) = &self.selected
        {
            match self.catalog.forget(path) {
                Ok(()) => {
                    self.selected = None;
                    self.error = None;
                }
                Err(error) => self.error = Some(format!("{error:#}")),
            }
        }
        None
    }

    fn show_dialog(
        &mut self,
        ctx: &EguiContext,
        dialog: Dialog,
        first_run: bool,
        upgrade: Option<(&Path, &str)>,
    ) -> Option<Action> {
        let mut browse = false;
        let mut cancel = false;
        let mut submit = false;
        let modal = project_modal("project-action").show(ctx, |ui| {
            ui.set_width(theme::PROJECT_DIALOG_WIDTH);
            let (title, label) = match dialog {
                Dialog::Settings if first_run => (strings::FIRST_RUN, strings::CONTINUE),
                Dialog::Settings => (strings::SETTINGS, strings::SAVE),
                Dialog::Create => (strings::NEW_PROJECT, strings::CREATE_AND_EDIT),
                Dialog::Open => (strings::IMPORT_PROJECT, strings::IMPORT_AND_EDIT),
                Dialog::Upgrade => (strings::UPDATE_PROJECT_TYPES, strings::UPDATE_AND_EDIT),
            };
            ui.heading(title);
            if dialog == Dialog::Upgrade {
                if let Some((path, version)) = upgrade {
                    ui.label(path.display().to_string());
                    ui.colored_label(
                        theme::WARNING,
                        format!(
                            "{} {version} {} {}",
                            strings::SDK_VERSION,
                            strings::VERSION_TO,
                            env!("CARGO_PKG_VERSION")
                        ),
                    );
                    ui.label(strings::UPDATE_TYPES_WARNING);
                }
            } else {
                if dialog == Dialog::Create {
                    ui.label(RichText::new(strings::FIRST_SCENE).strong());
                    ui.horizontal(|ui| {
                        let card_width = ((ui.available_width() - 18.0) / 4.0).max(96.0);
                        for (kind, title, detail) in [
                            (ProjectKind::TwoD, "2D", strings::SCENE_2D_DETAIL),
                            (ProjectKind::ThreeD, "3D", strings::SCENE_3D_DETAIL),
                            (
                                ProjectKind::TwoDThreeD,
                                "2D + 3D",
                                strings::SCENE_MIXED_DETAIL,
                            ),
                            (
                                ProjectKind::Jumpy,
                                "Jumpy Bird",
                                strings::SCENE_JUMPY_DETAIL,
                            ),
                        ] {
                            let response = ui
                                .add_sized(
                                    [card_width, 54.0],
                                    Button::selectable(
                                        self.kind == kind,
                                        RichText::new(title).strong(),
                                    )
                                    .wrap(),
                                )
                                .on_hover_text(detail);
                            if response.clicked() {
                                self.kind = kind;
                            }
                        }
                    });
                    ui.label(match self.kind {
                        ProjectKind::TwoD => strings::SCENE_2D_DETAIL,
                        ProjectKind::ThreeD => strings::SCENE_3D_DETAIL,
                        ProjectKind::TwoDThreeD => strings::SCENE_MIXED_DETAIL,
                        ProjectKind::Jumpy => strings::SCENE_JUMPY_DETAIL,
                    });
                    ui.label(strings::PROJECT_NAME);
                    ui.add(TextEdit::singleline(&mut self.name).desired_width(f32::INFINITY));
                }
                ui.label(match dialog {
                    Dialog::Settings => strings::DEFAULT_PROJECT_FOLDER,
                    Dialog::Create => strings::PARENT_PROJECT_FOLDER,
                    _ => strings::PROJECT_PATH,
                });
                ui.horizontal(|ui| {
                    ui.add(
                        TextEdit::singleline(&mut self.path)
                            .desired_width(theme::PROJECT_ADDRESS_WIDTH),
                    );
                    browse = ui.button(strings::BROWSE).clicked();
                });
                if dialog == Dialog::Create
                    && let Ok(path) = creation_path(&self.path, &self.name)
                {
                    ui.label(RichText::new(path.display().to_string()).color(theme::TEXT));
                }
            }
            if let Some(error) = &self.error {
                ui.colored_label(theme::REJECT, error);
            }
            ui.separator();
            ui.horizontal(|ui| {
                let enabled = match dialog {
                    Dialog::Create => !self.name.trim().is_empty() && !self.path.trim().is_empty(),
                    Dialog::Upgrade => true,
                    _ => !self.path.trim().is_empty(),
                };
                submit = ui
                    .add_enabled(enabled, Button::new(label).fill(theme::PRIMARY))
                    .clicked();
                if dialog != Dialog::Settings || !first_run {
                    cancel = ui.button(strings::CANCEL).clicked();
                }
            });
        });
        if self.browser.is_none()
            && (cancel || ((dialog != Dialog::Settings || !first_run) && modal.should_close()))
        {
            self.dialog = None;
            self.error = None;
            return (dialog == Dialog::Upgrade).then_some(Action::CancelUpgrade);
        }
        if browse {
            self.browser = Some(Browser::new(
                PathBuf::from(self.path.trim()),
                dialog == Dialog::Open,
            ));
        }
        if let Some(browser) = &mut self.browser {
            if let Some(choice) = browser.show(ctx) {
                if let Some(path) = choice {
                    self.path = path.display().to_string();
                    self.error = None;
                }
                self.browser = None;
            }
            return None;
        }
        if !submit {
            return None;
        }
        self.error = None;
        match self.submit(dialog) {
            Ok(action) => action,
            Err(error) => {
                self.error = Some(format!("{error:#}"));
                None
            }
        }
    }

    fn submit(&mut self, dialog: Dialog) -> Result<Option<Action>> {
        let path = PathBuf::from(self.path.trim());
        Ok(match dialog {
            Dialog::Settings => {
                let file = self
                    .config
                    .as_deref()
                    .context(strings::SETTINGS_UNAVAILABLE)?;
                self.settings = Some(Settings::save(file, &path)?);
                self.dialog = None;
                None
            }
            Dialog::Create => Some(Action::Create(
                creation_path(&self.path, &self.name)?,
                self.kind,
            )),
            Dialog::Open => Some(Action::Open(path)),
            Dialog::Upgrade => Some(Action::ApproveUpgrade),
        })
    }

    fn recent(&mut self, ui: &mut Ui, dirty: bool) -> Option<PathBuf> {
        ui.label(RichText::new(strings::RECENT).strong());
        ui.add(
            TextEdit::singleline(&mut self.filter)
                .hint_text(strings::SEARCH_PROJECTS)
                .desired_width(f32::INFINITY),
        );
        if dirty {
            ui.colored_label(theme::WARNING, strings::SAVE_FIRST);
        }
        if self.dialog.is_none()
            && let Some(error) = &self.error
        {
            ui.colored_label(theme::REJECT, error);
        }
        ui.separator();
        if self.catalog.paths.is_empty() {
            ui.label(strings::NO_RECENT);
        }
        let query = self.filter.to_lowercase();
        egui::ScrollArea::vertical()
            .show(ui, |ui| {
                for path in &self.catalog.paths {
                    let full = path.display().to_string();
                    if !full.to_lowercase().contains(&query) {
                        continue;
                    }
                    let name = if crate::project::is_main_scene(path) {
                        path.parent().and_then(Path::file_name)
                    } else {
                        path.file_stem()
                    }
                    .map_or_else(|| full.clone(), |name| name.to_string_lossy().into_owned());
                    let row = theme::row(
                        ui,
                        RichText::new(name).strong(),
                        self.selected.as_ref() == Some(path),
                        egui::Sense::click(),
                    );
                    ui.label(RichText::new(full).small().color(theme::TEXT));
                    let exists = path.is_file();
                    if !exists {
                        ui.colored_label(theme::WARNING, strings::PROJECT_MISSING);
                    }
                    if row.clicked() {
                        self.selected = Some(path.clone());
                        self.error = None;
                    }
                    row.context_menu(|ui| {
                        if ui.button(strings::EXPLORER).clicked() {
                            self.error = crate::show_in_files(path)
                                .err()
                                .map(|error| format!("{error:#}"));
                            ui.close();
                        }
                    });
                    if !dirty && exists && row.double_clicked() {
                        return Some(path.clone());
                    }
                    ui.separator();
                }
                None
            })
            .inner
    }
}

fn project_modal(name: &str) -> Modal {
    let id = egui::Id::new(name);
    Modal::new(id).area(Modal::default_area(id).fade_in(false))
}

fn creation_path(folder: &str, name: &str) -> Result<PathBuf> {
    let name = name.trim();
    ensure!(
        !name.is_empty()
            && name != "."
            && name != ".."
            && !name.ends_with('.')
            && !name.contains(|c: char| c.is_control()
                || matches!(c, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*')),
        "{}",
        strings::INVALID_PROJECT_NAME
    );
    let folder = Path::new(folder.trim());
    ensure!(folder.is_absolute(), "{}", strings::ABSOLUTE_FOLDER);
    Ok(folder.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_projects_open_by_folder_and_keep_their_paths() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("main.aurora-scene");
        let mut old = Project::open(&path)?;
        old.save()?;
        let project = Project::open_existing(temp.path())?;
        assert_eq!(project.path, path.canonicalize()?);
        assert!(!temp.path().join("main.scene").exists());
        Ok(())
    }

    #[test]
    fn saved_settings_supply_the_parent_for_new_projects() -> Result<()> {
        let workspace = tempfile::tempdir()?;
        let default = workspace.path().join("Games");
        let mut manager = Manager::new();
        manager.config = Some(workspace.path().join(".aurora/config"));
        manager.path = default.display().to_string();
        manager.submit(Dialog::Settings)?;
        manager.begin(Dialog::Create);
        manager.name = "Night Flight".into();
        let Some(Action::Create(path, ProjectKind::TwoD)) = manager.submit(Dialog::Create)? else {
            anyhow::bail!("Create did not produce a project path");
        };
        assert_eq!(path, default.canonicalize()?.join("Night Flight"));
        manager.name = "../escape".into();
        assert!(manager.submit(Dialog::Create).is_err());
        Ok(())
    }

    #[test]
    fn create_dialog_preserves_the_selected_starter_kind() -> Result<()> {
        let workspace = tempfile::tempdir()?;
        for (name, kind) in [
            ("TwoD", ProjectKind::TwoD),
            ("ThreeD", ProjectKind::ThreeD),
            ("Mixed", ProjectKind::TwoDThreeD),
            ("Jumpy", ProjectKind::Jumpy),
        ] {
            let mut manager = Manager::new();
            manager.path = workspace.path().display().to_string();
            manager.name = name.into();
            manager.kind = kind;

            let Some(Action::Create(path, actual_kind)) = manager.submit(Dialog::Create)? else {
                anyhow::bail!("Create did not produce a project action");
            };
            assert_eq!(path, workspace.path().join(name));
            assert_eq!(actual_kind, kind);
        }
        Ok(())
    }

    #[test]
    fn project_starters_keep_2d_and_3d_nodes_composable() -> Result<()> {
        use engine_core::scene::spatial::Role;

        let workspace = tempfile::tempdir()?;
        let mut two_d = Project::create(&workspace.path().join("TwoD"), ProjectKind::TwoD)?;
        assert_eq!(two_d.document.nodes.len(), 1);
        assert_eq!(two_d.document.nodes[0].name, "Canvas");
        assert!(two_d.document.nodes[0].spatial.is_none());
        two_d.add("Cube 3D", Some(0));
        two_d.save()?;
        let two_d = Project::open_existing(&two_d.root)?;
        assert_eq!(
            two_d.document.nodes[1]
                .spatial
                .as_ref()
                .map(|spatial| spatial.role),
            Some(Role::Cube)
        );

        let mut three_d = Project::create(&workspace.path().join("ThreeD"), ProjectKind::ThreeD)?;
        let initial_nodes = [
            ("Node 3D", Role::Pivot),
            ("Cube 3D", Role::Cube),
            ("Camera 3D", Role::Camera),
            ("Directional Light", Role::DirectionalLight),
        ];
        assert_eq!(three_d.document.nodes.len(), initial_nodes.len());
        for (node, (name, role)) in three_d.document.nodes.iter().zip(initial_nodes) {
            assert_eq!(node.name, name);
            assert_eq!(
                node.spatial.as_ref().map(|spatial| spatial.role),
                Some(role)
            );
        }
        three_d.add("Canvas", Some(0));
        three_d.save()?;
        let three_d = Project::open_existing(&three_d.root)?;
        assert_eq!(three_d.document.nodes[4].name, "Canvas");
        assert!(three_d.document.nodes[4].spatial.is_none());

        let mixed = Project::create(&workspace.path().join("Mixed"), ProjectKind::TwoDThreeD)?;
        assert_eq!(
            mixed.document.scene_kind,
            engine_core::save::SceneKind::TwoDThreeD
        );
        assert!(mixed.document.nodes[0].spatial.is_none());
        assert!(mixed.document.nodes[1].spatial.is_some());
        assert_eq!(
            mixed.document.nodes[2]
                .spatial
                .as_ref()
                .map(|spatial| spatial.role),
            Some(Role::Cube)
        );
        Ok(())
    }

    #[test]
    fn show_in_files_resolves_scene_files_to_the_project_folder() -> Result<()> {
        let workspace = tempfile::tempdir()?;
        let project_folder = workspace.path().join("Project");
        fs::create_dir(&project_folder)?;
        let main_scene = project_folder.join("main.scene");
        fs::write(&main_scene, b"{}")?;
        assert_eq!(
            crate::folder_to_show(&main_scene)?,
            project_folder.canonicalize()?
        );
        assert_eq!(
            crate::folder_to_show(&project_folder)?,
            project_folder.canonicalize()?
        );
        Ok(())
    }

    #[test]
    fn create_open_and_remember_without_replacing_a_project() -> Result<()> {
        let workspace = tempfile::tempdir()?;
        let dir = workspace.path().join("Orbit");
        let created = Project::create(&dir, ProjectKind::TwoD)?;
        assert!(dir.join("assets").is_dir());
        assert!(dir.join("tsconfig.json").is_file());
        assert!(dir.join(".aurora/index.d.ts").is_file());
        assert!(created.path.ends_with("main.scene"));
        assert!(created.path.is_file());
        assert!(Project::create(&dir, ProjectKind::TwoD).is_err());
        assert!(Project::open_existing(workspace.path()).is_err());
        let opened = Project::open_existing(&dir)?;
        assert_eq!(opened.path, created.path);
        let file = Some(workspace.path().join("state/projects.json"));
        let mut catalog = Catalog::open(file.clone())?;
        let mut another_window = Catalog::open(file.clone())?;
        catalog.touch(opened.path.clone())?;
        let other = Project::create(&workspace.path().join("Second"), ProjectKind::TwoD)?;
        another_window.touch(other.path.clone())?;
        catalog.touch(created.path.clone())?;
        assert_eq!(catalog.paths, [created.path.clone(), other.path.clone()]);
        catalog.forget(&other.path)?;
        assert!(other.path.is_file());
        assert_eq!(Catalog::open(file)?.paths, [created.path]);
        Ok(())
    }
}
