//! Supervises the offscreen player and refreshes the Game view texture.

use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, SyncSender},
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Context, Result, ensure};
use egui::{ColorImage, Context as EguiContext, TextureHandle, TextureOptions};
use engine_core::save::{MAX_NODES, NodeFile, PrefabFile, PropertyValue, SceneFile, ScriptFile};

use crate::{
    project::{self, Project},
    strings,
};

/// Which part of the project Play starts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum PlayScope {
    /// Stage only the open scene.
    #[default]
    Scene,
    /// Stage every scene in the project, with the open scene first.
    Game,
}

/// Stable per-project key for host-only secret files outside the project tree.
fn project_key(root: &Path) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in root.to_string_lossy().bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Scene a Play session opens first: the configured default in Game scope.
fn opening_scene(root: &Path, scope: PlayScope, start_scene: &str, open: &Path) -> Option<PathBuf> {
    if scope == PlayScope::Game {
        project::valid_scene_ref(root, start_scene).or_else(|| Some(open.to_path_buf()))
    } else {
        Some(open.to_path_buf())
    }
}

/// Rewrites project-relative asset paths to absolute paths for a staged scene.
fn rebase_paths(document: &mut SceneFile, project: &Project) {
    for node in &mut document.nodes {
        if let Some(path) = node.visual.as_mut().and_then(|visual| visual.asset_mut()) {
            *path = project.root.join(&*path);
        }
        if let Some(sprite) = &mut node.sprite {
            sprite.png = project.root.join(&sprite.png);
        }
    }
    for instance in &mut document.instances {
        instance.prefab = project.root.join(&instance.prefab);
    }
    if let Some(gameplay) = &mut document.gameplay {
        for path in gameplay
            .sounds
            .iter_mut()
            .chain(gameplay.frames.iter_mut().flat_map(|frame| &mut frame.png))
        {
            *path = project.root.join(&*path);
        }
    }
}

pub(super) struct Preview {
    run: Option<Run>,
    pending: Option<Receiver<Result<Run>>>,
    pub texture: Option<TextureHandle>,
    pub paused: bool,
    pub game_view: bool,
    pub reload_at: Option<Instant>,
    logs: Receiver<String>,
    sender: SyncSender<String>,
}

struct Run {
    child: Child,
    directory: tempfile::TempDir,
    last_update: Option<SystemTime>,
    size: [usize; 2],
}

impl Default for Preview {
    fn default() -> Self {
        let (sender, logs) = mpsc::sync_channel(256);
        Self {
            run: None,
            pending: None,
            texture: None,
            paused: false,
            game_view: false,
            reload_at: None,
            logs,
            sender,
        }
    }
}

impl Preview {
    pub fn is_playing(&self) -> bool {
        self.run.is_some() || self.pending.is_some()
    }

    pub fn start(&mut self, project: &Project, scope: PlayScope, start_scene: &str) {
        let (send, receive) = mpsc::sync_channel(1);
        let project = project.clone();
        let start_scene = start_scene.to_owned();
        let logs = self.sender.clone();
        self.pending = Some(receive);
        self.game_view = true;
        std::thread::spawn(move || {
            let _ = send.send(Self::launch(&project, scope, &start_scene, logs));
        });
    }

