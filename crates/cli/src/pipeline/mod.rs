//! Runs a game build inside a versioned OCI builder image or, for macOS, on
//! an Apple-branded host.
//!
//! Container builds orchestrate only image pull, container execution, output
//! verification, and cleanup. All compilation happens inside the image.

mod macos;

pub use macos::build as build_macos;

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::ExitStatus,
};

use anyhow::{Context, Result, bail, ensure};
use engine_core::app::bundle::{self, ExportManifest};
use serde::Serialize;

use crate::{
    engine::{ContainerEngine, RunSpec},
    target::Target,
    template,
};

/// Build metadata format written beside the package contents.
pub const METADATA_VERSION: u32 = 1;

/// Filename of the self-describing build record inside the package.
pub const BUILD_METADATA: &str = "build.json";

/// A requested local container build.
#[derive(Debug)]
pub struct BuildRequest {
    /// Project directory or authored scene to build.
    pub project: PathBuf,
    /// Selected target.
    pub target: Target,
    /// Absent host directory that receives the finished package.
    pub output: PathBuf,
    /// Additional saved scenes inside the project, in switch order.
    pub includes: Vec<PathBuf>,
    /// Template archive path or URL whose player and launcher are exported.
    pub template: Option<String>,
    /// Exact builder image reference, or the target's default.
    pub image: Option<String>,
    /// Keep the pulled builder image after the build.
    pub keep_image: bool,
    /// Print the resolved plan without pulling or running anything.
    pub dry_run: bool,
    /// macOS only: codesign identity, or `-` for an ad-hoc signature.
    pub sign: Option<String>,
    /// macOS only: `notarytool` keychain profile used for notarization.
    pub notarize: Option<String>,
}

/// Summary of a completed or planned build.
#[derive(Debug)]
pub struct BuildOutcome {
    /// Builder image that was used or would be used.
    pub image: String,
    /// Resolved image identifier, when the image was pulled.
    pub image_id: Option<String>,
    /// Final host output directory.
    pub output: PathBuf,
    /// Content digest of the verified package.
    pub source_bundle_sha256: Option<String>,
    /// Whether the request was a dry run.
    pub dry_run: bool,
}

/// Self-describing record included in every built package.
#[derive(Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BuildMetadata {
    version: u32,
    engine_version: String,
    target: String,
    target_triple: String,
    features: Vec<String>,
    builder_kind: String,
    builder_image: Option<String>,
    builder_image_id: Option<String>,
    source_bundle_sha256: String,
    toolchain: BTreeMap<String, String>,
}

struct ContainerMetadata<'a> {
    version: &'a str,
    target: &'a str,
    triple: &'a str,
    features: Vec<String>,
    image: &'a str,
    image_id: &'a str,
    source_bundle_sha256: &'a str,
    toolchain: BTreeMap<String, String>,
}

impl BuildMetadata {
    fn container(container: ContainerMetadata<'_>) -> Self {
        Self {
            version: METADATA_VERSION,
            engine_version: container.version.to_owned(),
            target: container.target.to_owned(),
            target_triple: container.triple.to_owned(),
            features: container.features,
            builder_kind: "container".into(),
            builder_image: Some(container.image.to_owned()),
            builder_image_id: Some(container.image_id.to_owned()),
            source_bundle_sha256: container.source_bundle_sha256.to_owned(),
            toolchain: container.toolchain,
        }
    }

    pub(super) fn host(
        version: &str,
        target: &str,
        triple: &str,
        source_bundle_sha256: &str,
    ) -> Self {
        Self {
            version: METADATA_VERSION,
            engine_version: version.to_owned(),
            target: target.to_owned(),
            target_triple: triple.to_owned(),
            features: Vec::new(),
            builder_kind: "host".into(),
            builder_image: None,
            builder_image_id: None,
            source_bundle_sha256: source_bundle_sha256.to_owned(),
            toolchain: BTreeMap::new(),
        }
    }
}

