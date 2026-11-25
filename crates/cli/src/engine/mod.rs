//! Detects and invokes a Docker- or Podman-compatible container engine.

use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
};

use anyhow::{Context, Result, bail, ensure};
use clap::ValueEnum;

/// Container engine selection from the command line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum EngineChoice {
    /// Prefer Docker, then fall back to Podman.
    #[default]
    Auto,
    /// Require Docker.
    Docker,
    /// Require Podman.
    Podman,
}

/// A container engine implementation that shares the OCI command surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineKind {
    /// Docker Engine or a Docker-compatible CLI.
    Docker,
    /// Podman.
    Podman,
}

impl EngineKind {
    /// The executable name looked up on `PATH`.
    pub fn program(self) -> &'static str {
        match self {
            Self::Docker => "docker",
            Self::Podman => "podman",
        }
    }

    /// The human-readable product name used in diagnostics.
    pub fn name(self) -> &'static str {
        match self {
            Self::Docker => "Docker",
            Self::Podman => "Podman",
        }
    }
}

/// A resolved container engine executable.
#[derive(Clone, Debug)]
pub struct ContainerEngine {
    kind: EngineKind,
    program: PathBuf,
}

impl ContainerEngine {
    /// Resolves the selected engine, searching `search_path` or the process `PATH`.
    ///
    /// Both engines share the OCI CLI surface, so one implementation serves either.
    pub fn detect(choice: EngineChoice, search_path: Option<&OsStr>) -> Result<Self> {
        let path = search_path
            .map(OsStr::to_os_string)
            .or_else(|| env::var_os("PATH"))
            .unwrap_or_default();
        let candidates = match choice {
            EngineChoice::Auto => vec![EngineKind::Docker, EngineKind::Podman],
            EngineChoice::Docker => vec![EngineKind::Docker],
            EngineChoice::Podman => vec![EngineKind::Podman],
        };
        for kind in candidates {
            if let Some(program) = find_program(&path, kind.program()) {
                return Ok(Self { kind, program });
            }
        }
        let wanted = match choice {
            EngineChoice::Auto => "Docker or Podman".to_owned(),
            EngineChoice::Docker => EngineKind::Docker.name().to_owned(),
            EngineChoice::Podman => EngineKind::Podman.name().to_owned(),
        };
        bail!(
            "{wanted} was not found on PATH. Aurora game builds run inside a local OCI builder \
             image. Install Docker Engine or Podman (Docker Desktop is not required), or set \
             AURORA_CONTAINER_ENGINE=docker|podman. See docs/BUILD_PACKAGES.md."
        );
    }

    /// Returns the engine product.
    pub fn kind(&self) -> EngineKind {
        self.kind
    }