    fn launch(
        project: &Project,
        scope: PlayScope,
        start_scene: &str,
        logs: SyncSender<String>,
    ) -> Result<Run> {
        let mut project = project.clone();
        let chosen = opening_scene(&project.root, scope, start_scene, &project.path);
        if let Some(path) = &chosen
            && *path != project.path
        {
            project.document =
                SceneFile::load(path).with_context(|| format!("Loading {}", path.display()))?;
            project.path = path.clone();
        }
        let chosen_relative = chosen
            .as_ref()
            .and_then(|path| path.strip_prefix(&project.root).ok())
            .map(|path| path.to_string_lossy().into_owned());
        let directory = tempfile::tempdir()?;
        let location = directory.path();
        let mut document = project.document.clone();
        rebase_paths(&mut document, &project);
        let mut scenes = vec![location.join("preview.scene")];
        let mut scene_names = vec![scene_name(&project.path)];
        if scope == PlayScope::Game {
            for reference in project::scene_references(&project.root) {
                if chosen_relative.as_deref() == Some(reference.as_str()) {
                    continue;
                }
                let source = project.root.join(&reference);
                let Ok(mut other) = SceneFile::load(&source) else {
                    continue;
                };
                rebase_paths(&mut other, &project);
                let destination = location.join(format!("game-{}.scene", scenes.len()));
                if other.save(&destination).is_err() {
                    continue;
                }
                scenes.push(destination);
                scene_names.push(
                    Path::new(&reference)
                        .file_stem()
                        .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
                );
            }
        }
        let nodes = binding_nodes(&project)?;
        let prefabs = freeze_script_prefabs(script_prefabs(&project, &nodes)?, location)?;
        document.script_prefabs = prefabs.values().cloned().collect();
        let scene = location.join("preview.scene");
        document.save(&scene)?;
        let engine_root = crate::engine_resources()?;
        let player = std::env::current_exe()?.with_file_name("aurora-player");
        let mut command = Command::new(player);
        command.arg("--preview-dir").arg(location);
        for scene in &scenes {
            command.arg("--scene").arg(scene);
        }
        command
            .arg("--asset-cache")
            .arg(project.root.join(".aurora/import"));
        if document.gameplay.is_some() {
            command
                .arg("--profile")
                .arg(project.root.join(".aurora/best-score"));
        }
        command
            .arg("--config")
            .arg(project.root.join(".aurora/game-config.json"));
        if let Some(user) = crate::project_manager::settings::config_file()
            .and_then(|path| path.parent().map(Path::to_path_buf))
        {
            command.arg("--secrets").arg(
                user.join("secrets")
                    .join(format!("{}.json", project_key(&project.root))),
            );
        }
        let (entry, bindings) = script_entry_for(
            &project,
            &nodes,
            &engine_root.join("scripts/build/dev.ts"),
            &prefabs,
            &scene_names,
            |path| {
                let relative = path
                    .canonicalize()?
                    .strip_prefix(project.root.canonicalize()?)
                    .context("Script lies outside the project")?
                    .to_path_buf();
                Ok(location.join("project").join(relative))
            },
        )?;
        if !bindings.is_empty() {
            stage_project_scripts(&project.root, &location.join("project"))?;
            fs::write(location.join("entry.ts"), entry)?;
            fs::write(
                location.join("protocol.mts"),
                codegen::protocol_typescript()?,
            )?;
            let sdk = serde_json::to_string(&engine_root.join("scripts/aurora/index.ts"))?;
            fs::write(
                location.join("tsconfig.json"),
                format!(
                    r#"{{"compilerOptions":{{"experimentalDecorators":true,"paths":{{"aurora":[{sdk}],"aurora-cc":[{sdk}]}}}}}}"#
                ),
            )?;
            let output = Command::new(engine_root.join("tools/build-editor-script.sh"))
                .arg(location)
                .output()
                .context("Building project script")?;
            if !output.status.success() {
                anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr));
            }
            command
                .arg("--script-dev")
                .arg(location.join("game-script"));
            for node in bindings {
                command.arg("--script-node").arg(node.to_string());
            }
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Launch aurora-player from the editor's binary directory")?;
        if let Some(stdout) = child.stdout.take() {
            log_pipe(stdout, logs.clone());
        }
        if let Some(stderr) = child.stderr.take() {
            log_pipe(stderr, logs);
        }
        Ok(Run {
            child,
            directory,
            last_update: None,
            size: document
                .display
                .unwrap_or_default()
                .preview_resolution()
                .map(|v| v as usize),
        })
    }

    pub fn input(&mut self, byte: u8) {
        if let Some(input) = self.run.as_mut().and_then(|run| run.child.stdin.as_mut()) {
            let _ = input.write_all(&[byte]);
        }
    }

    /// Sends a pointer press, keeping the existing Game view tap entry point.
    pub fn touch(&mut self, position: [f32; 2]) {
        self.pointer(b'm', position);
    }

    /// Sends pointer movement while the Game view drag is active.
    pub fn pointer_move(&mut self, position: [f32; 2]) {
        self.pointer(b'n', position);
    }

    /// Sends a pointer release at the given Game view position.
    pub fn pointer_release(&mut self, position: [f32; 2]) {
        self.pointer(b'o', position);
    }

    /// Starts a Game view touch contact with a stable editor-local id.
    pub fn touch_point(&mut self, id: u16, position: [f32; 2]) {
        self.touch_pointer(b'M', id, position);
    }

    /// Moves a Game view touch contact.
    pub fn touch_move(&mut self, id: u16, position: [f32; 2]) {
        self.touch_pointer(b'N', id, position);
    }

    /// Releases a Game view touch contact.
    pub fn touch_end(&mut self, id: u16, position: [f32; 2]) {
        self.touch_pointer(b'O', id, position);
    }

    /// Cancels one Game view touch contact.
    pub fn touch_cancel(&mut self, id: u16) {
        let [idl, idh] = id.to_le_bytes();
        self.write_input(&[b'C', idl, idh]);
    }

    /// Cancels every Game view contact, such as when the editor window loses focus.
    pub fn cancel_pointers(&mut self) {
        self.write_input(b"F");
    }

    /// Sends mouse wheel deltas at the given Game view position.
    pub fn wheel(&mut self, position: [f32; 2], delta: [f32; 2]) {
        let x = (position[0].clamp(0.0, 1.0) * f32::from(u16::MAX)).round() as u16;
        let y = (position[1].clamp(0.0, 1.0) * f32::from(u16::MAX)).round() as u16;
        let dx = delta[0]
            .round()
            .clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16;
        let dy = delta[1]
            .round()
            .clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16;
        let mut bytes = Vec::with_capacity(9);
        bytes.push(b'w');
        bytes.extend_from_slice(&x.to_le_bytes());
        bytes.extend_from_slice(&y.to_le_bytes());
        bytes.extend_from_slice(&dx.to_le_bytes());
        bytes.extend_from_slice(&dy.to_le_bytes());
        self.write_input(&bytes);
    }

    fn touch_pointer(&mut self, command: u8, id: u16, position: [f32; 2]) {
        let x = (position[0].clamp(0.0, 1.0) * f32::from(u16::MAX)).round() as u16;
        let y = (position[1].clamp(0.0, 1.0) * f32::from(u16::MAX)).round() as u16;
        let [xl, xh] = x.to_le_bytes();
        let [yl, yh] = y.to_le_bytes();
        let [idl, idh] = id.to_le_bytes();
        self.write_input(&[command, xl, xh, yl, yh, idl, idh]);
    }

    fn pointer(&mut self, command: u8, position: [f32; 2]) {
        let x = (position[0].clamp(0.0, 1.0) * f32::from(u16::MAX)).round() as u16;
        let y = (position[1].clamp(0.0, 1.0) * f32::from(u16::MAX)).round() as u16;
        let [xl, xh] = x.to_le_bytes();
        let [yl, yh] = y.to_le_bytes();
        self.write_input(&[command, xl, xh, yl, yh]);
    }

    fn write_input(&mut self, bytes: &[u8]) {
        if let Some(input) = self.run.as_mut().and_then(|run| run.child.stdin.as_mut()) {
            let _ = input.write_all(bytes);
        }
    }

    /// Sends a physical key release through the preview input channel.
    pub fn key_up(&mut self, key: engine_core::input::Key) {
        self.write_input(&[0x02, key.preview_byte()]);
    }

    pub fn pause(&mut self) {
        if self.run.is_some() {
            self.paused = !self.paused;
            self.input(b'p');
        }
    }

    pub fn stop(&mut self) {
        self.pending.take();
        self.run.take();
        self.texture = None;
        self.paused = false;
        self.reload_at = None;
    }

    pub fn poll(&mut self, ctx: &EguiContext, output: &mut Vec<String>) {
        output.extend(self.logs.try_iter());
        if let Some(pending) = &self.pending {
            match pending.try_recv() {
                Ok(result) => {
                    self.pending = None;
                    match result {
                        Ok(run) => {
                            self.run = Some(run);
                            self.paused = false;
                            output.push(strings::STARTED.into());
                        }
                        Err(error) => output.push(format!("Error: {error:#}")),
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.pending = None;
                    output.push(strings::LAUNCH_FAILED.into());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
            ctx.request_repaint_after(Duration::from_millis(40));
        }
        if let Some(run) = &mut self.run {
            if let Ok(Some(status)) = run.child.try_wait() {
                output.push(format!("{} {status}", strings::PLAYER_EXITED));
                self.stop();
                return;
            }
            let file = run.directory.path().join("frame.rgba");
            let modified = fs::metadata(&file)
                .and_then(|metadata| metadata.modified())
                .ok();
            if modified.is_some()
                && modified != run.last_update
                && let Ok(bytes) = fs::read(&file)
                && bytes.len() == run.size[0] * run.size[1] * 4
            {
                run.last_update = modified;
                let image = ColorImage::from_rgba_unmultiplied(run.size, &bytes);
                if let Some(texture) = &mut self.texture {
                    texture.set(image, TextureOptions::LINEAR);
                } else {
                    self.texture =
                        Some(ctx.load_texture("game-preview", image, TextureOptions::LINEAR));
                }
            }
            ctx.request_repaint_after(Duration::from_millis(40));
        }
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        // Closing the pipe lets the player drop its own supervised game script first.
        self.child.stdin.take();
        for _ in 0..20 {
            if self.child.try_wait().is_ok_and(|status| status.is_some()) {
                return;
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn log_pipe(pipe: impl std::io::Read + Send + 'static, queue: SyncSender<String>) {
    std::thread::spawn(move || {
        for line in BufReader::new(pipe).lines().map_while(Result::ok) {
            let _ = queue.try_send(line);
        }
    });
}

pub(super) fn binding_nodes(project: &Project) -> Result<Vec<NodeFile>> {
    let mut nodes = project.document.nodes.clone();
    for instance in &project.document.instances {
        let path = project.root.join(&instance.prefab);
        let directory = path
            .parent()
            .context("Prefab has no containing directory")?;
        let mut prefab = PrefabFile::load(&path)?;
        let offset = nodes.len();
        let count = prefab.nodes.len();
        ensure!(
            count <= MAX_NODES.saturating_sub(offset),
            "Too many scripted scene nodes"
        );
        for node in &mut prefab.nodes {
            node.parent = node
                .parent
                .map(|parent| offset + parent)
                .or(instance.parent);
            for script in &mut node.scripts {
                script.path = directory.join(&script.path);
                for value in script.values.values_mut() {
                    match value {
                        PropertyValue::Node(index) => {
                            ensure!(
                                *index < count,
                                "Prefab script node reference is out of bounds"
                            );
                            *index += offset;
                        }
                        PropertyValue::Component { node, .. } => {
                            ensure!(
                                *node < count,
                                "Prefab script component reference is out of bounds"
                            );
                            *node += offset;
                        }
                        PropertyValue::Prefab(path)
                        | PropertyValue::AudioClip(path)
                        | PropertyValue::SpriteFrame(path) => *path = directory.join(&*path),
                        PropertyValue::Prefabs(paths)
                        | PropertyValue::AudioClips(paths)
                        | PropertyValue::SpriteFrames(paths) => {
                            for path in paths {
                                *path = directory.join(&*path);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        nodes.extend(prefab.nodes);
    }
    Ok(nodes)
}

pub(super) fn script_prefabs(
    project: &Project,
    nodes: &[NodeFile],
) -> Result<BTreeMap<PathBuf, PathBuf>> {
    script_prefabs_within(project, nodes, &project.root)
}

pub(super) fn script_prefabs_within(
    project: &Project,
    nodes: &[NodeFile],
    scope: &Path,
) -> Result<BTreeMap<PathBuf, PathBuf>> {
    let root = &project.root;
    let mut prefabs = BTreeMap::new();
    let mut pending: Vec<_> = project
        .assets()
        .into_iter()
        .chain(
            project
                .document
                .script_prefabs
                .iter()
                .map(|path| root.join(path)),
        )
        .chain(prefab_properties(nodes).map(|path| root.join(path)))
        .collect();
    let mut visited = std::collections::BTreeSet::new();
    while let Some(path) = pending.pop() {
        if !project::is_prefab(&path) || !path.is_file() {
            continue;
        }
        let resolved = path.canonicalize()?;
        if !resolved.starts_with(scope) {
            continue;
        }
        if visited.insert(resolved.clone()) {
            let prefab = PrefabFile::load(&resolved)?;
            let directory = resolved
                .parent()
                .context("Prefab has no containing directory")?;
            pending.extend(prefab_properties(&prefab.nodes).map(|path| directory.join(path)));
        }
        prefabs.insert(project.relative(&path), resolved);
    }
    Ok(prefabs)
}

fn freeze_script_prefabs(
    mut prefabs: BTreeMap<PathBuf, PathBuf>,
    destination: &Path,
) -> Result<BTreeMap<PathBuf, PathBuf>> {
    if prefabs.is_empty() {
        return Ok(prefabs);
    }
    let staged = destination.join("prefabs");
    fs::create_dir(&staged)?;
    for (asset, source) in prefabs.values_mut().enumerate() {
        let directory = source
            .parent()
            .context("Prefab has no containing directory")?;
        let mut prefab = PrefabFile::load(source)?;
        for node in &mut prefab.nodes {
            if let Some(path) = node.visual.as_mut().and_then(|visual| visual.asset_mut()) {
                *path = directory.join(&*path);
            }
            if let Some(sprite) = &mut node.sprite {
                sprite.png = directory.join(&sprite.png);
            }
            for script in &mut node.scripts {
                script.path = directory.join(&script.path);
                for value in script.values.values_mut() {
                    match value {
                        PropertyValue::Prefab(path)
                        | PropertyValue::AudioClip(path)
                        | PropertyValue::SpriteFrame(path) => *path = directory.join(&*path),
                        PropertyValue::Prefabs(paths)
                        | PropertyValue::AudioClips(paths)
                        | PropertyValue::SpriteFrames(paths) => {
                            for path in paths {
                                *path = directory.join(&*path);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        let snapshot = staged.join(format!("{asset}.prefab"));
        prefab.save(&snapshot)?;
        // The generated factories and player's dynamic loader share this snapshot.
        *source = snapshot;
    }
    Ok(prefabs)
}

fn prefab_properties(nodes: &[NodeFile]) -> impl Iterator<Item = &Path> {
    nodes
        .iter()
        .flat_map(|node| &node.scripts)
        .flat_map(|script| script.values.values())
        .flat_map(|value| match value {
            PropertyValue::Prefab(path) => vec![path.as_path()],
            PropertyValue::Prefabs(paths) => paths.iter().map(PathBuf::as_path).collect(),
            _ => Vec::new(),
        })
}

#[cfg(test)]
fn script_entry(project: &Project, engine_root: &Path) -> Result<(String, Vec<usize>)> {
    let nodes = binding_nodes(project)?;
    script_entry_for(
        project,
        &nodes,
        &engine_root.join("scripts/build/dev.ts"),
        &script_prefabs(project, &nodes)?,
        &[scene_name(&project.path)],
        |path| Ok(path.to_path_buf()),
    )
}

fn stage_project_scripts(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == "node_modules" || name == "target" || name.to_string_lossy().starts_with('.') {
            continue;
        }
        let target = destination.join(&name);
        if entry.file_type()?.is_dir() {
            stage_project_scripts(&entry.path(), &target)?;
        } else if entry.file_type()?.is_file()
            && matches!(
                entry.path().extension().and_then(|value| value.to_str()),
                Some("ts" | "mts" | "cts")
            )
        {
            let source = fs::read_to_string(entry.path())?;
            let lowered = codegen::lower_runtime(&source)
                .with_context(|| format!("Lowering {}", entry.path().display()))?;
            fs::write(target, lowered)?;
        }
    }
    let node_modules = source.join("node_modules");
    if node_modules.is_dir() {
        let staged_modules = destination.join("node_modules");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&node_modules, &staged_modules)?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&node_modules, &staged_modules)?;
    }
    Ok(())
}

pub(super) fn script_entry_for(
    project: &Project,
    nodes: &[NodeFile],
    launch_path: &Path,
    prefabs: &BTreeMap<PathBuf, PathBuf>,
    scene_names: &[String],
    mut source_path: impl FnMut(&Path) -> Result<PathBuf>,
) -> Result<(String, Vec<usize>)> {
    use std::fmt::Write as _;

    let launch = serde_json::to_string(launch_path)?;
    let mut entry = format!(
        "import {{ launchMany }} from {launch};\nimport {{ Component as AuroraComponent, NodeRef, TextRef, ImageRef, Prefab, bindScriptScene, registerScriptPrefab, registerScriptScenes, registerComponentFactory, registerComponentClass }} from \"aurora\";\n"
    );
    writeln!(
        entry,
        "registerScriptScenes({});",
        serde_json::to_string(scene_names)?
    )?;
    let prefab_paths = prefabs
        .keys()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .collect::<Vec<_>>();
    crate::script_assets::write_catalog(&mut entry, project, &prefab_paths)?;
    let mut attachments = Vec::new();
    for (node, content) in nodes.iter().enumerate() {
        for attachment in &content.scripts {
            let path = project.root.join(&attachment.path);
            let component = component_metadata(&path, attachment)?;
            attachments.push((node, attachment, path, component));
        }
    }
    // Stable sort preserves document/attachment order for equal priorities.
    attachments.sort_by_key(|(_, _, _, metadata)| metadata.order);
    let prefab_ids: BTreeMap<_, _> = prefabs
        .keys()
        .enumerate()
        .map(|(index, key)| (key.clone(), index))
        .collect();
    let mut bindings = Vec::with_capacity(attachments.len());
    let mut names = Vec::with_capacity(attachments.len());
    let mut generated = Vec::with_capacity(attachments.len());
    let mut component_refs: BTreeMap<(usize, String), String> = BTreeMap::new();
    for (node, attachment, path, component) in attachments {
        let index = bindings.len();
        let alias = format!("Component{index}");
        write_import(
            &mut entry,
            &component,
            &attachment.component,
            &alias,
            &source_path(&path)?,
        )?;
        let instance = format!("instance{index}");
        writeln!(entry, "const {instance} = new {alias}();")?;
        component_refs.insert((node, attachment.component.clone()), instance.clone());
        bindings.push(node);
        names.push(&attachment.component);
        generated.push((component, attachment, instance));
    }
    for (component, attachment, instance) in &generated {
        write_properties(
            &mut entry,
            project,
            component,
            attachment,
            &project.root,
            &prefab_ids,
            instance,
            nodes,
            None,
            &component_refs,
        )?;
    }
    let instances = (0..bindings.len())
        .map(|index| format!("instance{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    if !bindings.is_empty() {
        let parents: Vec<_> = nodes
            .iter()
            .map(|node| node.parent.map_or(-1, |parent| parent as i64))
            .collect();
        let names_by_node: Vec<_> = nodes.iter().map(|node| &node.name).collect();
        let text_contents = script_text_contents(nodes);
        writeln!(
            entry,
            "bindScriptScene({}, {}, [{instances}], {}, {}, {}, {});",
            serde_json::to_string(&names_by_node)?,
            serde_json::to_string(&parents)?,
            serde_json::to_string(&bindings)?,
            serde_json::to_string(&names)?,
            serde_json::to_string(&prefabs.keys().collect::<Vec<_>>())?,
            serde_json::to_string(&text_contents)?,
        )?;
    }
    if !bindings.is_empty() {
        let mut factories = BTreeMap::new();
        for path in project.assets() {
            if project::extension(&path) != "ts" || !path.is_file() {
                continue;
            }
            let source = fs::read_to_string(&path)?;
            let Ok(components) = codegen::extract(&source) else {
                continue;
            };
            for (name, component) in components {
                ensure!(
                    factories
                        .insert(name.clone(), (path.clone(), component))
                        .is_none(),
                    "Duplicate component factory name {name}"
                );
            }
        }
        for (index, (name, (path, component))) in factories.iter().enumerate() {
            let alias = format!("RuntimeComponent{index}");
            let factory = format!("runtimeComponentFactory{index}");
            write_import(&mut entry, component, name, &alias, &source_path(path)?)?;
            writeln!(
                entry,
                "function {factory}(_references: number[]): AuroraComponent {{ return new {alias}(); }}"
            )?;
            writeln!(
                entry,
                "registerComponentFactory({}, {factory});",
                serde_json::to_string(name)?
            )?;
            writeln!(
                entry,
                "registerComponentClass({alias}, {});",
                serde_json::to_string(name)?
            )?;
        }
        for (asset, path) in prefabs.values().enumerate() {
            write_prefab(
                &mut entry,
                project,
                &prefab_ids,
                asset,
                path,
                &mut source_path,
            )?;
        }
    }
    writeln!(entry, "launchMany([{instances}]);")?;
    Ok((entry, bindings))
}

fn scene_name(path: &Path) -> String {
    path.file_stem()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}

fn script_text_contents(nodes: &[NodeFile]) -> Vec<Option<String>> {
    nodes
        .iter()
        .map(|node| match &node.visual {
            Some(engine_core::save::visual::Visual::Text { spans, .. }) => {
                Some(spans.iter().map(|span| span.text.as_str()).collect())
            }
            _ => None,
        })
        .collect()
}

fn component_metadata(path: &Path, attachment: &ScriptFile) -> Result<codegen::Metadata> {
    let mut metadata = codegen::extract(&fs::read_to_string(path)?)
        .with_context(|| format!("Reading component metadata from {}", path.display()))?;
    metadata.remove(&attachment.component).with_context(|| {
        format!(
            "Missing component {} in {}",
            attachment.component,
            path.display()
        )
    })
}

fn write_import(
    entry: &mut String,
    component: &codegen::Metadata,
    name: &str,
    alias: &str,
    path: &Path,
) -> Result<()> {
    use std::fmt::Write as _;
    let exported = component.export.as_deref().unwrap_or(name);
    ensure!(
        identifier(exported),
        "Invalid exported class name {exported}"
    );
    writeln!(
        entry,
        "import {{ {exported} as {alias} }} from {};",
        serde_json::to_string(path)?
    )?;
    Ok(())
}

fn write_prefab(
    entry: &mut String,
    project: &Project,
    prefab_ids: &BTreeMap<PathBuf, usize>,
    asset: usize,
    path: &Path,
    source_path: &mut impl FnMut(&Path) -> Result<PathBuf>,
) -> Result<()> {
    use std::fmt::Write as _;
    let prefab = PrefabFile::load(path)?;
    let directory = path
        .parent()
        .context("Prefab has no containing directory")?;
    // One builder creates every component of a clone so component references
    // resolve before any instance starts running. The script binds the array to
    // the clone's node references.
    let builder = format!("prefabBuild{asset}");
    let mut names = Vec::new();
    let mut prepared = Vec::new();
    for (node, attachment) in prefab
        .nodes
        .iter()
        .enumerate()
        .flat_map(|(node, content)| content.scripts.iter().map(move |script| (node, script)))
    {
        let ordinal = prepared.len();
        let source = directory.join(&attachment.path);
        let component = component_metadata(&source, attachment)?;
        let alias = format!("PrefabComponent{asset}_{ordinal}");
        // Imports and class registrations stay at module scope; the builder only
        // constructs instances so repeated clones share no component state.
        write_import(
            entry,
            &component,
            &attachment.component,
            &alias,
            &source_path(&source)?,
        )?;
        writeln!(
            entry,
            "registerComponentClass({alias}, {});",
            serde_json::to_string(&attachment.component)?
        )?;
        names.push(&attachment.component);
        prepared.push((node, component, attachment, alias));
    }
    writeln!(
        entry,
        "function {builder}(references: number[]): AuroraComponent[] {{"
    )?;
    writeln!(entry, "const instances: AuroraComponent[] = [];")?;
    let mut built = Vec::new();
    let mut component_refs: BTreeMap<(usize, String), String> = BTreeMap::new();
    for (ordinal, (node, component, attachment, alias)) in prepared.iter().enumerate() {
        let instance = format!("instance{ordinal}");
        writeln!(entry, "const {instance} = new {alias}();")?;
        writeln!(entry, "instances.push({instance});")?;
        component_refs.insert((*node, attachment.component.clone()), instance.clone());
        built.push((component, attachment, instance));
    }
    for (component, attachment, instance) in &built {
        write_properties(
            entry,
            project,
            component,
            attachment,
            directory,
            prefab_ids,
            instance,
            &prefab.nodes,
            Some(prefab.nodes.len()),
            &component_refs,
        )?;
    }
    writeln!(entry, "return instances;\n}}")?;
    let node_names: Vec<_> = prefab.nodes.iter().map(|node| &node.name).collect();
    let text_contents = script_text_contents(&prefab.nodes);
    writeln!(
        entry,
        "registerScriptPrefab({asset}, {}, {builder}, {}, {});",
        serde_json::to_string(&node_names)?,
        serde_json::to_string(&names)?,
        serde_json::to_string(&text_contents)?,
    )?;
    Ok(())
}

fn resource_key(project: &Project, path: &Path) -> Result<String> {
    let relative = if path.is_absolute() {
        project.relative(path)
    } else {
        path.to_path_buf()
    };
    let text = relative
        .to_str()
        .context("Script resource path must be UTF-8")?;
    Ok(text.replace(std::path::MAIN_SEPARATOR, "/"))
}

fn gameplay_key(project: &Project, path: &Path) -> Result<String> {
    let relative = if path.is_absolute() {
        project.relative(path)
    } else {
        path.to_path_buf()
    };
    Ok(relative
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/"))
}

/// Asset properties can only resolve assets the scene already registers in a
/// sound or frame bank; anything else stays at the component's field default.
fn audio_registered(project: &Project, key: &str) -> bool {
    project.document.gameplay.as_ref().is_some_and(|gameplay| {
        gameplay
            .sounds
            .iter()
            .any(|path| gameplay_key(project, path).is_ok_and(|candidate| candidate == key))
    })
}

fn frame_registered(project: &Project, key: &str) -> bool {
    project.document.gameplay.as_ref().is_some_and(|gameplay| {
        gameplay
            .frames
            .iter()
            .flat_map(|bank| &bank.png)
            .any(|path| gameplay_key(project, path).is_ok_and(|candidate| candidate == key))
    })
}

#[allow(clippy::too_many_arguments)]
fn write_properties(
    entry: &mut String,
    project: &Project,
    component: &codegen::Metadata,
    attachment: &ScriptFile,
    directory: &Path,
    prefab_ids: &BTreeMap<PathBuf, usize>,
    instance: &str,
    scene_nodes: &[NodeFile],
    prefab_nodes: Option<usize>,
    component_refs: &BTreeMap<(usize, String), String>,
) -> Result<()> {
    use std::fmt::Write as _;
    for (key, value) in &attachment.values {
        let Some(spec) = component.props.get(key) else {
            continue;
        };
        if let (Some(filter), PropertyValue::Node(index)) = (spec.node_filter, value)
            && !scene_nodes
                .get(*index)
                .is_some_and(|node| node_matches_filter(filter, node))
        {
            continue;
        };
        if !crate::inspector::value_matches(spec.kind, value) {
            continue;
        }
        let value = match value {
            PropertyValue::Number(value) => serde_json::to_string(value)?,
            PropertyValue::Integer(value) => serde_json::to_string(value)?,
            PropertyValue::Boolean(value) => serde_json::to_string(value)?,
            PropertyValue::String(value) => serde_json::to_string(value)?,
            PropertyValue::Text(value) | PropertyValue::Enum(value) => {
                serde_json::to_string(value)?
            }
            PropertyValue::Vector2(value) => serde_json::to_string(value)?,
            PropertyValue::Vector3(value) => serde_json::to_string(value)?,
            PropertyValue::Size(value) => serde_json::to_string(value)?,
            PropertyValue::Rect(value) => serde_json::to_string(value)?,
            PropertyValue::Bitmask(value) => serde_json::to_string(value)?,
            PropertyValue::Color(value) => serde_json::to_string(value)?,
            PropertyValue::Node(value) => {
                let reference = if let Some(count) = prefab_nodes {
                    ensure!(
                        *value < count,
                        "Prefab script node reference is out of bounds"
                    );
                    format!("references[{value}]")
                } else {
                    value.to_string()
                };
                if component.export.is_some() {
                    let reference_type = match spec.node_filter {
                        Some(codegen::NodeFilter::Text) => "TextRef",
                        Some(codegen::NodeFilter::Image) => "ImageRef",
                        Some(codegen::NodeFilter::Design) => "DesignRef",
                        None => "NodeRef",
                    };
                    format!("new {reference_type}({reference}, {instance}.scene)")
                } else {
                    reference
                }
            }
            PropertyValue::Prefab(value) => {
                let absolute = directory.join(value);
                if component.export.is_some() {
                    let key = project.relative(&absolute);
                    let asset = prefab_ids
                        .get(&key)
                        .map_or(String::from("-1"), |id| id.to_string());
                    format!("new Prefab({}, {asset})", serde_json::to_string(&key)?)
                } else {
                    serde_json::to_string(&absolute)?
                }
            }
            PropertyValue::Prefabs(values) => {
                if component.export.is_none() {
                    let absolute = values
                        .iter()
                        .map(|value| directory.join(value))
                        .collect::<Vec<_>>();
                    serde_json::to_string(&absolute)?
                } else {
                    let items = values
                        .iter()
                        .map(|value| {
                            let absolute = directory.join(value);
                            let key = project.relative(&absolute);
                            let asset = prefab_ids
                                .get(&key)
                                .map_or(String::from("-1"), |id| id.to_string());
                            Ok(format!(
                                "new Prefab({}, {asset})",
                                serde_json::to_string(&key)?
                            ))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    format!("[{}]", items.join(", "))
                }
            }
            PropertyValue::AudioClips(values) => {
                if component.export.is_none() {
                    let absolute = values
                        .iter()
                        .map(|value| directory.join(value))
                        .collect::<Vec<_>>();
                    serde_json::to_string(&absolute)?
                } else {
                    let items = values
                        .iter()
                        .filter_map(|value| {
                            let key = resource_key(project, &directory.join(value)).ok()?;
                            audio_registered(project, &key)
                                .then(|| format!("{instance}.resources.requireAudioClip({key:?})"))
                        })
                        .collect::<Vec<_>>();
                    format!("[{}]", items.join(", "))
                }
            }
            PropertyValue::SpriteFrames(values) => {
                if component.export.is_none() {
                    let absolute = values
                        .iter()
                        .map(|value| directory.join(value))
                        .collect::<Vec<_>>();
                    serde_json::to_string(&absolute)?
                } else {
                    let items = values
                        .iter()
                        .filter_map(|value| {
                            let key = resource_key(project, &directory.join(value)).ok()?;
                            frame_registered(project, &key).then(|| {
                                format!("{instance}.resources.requireSpriteFrame({key:?})")
                            })
                        })
                        .collect::<Vec<_>>();
                    format!("[{}]", items.join(", "))
                }
            }
            PropertyValue::AudioClip(value) => {
                let absolute = directory.join(value);
                if component.export.is_none() {
                    serde_json::to_string(&absolute)?
                } else {
                    let key = resource_key(project, &absolute)?;
                    if !audio_registered(project, &key) {
                        continue;
                    }
                    format!(
                        "{instance}.resources.requireAudioClip({})",
                        serde_json::to_string(&key)?
                    )
                }
            }
            PropertyValue::SpriteFrame(value) => {
                let absolute = directory.join(value);
                if component.export.is_none() {
                    serde_json::to_string(&absolute)?
                } else {
                    let key = resource_key(project, &absolute)?;
                    if !frame_registered(project, &key) {
                        continue;
                    }
                    format!(
                        "{instance}.resources.requireSpriteFrame({})",
                        serde_json::to_string(&key)?
                    )
                }
            }
            PropertyValue::Component { node, component } => {
                let Some(reference) = component_refs.get(&(*node, component.clone())) else {
                    continue;
                };
                reference.clone()
            }
            PropertyValue::Numbers(items) => serde_json::to_string(items)?,
            PropertyValue::Booleans(items) => serde_json::to_string(items)?,
            PropertyValue::Strings(items) => serde_json::to_string(items)?,
            PropertyValue::Nodes(items) => {
                if let Some(count) = prefab_nodes {
                    ensure!(
                        items.iter().all(|value| *value < count),
                        "Prefab script node reference is out of bounds"
                    );
                }
                if component.export.is_some() {
                    let references = items
                        .iter()
                        .map(|value| {
                            let reference = if prefab_nodes.is_some() {
                                format!("references[{value}]")
                            } else {
                                value.to_string()
                            };
                            format!("new NodeRef({reference}, {instance}.scene)")
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("[{references}]")
                } else {
                    serde_json::to_string(items)?
                }
            }
        };
        if identifier(key) {
            writeln!(entry, "{instance}.{key} = {value};")?;
        } else {
            writeln!(
                entry,
                "{instance}[{}] = {value};",
                serde_json::to_string(key)?
            )?;
        }
    }
    Ok(())
}

fn node_matches_filter(filter: codegen::NodeFilter, node: &NodeFile) -> bool {
    match filter {
        codegen::NodeFilter::Text => matches!(
            &node.visual,
            Some(engine_core::save::visual::Visual::Text { .. })
        ),
        codegen::NodeFilter::Image => {
            node.sprite.is_some()
                || matches!(
                    &node.visual,
                    Some(
                        engine_core::save::visual::Visual::Atlas { .. }
                            | engine_core::save::visual::Visual::TileMap { .. }
                            | engine_core::save::visual::Visual::NineSlice { .. }
                    )
                )
        }
        codegen::NodeFilter::Design => matches!(
            &node.visual,
            Some(engine_core::save::visual::Visual::Design(_))
        ),
    }
}

pub(super) fn identifier(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_keys_are_stable_and_path_specific() {
        assert_eq!(
            project_key(Path::new("/tmp/aurora-game")),
            project_key(Path::new("/tmp/aurora-game"))
        );
        assert_ne!(
            project_key(Path::new("/tmp/aurora-game")),
            project_key(Path::new("/tmp/other-game"))
        );
    }

    #[test]
    fn game_scope_starts_from_the_configured_default_scene() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir(root.join("levels"))?;
        let main = root.join("main.scene");
        let intro = root.join("levels/Intro.scene");
        fs::write(&main, r#"{"version":1,"nodes":[]}"#)?;
        fs::write(&intro, r#"{"version":1,"nodes":[]}"#)?;
        assert_eq!(
            opening_scene(root, PlayScope::Game, "levels/Intro.scene", &main),
            Some(intro.clone())
        );
        assert_eq!(
            opening_scene(root, PlayScope::Game, "", &main),
            Some(main.clone())
        );
        assert_eq!(
            opening_scene(root, PlayScope::Scene, "levels/Intro.scene", &main),
            Some(main)
        );
        Ok(())
    }

    #[test]
    fn script_entry_registers_every_scene_name_in_play_order() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let project = Project::open(temp.path())?;
        let (entry, _) = script_entry_for(
            &project,
            &[],
            Path::new("dev.ts"),
            &BTreeMap::new(),
            &["main".to_owned(), "level2".to_owned()],
            |path| Ok(path.to_path_buf()),
        )?;
        assert!(entry.contains("registerScriptScenes([\"main\",\"level2\"]);"));
        Ok(())
    }

    #[test]
    fn decorator_entry_resolves_default_exports_and_typed_inspector_references() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let mut project = Project::open(temp.path())?;
        fs::write(
            temp.path().join("Driver.ts"),
            r#"
import { ccclass, property, Component, NodeRef, TextRef, ImageRef, Ref, Prefab } from "aurora";
@ccclass("Camera Driver")
export default class Driver extends Component {
  @property(NodeRef) target: NodeRef | null = null;
  @property({ type: Ref.Text }) label: TextRef | null = null;
  @property({ type: Ref.Image }) imageNode: ImageRef | null = null;
  @property(Prefab) prefab: Prefab | null = null;
  @property(AudioClip) clip: AudioClip | null = null;
  @property(SpriteFrame) frame: SpriteFrame | null = null;
  @property(AudioClip) missing: AudioClip | null = null;
  @property([Number]) speeds: number[] = [];
  @property([NodeRef]) targets: NodeRef[] = [];
  @property([Prefab]) prefabs: Prefab[] = [];
  @property([AudioClip]) clips: AudioClip[] = [];
  @property([SpriteFrame]) frames: SpriteFrame[] = [];
}
"#,
        )?;
        project.document.nodes[0]
            .scripts
            .push(engine_core::save::ScriptFile {
                path: "Driver.ts".into(),
                component: "Camera Driver".into(),
                values: [
                    ("target".into(), PropertyValue::Node(0)),
                    ("label".into(), PropertyValue::Node(0)),
                    ("imageNode".into(), PropertyValue::Node(1)),
                    (
                        "prefab".into(),
                        PropertyValue::Prefab("Marker.prefab".into()),
                    ),
                    (
                        "clip".into(),
                        PropertyValue::AudioClip("audio/confirm.wav".into()),
                    ),
                    (
                        "frame".into(),
                        PropertyValue::SpriteFrame("art/hero.png".into()),
                    ),
                    (
                        "missing".into(),
                        PropertyValue::AudioClip("audio/absent.wav".into()),
                    ),
                    ("speeds".into(), PropertyValue::Numbers(vec![1.5, 2.0])),
                    ("targets".into(), PropertyValue::Nodes(vec![0, 0])),
                    (
                        "prefabs".into(),
                        PropertyValue::Prefabs(vec!["Marker.prefab".into()]),
                    ),
                    (
                        "clips".into(),
                        PropertyValue::AudioClips(vec![
                            "audio/confirm.wav".into(),
                            "audio/absent.wav".into(),
                        ]),
                    ),
                    (
                        "frames".into(),
                        PropertyValue::SpriteFrames(vec![
                            "art/hero.png".into(),
                            "art/missing.png".into(),
                        ]),
                    ),
                ]
                .into(),
            });
        project.document.nodes[0].visual = Some(engine_core::save::visual::Visual::Text {
            font: None,
            spans: vec![engine_core::save::visual::Span {
                text: "Ready".into(),
                size: 24,
                color: [255; 4],
            }],
        });
        let mut image_node = project.document.nodes[0].clone();
        image_node.name = "Image".into();
        image_node.parent = Some(0);
        image_node.scripts.clear();
        image_node.visual = Some(engine_core::save::visual::Visual::Atlas {
            png: None,
            cell: [32, 32],
            index: 0,
        });
        project.document.nodes.push(image_node);
        project.document.gameplay = Some(engine_core::save::GameplayFile {
            sensors: Vec::new(),
            frames: vec![engine_core::save::FrameFile {
                node: 0,
                png: vec!["art/hero.png".into()],
            }],
            sounds: vec!["audio/confirm.wav".into()],
        });
        let (entry, bindings) = script_entry(&project, temp.path())?;
        assert_eq!(bindings, [0]);
        assert!(entry.contains("import { default as Component0 }"));
        assert!(entry.contains("new NodeRef(0, instance0.scene)"));
        assert!(entry.contains("instance0.label = new TextRef(0, instance0.scene);"));
        assert!(entry.contains("instance0.imageNode = new ImageRef(1, instance0.scene);"));
        assert!(entry.contains(r#"["Ready",null]"#));
        assert!(entry.contains("new Prefab("));
        assert!(entry.contains(
            "instance0.clip = instance0.resources.requireAudioClip(\"audio/confirm.wav\");"
        ));
        assert!(entry.contains(
            "instance0.frame = instance0.resources.requireSpriteFrame(\"art/hero.png\");"
        ));
        assert!(!entry.contains("absent.wav"));
        assert!(entry.contains("instance0.speeds = [1.5,2.0];"));
        assert!(
            entry.contains("[new NodeRef(0, instance0.scene), new NodeRef(0, instance0.scene)]")
        );
        assert!(entry.contains("new Prefab(\"Marker.prefab\", -1)"));
        assert!(entry.contains(
            "instance0.clips = [instance0.resources.requireAudioClip(\"audio/confirm.wav\")];"
        ));
        assert!(entry.contains(
            "instance0.frames = [instance0.resources.requireSpriteFrame(\"art/hero.png\")];"
        ));
        assert!(!entry.contains("absent.wav"));
        assert!(!entry.contains("missing.png"));
        Ok(())
    }

    #[test]
    fn component_references_resolve_to_sibling_instances() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let mut project = Project::open(temp.path())?;
        fs::write(
            temp.path().join("Health.ts"),
            r#"
import { ccclass, Component } from "aurora";
@ccclass("Health")
export class Health extends Component {}
"#,
        )?;
        fs::write(
            temp.path().join("Driver.ts"),
            r#"
import { ccclass, property, Component } from "aurora";
import { Health } from "./Health.ts";
@ccclass("Driver")
export class Driver extends Component {
  @property(Health) health: Health | null = null;
}
"#,
        )?;
        let node = &mut project.document.nodes[0];
        node.scripts.push(engine_core::save::ScriptFile {
            path: "Health.ts".into(),
            component: "Health".into(),
            values: BTreeMap::new(),
        });
        node.scripts.push(engine_core::save::ScriptFile {
            path: "Driver.ts".into(),
            component: "Driver".into(),
            values: [(
                "health".into(),
                PropertyValue::Component {
                    node: 0,
                    component: "Health".into(),
                },
            )]
            .into(),
        });
        let (entry, _) = script_entry(&project, temp.path())?;
        assert!(entry.contains("instance1.health = instance0;"), "{entry}");
        Ok(())
    }

    #[test]
    fn prefab_builders_create_all_components_before_assigning_references() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let mut project = Project::open(temp.path())?;
        for (file, source) in [
            (
                "Health.ts",
                r#"
import { ccclass, Component } from "aurora";
@ccclass("Health")
export class Health extends Component {}
"#,
            ),
            (
                "Driver.ts",
                r#"
import { ccclass, property, Component } from "aurora";
import { Health } from "./Health.ts";
@ccclass("Driver")
export class Driver extends Component {
  @property(Health) health: Health | null = null;
}
"#,
            ),
        ] {
            fs::write(temp.path().join(file), source)?;
        }
        let prefab = engine_core::save::PrefabFile {
            version: engine_core::save::VERSION,
            joints: Vec::new(),
            nodes: vec![NodeFile {
                name: "Root".into(),
                parent: None,
                transform: Default::default(),
                spatial: None,
                visual: None,
                sprite: None,
                appearance: None,
                physics2d: None,
                scripts: vec![
                    engine_core::save::ScriptFile {
                        path: "Health.ts".into(),
                        component: "Health".into(),
                        values: BTreeMap::new(),
                    },
                    engine_core::save::ScriptFile {
                        path: "Driver.ts".into(),
                        component: "Driver".into(),
                        values: [(
                            "health".into(),
                            PropertyValue::Component {
                                node: 0,
                                component: "Health".into(),
                            },
                        )]
                        .into(),
                    },
                ],
            }],
            animations: Vec::new(),
        };
        prefab.save(&temp.path().join("Marker.prefab"))?;
        project
            .document
            .instances
            .push(engine_core::save::InstanceFile {
                prefab: "Marker.prefab".into(),
                parent: None,
            });
        let (entry, _) = script_entry(&project, temp.path())?;
        let builder = entry
            .split("function prefabBuild0")
            .nth(1)
            .context("Missing prefab builder")?;
        assert!(
            builder.contains("const instance0 = new PrefabComponent0_0();"),
            "{entry}"
        );
        assert!(
            builder.contains("const instance1 = new PrefabComponent0_1();"),
            "{entry}"
        );
        assert!(builder.contains("instance1.health = instance0;"), "{entry}");
        let assignment = builder
            .find("instance1.health")
            .context("Missing component assignment")?;
        let end = builder
            .find("return instances;")
            .context("Missing builder return")?;
        assert!(assignment < end);
        Ok(())
    }

    #[test]
    fn entry_keeps_distinct_attachment_instances_in_node_order() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let mut project = Project::open(temp.path())?;
        let metadata = r#"
registerComponent(["Mover", {"props":{"speed":{"type":"number"}}}]);
"#;
        fs::write(temp.path().join("A.ts"), metadata)?;
        let first = &mut project.document.nodes[0];
        for speed in [1.0, 2.0] {
            first.scripts.push(engine_core::save::ScriptFile {
                path: "A.ts".into(),
                component: "Mover".into(),
                values: [("speed".into(), PropertyValue::Number(speed))].into(),
            });
        }
        let mut second = first.clone();
        second.scripts.truncate(1);
        second.scripts[0]
            .values
            .insert("speed".into(), PropertyValue::Number(3.0));
        project.document.nodes.push(second);

        let (entry, bindings) = script_entry(&project, temp.path())?;
        assert_eq!(bindings, [0, 0, 1]);
        for (index, speed) in [1.0, 2.0, 3.0].into_iter().enumerate() {
            assert!(entry.contains(&format!("Mover as Component{index}")));
            assert!(entry.contains(&format!("instance{index}.speed = {speed:.1};")));
        }
        assert!(entry.ends_with("launchMany([instance0, instance1, instance2]);\n"));
        Ok(())
    }
}