/// Writes the build record, then regenerates and verifies the integrity
/// manifest so the record is covered by the player's file-set check.
pub(super) fn write_build_metadata(
    source: &Path,
    metadata: &BuildMetadata,
    engine_version: &str,
    manifest_target: &str,
) -> Result<ExportManifest> {
    fs::write(
        source.join(BUILD_METADATA),
        serde_json::to_vec_pretty(metadata)?,
    )?;
    let manifest = ExportManifest::create(source, engine_version, manifest_target)?;
    manifest.verify(source)?;
    fs::write(
        source.join(bundle::EXPORT_MANIFEST),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(manifest)
}

/// Builds `request` using the already resolved container engine.
pub fn build(engine: &ContainerEngine, request: &BuildRequest) -> Result<BuildOutcome> {
    let version = env!("CARGO_PKG_VERSION");
    request.target.ensure_supported()?;
    ensure!(
        request.sign.is_none() && request.notarize.is_none(),
        "--sign and --notarize apply only to --target macos"
    );
    let (project, selected) = resolve_project(&request.project)?;
    let template = template::resolve(
        request.template.as_deref(),
        &project,
        &request.target,
        request.dry_run,
    )?;
    if let Some(template) = &template {
        eprintln!(
            "Using target template {} (player {}, launcher {})",
            template.directory.display(),
            template.player.display(),
            template.launcher.display()
        );
    }
    let (template_dir, template_player) = match &template {
        Some(resolved) => (
            Some(resolved.directory.clone()),
            Some(
                resolved
                    .player
                    .file_name()
                    .context("Template player has no filename")?
                    .to_string_lossy()
                    .into_owned(),
            ),
        ),
        None => (None, None),
    };
    let project_arg = if selected == project {
        "/project".to_owned()
    } else {
        let name = selected
            .file_name()
            .and_then(|name| name.to_str())
            .context("Scene filenames must be UTF-8")?;
        format!("/project/{name}")
    };
    let output = absolute_output(&request.output)?;
    ensure!(
        !output.try_exists()?,
        "Build output already exists: {}",
        output.display()
    );
    let includes = resolve_includes(&project, &request.includes)?
        .into_iter()
        .map(|include| {
            let relative = include
                .strip_prefix(&project)?
                .to_str()
                .context("Included scene paths must be UTF-8")?
                .replace(std::path::MAIN_SEPARATOR, "/");
            Ok(format!("/project/{relative}"))
        })
        .collect::<Result<Vec<_>>>()?;
    let image = request
        .image
        .clone()
        .unwrap_or_else(|| request.target.image(version));
    let name = container_name();

    if request.dry_run {
        let staging = std::env::temp_dir().join("aurora-build-staging");
        let spec = RunSpec {
            name,
            image: image.clone(),
            project,
            project_arg,
            staging,
            target: request.target.id,
            export_target: request.target.manifest_target,
            includes,
            template: template_dir.clone(),
            template_player: template_player.clone(),
        };
        let args = spec
            .args(engine.kind())
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        eprintln!(
            "Dry run: {} would build target {} with {}",
            engine.program().display(),
            request.target.id,
            image
        );
        eprintln!("  {}", args.join(" "));
        if let Some(source) = &request.template {
            eprintln!("  Template: {source} (resolved only when not a dry run)");
        }
        return Ok(BuildOutcome {
            image,
            image_id: None,
            output,
            source_bundle_sha256: None,
            dry_run: true,
        });
    }

    ensure!(
        output.parent().is_some_and(Path::is_dir),
        "The output parent directory must exist: {}",
        output.display()
    );
    eprintln!(
        "Pulling builder image {image} (engine version {version}, target {})",
        request.target.id
    );
    if let Err(error) = engine.pull(&image) {
        // Offline builds and locally built images use the exact same versioned
        // reference. Only fall back when that reference already exists.
        if engine.inspect_image(&image).is_err() {
            return Err(error);
        }
        eprintln!(
            "Warning: could not pull {image}; using the local image with this exact reference."
        );
    }
    let cleanup = ImageCleanup {
        engine,
        image: image.clone(),
        keep: request.keep_image,
    };
    let (image_id, labels) = engine.inspect_image(&image)?;
    if let Some(builder_version) = labels
        .get("org.aurora.engine_version")
        .and_then(serde_json::Value::as_str)
    {
        ensure!(
            builder_version == version,
            "Builder image targets engine version {builder_version} but this CLI is {version}"
        );
    }

    let staging = tempfile::Builder::new()
        .prefix("aurora-build-")
        .tempdir()
        .context("Creating the build staging directory")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        // The builder runs as a non-root image user that must write the package.
        fs::set_permissions(staging.path(), fs::Permissions::from_mode(0o777))?;
    }
    let spec = RunSpec {
        name,
        image: image.clone(),
        project,
        project_arg,
        staging: staging.path().to_path_buf(),
        target: request.target.id,
        export_target: request.target.manifest_target,
        includes,
        template: template_dir,
        template_player,
    };
    eprintln!("Building inside {}", spec.name);
    let status = engine.run(&spec)?;
    ensure_success(status)?;

    let app = staging.path().join("app");
    ensure!(
        app.is_dir(),
        "The builder did not write a package to the staging directory"
    );
    let manifest_path = app.join(bundle::EXPORT_MANIFEST);
    let container_manifest: ExportManifest = serde_json::from_slice(
        &fs::read(&manifest_path)
            .with_context(|| format!("Reading {}", manifest_path.display()))?,
    )
    .context("The builder produced an invalid export manifest")?;
    ensure!(
        container_manifest.engine_version == version,
        "Builder image produced engine version {} but this CLI is {version}",
        container_manifest.engine_version
    );
    ensure!(
        container_manifest.target == request.target.manifest_target,
        "Builder image produced target {} but {} was requested",
        container_manifest.target,
        request.target.manifest_target
    );
    container_manifest
        .verify(&app)
        .context("The builder output failed its integrity check")?;

    let metadata = BuildMetadata::container(ContainerMetadata {
        version,
        target: request.target.manifest_target,
        triple: request.target.triple,
        features: request
            .target
            .features
            .iter()
            .map(|feature| (*feature).to_owned())
            .collect(),
        image: &image,
        image_id: &image_id,
        source_bundle_sha256: &container_manifest.source_bundle_sha256,
        toolchain: toolchain_labels(&labels),
    });
    write_build_metadata(&app, &metadata, version, request.target.manifest_target)?;

    fs::create_dir(&output)
        .with_context(|| format!("Creating build output {}", output.display()))?;
    if let Err(error) = copy_tree(&app, &output) {
        let _ = fs::remove_dir_all(&output);
        return Err(error);
    }
    let written: ExportManifest =
        serde_json::from_slice(&fs::read(output.join(bundle::EXPORT_MANIFEST))?)?;
    if let Err(error) = written
        .verify(&output)
        .context("The published build failed its integrity check")
    {
        let _ = fs::remove_dir_all(&output);
        return Err(error);
    }
    drop(cleanup);
    Ok(BuildOutcome {
        image,
        image_id: Some(image_id),
        output,
        source_bundle_sha256: Some(written.source_bundle_sha256),
        dry_run: false,
    })
}

