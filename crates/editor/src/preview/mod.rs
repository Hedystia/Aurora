//! Supervises the offscreen player and refreshes the Game view texture.

use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, SyncSender},
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Context, Result, ensure};
use egui::{ColorImage, Context as EguiContext, TextureHandle, TextureOptions};
use engine_core::app::preview::SIZE;
use engine_core::save::PropertyValue;

use crate::{project::Project, strings};

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

    pub fn start(&mut self, project: &Project) {
        let (send, receive) = mpsc::sync_channel(1);
        let project = project.clone();
        let logs = self.sender.clone();
        self.pending = Some(receive);
        self.game_view = true;
        std::thread::spawn(move || {
            let _ = send.send(Self::launch(&project, logs));
        });
    }

    fn launch(project: &Project, logs: SyncSender<String>) -> Result<Run> {
        let directory = tempfile::tempdir()?;
        let location = directory.path();
        let mut document = project.document.clone();
        for node in &mut document.nodes {
            if let Some(sprite) = &mut node.sprite {
                sprite.png = project.root.join(&sprite.png);
            }
        }
        for instance in &mut document.instances {
            instance.prefab = project.root.join(&instance.prefab);
        }
        let scene = location.join("preview.aurora-scene");
        document.save(&scene)?;
        let engine_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()?;
        let player = std::env::current_exe()?.with_file_name("aurora-player");
        let mut command = Command::new(player);
        command
            .arg("--preview-dir")
            .arg(location)
            .arg("--scene")
            .arg(scene)
            .arg("--asset-cache")
            .arg(project.root.join(".aurora/import"));
        if let Some((node, attachment)) =
            project
                .document
                .nodes
                .iter()
                .enumerate()
                .find_map(|(node, content)| {
                    content.scripts.first().map(|attachment| (node, attachment))
                })
        {
            let path = project.root.join(&attachment.path);
            let content = fs::read_to_string(&path)?;
            let metadata = codegen::extract(&content)?;
            let component = metadata
                .get(&attachment.component)
                .context("Component is missing from script metadata")?;
            let script = serde_json::to_string(&path)?;
            let launch = serde_json::to_string(&engine_root.join("scripts/build/dev.ts"))?;
            let exported = &attachment.component;
            ensure!(
                identifier(exported),
                "Invalid exported class name {exported}"
            );
            let mut entry = format!(
                "import {{ {exported} }} from {script};\nimport {{ launch }} from {launch};\nimport {{ registerComponent }} from \"aurora-cc\";\nconst instance = new {exported}();\n"
            );
            for (key, value) in &attachment.values {
                if !component
                    .props
                    .get(key)
                    .is_some_and(|spec| crate::inspector::value_matches(spec.kind, value))
                {
                    continue;
                }
                let value = match value {
                    PropertyValue::Number(v) => serde_json::to_value(v)?,
                    PropertyValue::Boolean(v) => serde_json::to_value(v)?,
                    PropertyValue::String(v) => serde_json::to_value(v)?,
                    PropertyValue::Node(v) => serde_json::to_value(v)?,
                    PropertyValue::Prefab(v) => serde_json::to_value(project.root.join(v))?,
                };
                entry.push_str(&format!(
                    "instance[{}] = {};\n",
                    serde_json::to_string(key)?,
                    value
                ));
            }
            entry.push_str(&format!(
                "launch(instance);\nregisterComponent({});\n",
                serde_json::to_string(&(exported, component))?
            ));
            fs::write(location.join("entry.ts"), entry)?;
            let sdk = serde_json::to_string(&engine_root.join("scripts/aurora-cc/index.ts"))?;
            fs::write(
                location.join("tsconfig.json"),
                format!(
                    r#"{{"compilerOptions":{{"baseUrl":".","paths":{{"aurora-cc":[{sdk}]}}}}}}"#
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
                .arg(location.join("game-script"))
                .arg("--script-node")
                .arg(node.to_string());
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
        })
    }

    pub fn input(&mut self, byte: u8) {
        if let Some(input) = self.run.as_mut().and_then(|run| run.child.stdin.as_mut()) {
            let _ = input.write_all(&[byte]);
        }
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
                && bytes.len() == SIZE[0] as usize * SIZE[1] as usize * 4
            {
                run.last_update = modified;
                let image = ColorImage::from_rgba_unmultiplied(SIZE.map(|n| n as usize), &bytes);
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

pub(super) fn identifier(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