    /// Returns the resolved executable path.
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// Returns the client version string reported by the engine.
    pub fn version(&self) -> Result<String> {
        let output = Command::new(&self.program)
            .arg("--version")
            .output()
            .with_context(|| format!("Running {} --version", self.program.display()))?;
        ensure!(
            output.status.success(),
            "{} --version failed: {}",
            self.program.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    /// Pulls the exact builder image.
    pub fn pull(&self, image: &str) -> Result<()> {
        self.status(&["pull", image], "pulling the builder image")
    }

    /// Reads the image identifier and labels with one inspect call.
    ///
    /// Returns `(image id, labels)` parsed from `{{.Id}}|{{json .Config.Labels}}`.
    pub fn inspect_image(&self, image: &str) -> Result<(String, serde_json::Value)> {
        let output = Command::new(&self.program)
            .args([
                "image",
                "inspect",
                "--format",
                "{{.Id}}|{{json .Config.Labels}}",
                image,
            ])
            .output()
            .with_context(|| format!("Inspecting builder image {image}"))?;
        ensure!(
            output.status.success(),
            "Could not inspect builder image {image}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        let text = String::from_utf8(output.stdout).context("Image inspect output is not UTF-8")?;
        let (id, labels) = text
            .trim()
            .split_once('|')
            .context("Unexpected image inspect output")?;
        ensure!(!id.trim().is_empty(), "Builder image has no identifier");
        let labels = if labels.trim().is_empty() || labels.trim() == "null" {
            serde_json::Value::Null
        } else {
            serde_json::from_str(labels).context("Builder image labels are not valid JSON")?
        };
        Ok((id.trim().to_owned(), labels))
    }

    /// Runs the builder container and waits for it to finish.
    pub fn run(&self, spec: &RunSpec) -> Result<ExitStatus> {
        let mut command = Command::new(&self.program);
        command.args(spec.args(self.kind));
        command
            .status()
            .with_context(|| format!("Running the builder container {}", spec.name))
    }

    /// Removes a builder image. A missing image is not an error.
    pub fn remove_image(&self, image: &str) -> Result<()> {
        let output = Command::new(&self.program)
            .args(["rmi", "-f", image])
            .output()
            .with_context(|| format!("Removing builder image {image}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let missing = stderr.to_ascii_lowercase();
            if !(missing.contains("no such image")
                || missing.contains("not found")
                || missing.contains("unable to find image"))
            {
                bail!("Could not remove builder image {image}: {}", stderr.trim());
            }
        }
        Ok(())
    }

    fn status(&self, args: &[&str], action: &str) -> Result<()> {
        let mut command = Command::new(&self.program);
        command.args(args);
        let status = command
            .status()
            .with_context(|| format!("{} while {action}", self.program.display()))?;
        if !status.success() {
            let hint = if self.kind == EngineKind::Docker {
                " Docker is installed but its daemon may be unreachable; start it or add your \
                 user to the docker group, or use rootless Podman."
            } else {
                ""
            };
            bail!("{} failed {action}.{hint}", self.kind.name());
        }
        Ok(())
    }
}

/// One builder container invocation.
#[derive(Debug)]
pub struct RunSpec {
    /// Unique, label-scoped container name for cleanup diagnostics.
    pub name: String,
    /// Exact builder image reference.
    pub image: String,
    /// Host project root, mounted read-only at `/project`.
    pub project: PathBuf,
    /// Container path passed to the editor: `/project` or `/project/scene.scene`.
    pub project_arg: String,
    /// Host staging directory that receives the package, mounted read-write.
    pub staging: PathBuf,
    /// Target identifier exposed to the builder.
    pub target: &'static str,
    /// Included scenes as container paths below `/project`.
    pub includes: Vec<String>,
}

impl RunSpec {
    /// Builds the argument vector for this run.
    ///
    /// Volumes are passed as separate arguments, so paths with spaces need no quoting.
    pub fn args(&self, kind: EngineKind) -> Vec<OsString> {
        let mut args: Vec<OsString> = vec![
            "run".into(),
            "--rm".into(),
            "--name".into(),
            self.name.as_str().into(),
            "--network=none".into(),
            "--cap-drop=ALL".into(),
            "--security-opt=no-new-privileges".into(),
            "--env".into(),
            format!("AURORA_TARGET={}", self.target).into(),
            "--env".into(),
            format!("AURORA_PROJECT={}", self.project_arg).into(),
            "--env".into(),
            "AURORA_OUTPUT=/out/app".into(),
        ];
        if !self.includes.is_empty() {
            args.push(OsString::from("--env"));
            args.push(OsString::from(format!(
                "AURORA_INCLUDE_SCENES={}",
                self.includes.join("\n")
            )));
        }
        // Bind mounts are not relabeled: relabeling would mutate the user's
        // project. Podman instead disables SELinux confinement for this
        // unprivileged, network-less build container.
        let mut project = self.project.clone().into_os_string();
        project.push(":/project:ro");
        let mut staging = self.staging.clone().into_os_string();
        staging.push(":/out:rw");
        if kind == EngineKind::Podman {
            args.push(OsString::from("--security-opt=label=disable"));
        }
        args.push(OsString::from("--volume"));
        args.push(project);
        args.push(OsString::from("--volume"));
        args.push(staging);
        args.push(OsString::from("--label"));
        args.push(OsString::from("org.aurora.builder=1"));
        args.push(OsString::from(&self.image));
        args
    }
}

fn find_program(search_path: &OsStr, name: &str) -> Option<PathBuf> {
    let suffix = env::consts::EXE_SUFFIX;
    for directory in env::split_paths(search_path) {
        let candidate = directory.join(format!("{name}{suffix}"));
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_args_mount_only_project_and_staging() {
        let spec = RunSpec {
            name: "aurora-build-test".into(),
            image: "example/builder:1".into(),
            project: PathBuf::from("/work/my project"),
            project_arg: "/project".into(),
            staging: PathBuf::from("/tmp/stage dir"),
            target: "linux",
            includes: vec!["/project/scenes/second.scene".into()],
        };
        let args = spec.args(EngineKind::Docker);
        let args: Vec<String> = args
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args[0], "run");
        assert!(args.contains(&"--network=none".to_owned()));
        assert!(args.contains(&"--cap-drop=ALL".to_owned()));
        assert!(args.contains(&"AURORA_TARGET=linux".to_owned()));
        assert!(args.contains(&"AURORA_PROJECT=/project".to_owned()));
        assert!(args.contains(&"AURORA_OUTPUT=/out/app".to_owned()));
        assert!(args.contains(&"AURORA_INCLUDE_SCENES=/project/scenes/second.scene".to_owned()));
        assert!(args.contains(&"/work/my project:/project:ro".to_owned()));
        assert!(args.contains(&"/tmp/stage dir:/out:rw".to_owned()));
        assert_eq!(args.last().map(String::as_str), Some("example/builder:1"));
        let volumes = args
            .iter()
            .filter(|arg| arg.contains(":/project") || arg.contains(":/out"))
            .count();
        assert_eq!(volumes, 2);
    }

    #[test]
    fn podman_disables_labels_without_relabeling_mounts() {
        let spec = RunSpec {
            name: "n".into(),
            image: "i".into(),
            project: PathBuf::from("/p"),
            project_arg: "/project".into(),
            staging: PathBuf::from("/s"),
            target: "linux",
            includes: Vec::new(),
        };
        let args: Vec<String> = spec
            .args(EngineKind::Podman)
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"--security-opt=label=disable".to_owned()));
        assert!(args.contains(&"/p:/project:ro".to_owned()));
        assert!(args.contains(&"/s:/out:rw".to_owned()));
    }
}