fn ensure_success(status: ExitStatus) -> Result<()> {
    if status.success() {
        return Ok(());
    }
    match status.code() {
        Some(code) => bail!("Builder container failed with exit code {code}"),
        None => bail!("Builder container was terminated by a signal"),
    }
}

pub(super) fn resolve_project(path: &Path) -> Result<(PathBuf, PathBuf)> {
    let selected = path
        .canonicalize()
        .with_context(|| format!("Resolving project {}", path.display()))?;
    if selected.is_dir() {
        return Ok((selected.clone(), selected));
    }
    ensure!(selected.is_file(), "The build project must exist");
    let root = selected
        .parent()
        .context("The selected scene has no containing project")?
        .to_path_buf();
    Ok((root, selected))
}

pub(super) fn resolve_includes(project: &Path, includes: &[PathBuf]) -> Result<Vec<PathBuf>> {
    includes
        .iter()
        .map(|include| {
            let source = include
                .canonicalize()
                .with_context(|| format!("Resolving included scene {}", include.display()))?;
            ensure!(
                source.starts_with(project),
                "Included scenes must live inside the project: {}",
                include.display()
            );
            ensure!(source.is_file(), "Included scene is not a file");
            Ok(source)
        })
        .collect()
}

pub(super) fn absolute_output(output: &Path) -> Result<PathBuf> {
    let name = output
        .file_name()
        .context("Build output needs a directory name")?;
    let parent = match output.parent().filter(|path| !path.as_os_str().is_empty()) {
        Some(parent) => parent
            .canonicalize()
            .with_context(|| format!("Resolving output parent {}", parent.display()))?,
        None => std::env::current_dir()?.canonicalize()?,
    };
    Ok(parent.join(name))
}

fn toolchain_labels(labels: &serde_json::Value) -> BTreeMap<String, String> {
    let mut toolchain = BTreeMap::new();
    let Some(labels) = labels.as_object() else {
        return toolchain;
    };
    for (key, value) in labels {
        let Some(name) = key.strip_prefix("org.aurora.toolchain.") else {
            continue;
        };
        if let Some(value) = value.as_str() {
            toolchain.insert(name.to_owned(), value.to_owned());
        }
    }
    toolchain
}

pub(super) fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    let mut entries = fs::read_dir(source)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let kind = entry.file_type()?;
        ensure!(
            !kind.is_symlink(),
            "Build packages cannot contain symlinks: {}",
            entry.path().display()
        );
        let target = destination.join(entry.file_name());
        if kind.is_dir() {
            fs::create_dir(&target)?;
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &target)?;
            #[cfg(unix)]
            {
                fs::set_permissions(&target, entry.metadata()?.permissions())?;
            }
        } else {
            bail!(
                "Unsupported build package entry: {}",
                entry.path().display()
            );
        }
    }
    Ok(())
}

fn container_name() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!(
        "aurora-build-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

/// Removes the ephemeral builder image when the build finishes, unless asked
/// to keep it. Cleanup runs on failure as well as success.
struct ImageCleanup<'a> {
    engine: &'a ContainerEngine,
    image: String,
    keep: bool,
}

impl Drop for ImageCleanup<'_> {
    fn drop(&mut self) {
        if self.keep {
            return;
        }
        if let Err(error) = self.engine.remove_image(&self.image) {
            eprintln!("Warning: could not remove builder image: {error:#}");
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use sha2::{Digest, Sha256};

    use super::*;
    use crate::engine::{EngineChoice, EngineKind};

    const VERSION: &str = env!("CARGO_PKG_VERSION");

    struct Stub {
        _directory: tempfile::TempDir,
        log: PathBuf,
    }

    impl Stub {
        fn docker(run_status: i32) -> Result<Self> {
            Self::write("docker", run_status, 0)
        }

        fn docker_with_pull_failure(run_status: i32) -> Result<Self> {
            Self::write("docker", run_status, 1)
        }

        fn write(name: &str, run_status: i32, pull_status: i32) -> Result<Self> {
            let directory = tempfile::tempdir()?;
            let bin = directory.path().join("bin");
            fs::create_dir(&bin)?;
            let program = bin.join(name);
            let log = directory.path().join("engine.log");
            let hash = hash(b"scene");
            let bundle = bundle_hash("main.scene", &hash);
            let script = format!(
                r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> '{log}'
case "$1" in
  --version) echo "stub {name} 99.0.0" ;;
  pull) exit {pull_status} ;;
  image) echo 'sha256:stub-image|{{"org.aurora.engine_version":"{version}","org.aurora.toolchain.rust":"1.99.1","org.aurora.toolchain.scriptc":"0.4.16"}}' ;;
  run)
    previous=''
    out=''
    for argument in "$@"; do
      case "$previous" in
        --volume|-v)
          case "$argument" in
            *:/out:rw*) out=${{argument%:/out:rw*}} ;;
          esac
          ;;
      esac
      previous=$argument
    done
    if [ -z "$out" ]; then echo "missing /out mount" >&2; exit 9; fi
    mkdir -p "$out/app"
    printf 'scene' > "$out/app/main.scene"
    printf '%s' '{{"version":1,"engine_version":"{version}","target":"linux-x86_64","source_bundle_sha256":"{bundle}","files":[{{"path":"main.scene","sha256":"{hash}"}}]}}' > "$out/app/export.json"
    exit {run_status}
    ;;
  rmi) ;;
  *) echo "unexpected stub command: $*" >&2; exit 8 ;;
esac
"#,
                log = log.display(),
                name = name,
                version = VERSION,
                hash = hash,
                bundle = bundle,
                run_status = run_status,
                pull_status = pull_status
            );
            fs::write(&program, script)?;
            fs::set_permissions(&program, fs::Permissions::from_mode(0o755))?;
            Ok(Self {
                _directory: directory,
                log,
            })
        }

        fn engine(&self, kind: EngineKind) -> Result<ContainerEngine> {
            let bin = self._directory.path().join("bin");
            ContainerEngine::detect(
                match kind {
                    EngineKind::Docker => EngineChoice::Docker,
                    EngineKind::Podman => EngineChoice::Podman,
                },
                Some(bin.as_os_str()),
            )
        }

        fn log(&self) -> String {
            fs::read_to_string(&self.log).unwrap_or_default()
        }
    }

    fn hash(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    fn bundle_hash(path: &str, hash: &str) -> String {
        let mut digest = Sha256::new();
        digest.update((path.len() as u64).to_le_bytes());
        digest.update(path.as_bytes());
        digest.update(hash.as_bytes());
        format!("{:x}", digest.finalize())
    }

    fn request(project: PathBuf, output: PathBuf) -> Result<BuildRequest> {
        Ok(BuildRequest {
            project,
            target: Target::parse("linux")?,
            output,
            includes: Vec::new(),
            template: None,
            image: None,
            keep_image: false,
            dry_run: false,
            sign: None,
            notarize: None,
        })
    }

    fn project_named(root: &Path, name: &str) -> Result<PathBuf> {
        let project = root.join(name);
        fs::create_dir(&project)?;
        fs::write(project.join("main.scene"), "scene")?;
        Ok(project)
    }

    fn failure(result: Result<BuildOutcome>) -> Result<String> {
        match result {
            Ok(_) => bail!("expected the build to fail"),
            Err(error) => Ok(format!("{error:#}")),
        }
    }

    #[test]
    fn builds_verifies_and_cleans_up() -> Result<()> {
        let stub = Stub::docker(0)?;
        let engine = stub.engine(EngineKind::Docker)?;
        let work = tempfile::tempdir()?;
        let project = project_named(work.path(), "project")?;
        let output = work.path().join("app");

        let outcome = build(&engine, &request(project, output.clone())?)?;
        assert!(!outcome.dry_run);
        assert_eq!(outcome.image, Target::parse("linux")?.image(VERSION));
        assert_eq!(outcome.image_id.as_deref(), Some("sha256:stub-image"));
        assert!(output.join("main.scene").is_file());
        assert!(output.join(bundle::EXPORT_MANIFEST).is_file());
        let metadata: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join(BUILD_METADATA))?)?;
        assert_eq!(metadata["engine_version"], VERSION);
        assert_eq!(metadata["target"], "linux-x86_64");
        assert_eq!(metadata["target_triple"], "x86_64-unknown-linux-gnu");
        assert_eq!(metadata["builder_kind"], "container");
        assert_eq!(metadata["builder_image_id"], "sha256:stub-image");
        assert_eq!(metadata["toolchain"]["rust"], "1.99.1");
        assert_eq!(metadata["toolchain"]["scriptc"], "0.4.16");

        let log = stub.log();
        assert!(log.lines().any(|line| line.starts_with("pull ")), "{log}");
        assert!(log.lines().any(|line| line.starts_with("run ")), "{log}");
        assert!(
            log.lines().any(|line| line.starts_with("rmi -f ")),
            "ephemeral image must be removed: {log}"
        );
        Ok(())
    }

    #[test]
    fn falls_back_to_a_matching_local_image_when_pull_fails() -> Result<()> {
        let stub = Stub::docker_with_pull_failure(0)?;
        let engine = stub.engine(EngineKind::Docker)?;
        let work = tempfile::tempdir()?;
        let project = project_named(work.path(), "project")?;
        build(&engine, &request(project, work.path().join("app"))?)?;
        let log = stub.log();
        assert!(log.lines().any(|line| line.starts_with("pull ")), "{log}");
        assert!(log.lines().any(|line| line.starts_with("run ")), "{log}");
        Ok(())
    }

    #[test]
    fn accepts_a_scene_file_as_the_project() -> Result<()> {
        let stub = Stub::docker(0)?;
        let engine = stub.engine(EngineKind::Docker)?;
        let work = tempfile::tempdir()?;
        let project = project_named(work.path(), "project")?;
        let scene = project.join("main.scene");
        build(&engine, &request(scene, work.path().join("app"))?)?;
        assert!(
            stub.log().contains("AURORA_PROJECT=/project/main.scene"),
            "{}",
            stub.log()
        );
        Ok(())
    }

    #[test]
    fn preserves_image_when_requested() -> Result<()> {
        let stub = Stub::docker(0)?;
        let engine = stub.engine(EngineKind::Docker)?;
        let work = tempfile::tempdir()?;
        let project = project_named(work.path(), "project")?;
        let mut request = request(project, work.path().join("app"))?;
        request.keep_image = true;
        build(&engine, &request)?;
        assert!(
            !stub.log().lines().any(|line| line.starts_with("rmi ")),
            "{}",
            stub.log()
        );
        Ok(())
    }

    #[test]
    fn failing_container_propagates_status_and_still_cleans_up() -> Result<()> {
        let stub = Stub::docker(7)?;
        let engine = stub.engine(EngineKind::Docker)?;
        let work = tempfile::tempdir()?;
        let project = project_named(work.path(), "project")?;
        let error = failure(build(&engine, &request(project, work.path().join("app"))?))?;
        assert!(error.contains("exit code 7"), "{error}");
        let log = stub.log();
        assert!(log.lines().any(|line| line.starts_with("rmi -f ")), "{log}");
        Ok(())
    }

    #[test]
    fn supports_paths_with_spaces() -> Result<()> {
        let stub = Stub::docker(0)?;
        let engine = stub.engine(EngineKind::Docker)?;
        let work = tempfile::tempdir()?;
        let project = project_named(work.path(), "project with space")?;
        let output = work.path().join("output with space");
        let outcome = build(&engine, &request(project, output.clone())?)?;
        assert!(outcome.output.ends_with("output with space"));
        assert!(output.join("build.json").is_file());
        assert!(
            stub.log().contains("project with space:/project:ro"),
            "{}",
            stub.log()
        );
        Ok(())
    }

    #[test]
    fn dry_run_does_not_pull_or_run() -> Result<()> {
        let stub = Stub::docker(0)?;
        let engine = stub.engine(EngineKind::Docker)?;
        let work = tempfile::tempdir()?;
        let project = project_named(work.path(), "project")?;
        let mut request = request(project, work.path().join("app"))?;
        request.dry_run = true;
        let outcome = build(&engine, &request)?;
        assert!(outcome.dry_run);
        let log = stub.log();
        assert!(!log.lines().any(|line| line.starts_with("pull ")), "{log}");
        assert!(!log.lines().any(|line| line.starts_with("run ")), "{log}");
        Ok(())
    }

    #[test]
    fn refuses_blocked_targets_before_running() -> Result<()> {
        let stub = Stub::docker(0)?;
        let engine = stub.engine(EngineKind::Docker)?;
        let work = tempfile::tempdir()?;
        let project = project_named(work.path(), "project")?;
        let mut request = request(project, work.path().join("app"))?;
        request.target = Target::parse("android")?;
        let error = failure(build(&engine, &request))?;
        assert!(error.contains("no Android application runtime"), "{error}");
        assert!(stub.log().is_empty());
        Ok(())
    }

    #[test]
    fn refuses_existing_output_and_outside_scenes() -> Result<()> {
        let stub = Stub::docker(0)?;
        let engine = stub.engine(EngineKind::Docker)?;
        let work = tempfile::tempdir()?;
        let project = project_named(work.path(), "project")?;
        let output = work.path().join("app");
        fs::create_dir(&output)?;
        let error = failure(build(&engine, &request(project.clone(), output)?))?;
        assert!(error.contains("already exists"), "{error}");

        let outside = work.path().join("outside.scene");
        fs::write(&outside, "scene")?;
        let mut request = request(project, work.path().join("app2"))?;
        request.includes.push(outside);
        let error = failure(build(&engine, &request))?;
        assert!(error.contains("inside the project"), "{error}");
        Ok(())
    }

    #[test]
    fn auto_prefers_docker_then_podman() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let bin = directory.path().join("bin");
        fs::create_dir(&bin)?;
        for name in ["docker", "podman"] {
            let path = bin.join(name);
            fs::write(&path, "#!/bin/sh\nexit 0\n")?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
        }
        let engine = ContainerEngine::detect(EngineChoice::Auto, Some(bin.as_os_str()))?;
        assert_eq!(engine.kind(), EngineKind::Docker);
        assert!(engine.program().ends_with("docker"));
        Ok(())
    }

    #[test]
    fn missing_engine_is_actionable() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let error =
            match ContainerEngine::detect(EngineChoice::Auto, Some(directory.path().as_os_str())) {
                Ok(_) => bail!("expected detection to fail"),
                Err(error) => format!("{error:#}"),
            };
        assert!(error.contains("Docker or Podman"), "{error}");
        assert!(error.contains("Docker Desktop is not required"), "{error}");
        Ok(())
    }
}
