//! Verified, cached target templates for cross-target exports.
//!
//! A template is a small archive produced by `tools/package-target-template.sh`.
//! Its manifest records the engine version, target, triple and a SHA-256 for
//! each runtime file. Downloads use `curl` and local hashing, matching the
//! toolchain provisioning in `tools/setup.sh`, so the CLI needs no HTTP crate.

use std::{
    collections::BTreeSet,
    fs,
    io::{BufReader, Read},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::target::Target;

const MANIFEST: &str = "template.json";
const FORMAT_VERSION: u32 = 1;
const MAX_FILES: usize = 8;
const MAX_BYTES: u64 = 512 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

/// Self-describing template manifest shipped inside the archive.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TemplateManifest {
    version: u32,
    engine_version: String,
    target: String,
    triple: String,
    bundle_digest: String,
    files: Vec<TemplateFile>,
}

/// One hashed runtime file in a template.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TemplateFile {
    path: String,
    sha256: String,
}

/// A verified template already staged in the project's cache.
#[derive(Debug)]
pub(super) struct ResolvedTemplate {
    /// Directory holding the manifest and runtime files.
    pub directory: PathBuf,
    /// Player runtime to pass to the editor's `--player` flag.
    pub player: PathBuf,
    /// Launcher included in the template.
    pub launcher: PathBuf,
}

/// Player filename for a manifest target.
pub(super) fn player_name(target: &str) -> &'static str {
    if target.starts_with("windows-") {
        "aurora-player.exe"
    } else {
        "aurora-player"
    }
}

fn launcher_name(target: &str) -> &'static str {
    if target.starts_with("windows-") {
        "run-game.cmd"
    } else {
        "run-game.sh"
    }
}

/// Downloads or copies, verifies and atomically caches a target template.
///
/// `dry_run` returns without touching the network or the cache. A populated
/// cache entry is reused only after passing the same verification as a fresh
/// download, so a corrupted or wrong-version cache is rejected too.
pub(super) fn resolve(
    source: Option<&str>,
    project_root: &Path,
    target: &Target,
    dry_run: bool,
) -> Result<Option<ResolvedTemplate>> {
    let Some(source) = source else {
        return Ok(None);
    };
    if dry_run {
        return Ok(None);
    }

    let version = env!("CARGO_PKG_VERSION");
    let cache_root = project_root.join(".aurora/templates");
    fs::create_dir_all(&cache_root)?;
    let cache = cache_root.join(format!("{version}-{}", target.manifest_target));
    if cache.try_exists()? {
        ensure!(
            cache.is_dir() && !cache.is_symlink(),
            "Template cache entry is not a directory: {}",
            cache.display()
        );
        let manifest = verify(&cache, version, target)?;
        return Ok(Some(resolved(&cache, &manifest)?));
    }

    let staging = tempfile::Builder::new()
        .prefix(".template-")
        .tempdir_in(&cache_root)?;
    stage(source, staging.path())?;
    let manifest = verify(staging.path(), version, target)?;
    let staged = staging.keep();
    match fs::rename(&staged, &cache) {
        Ok(()) => Ok(Some(resolved(&cache, &manifest)?)),
        Err(error) => {
            let _ = fs::remove_dir_all(&staged);
            if cache.try_exists()? {
                let existing = verify(&cache, version, target)?;
                Ok(Some(resolved(&cache, &existing)?))
            } else {
                Err(error.into())
            }
        }
    }
}

fn resolved(root: &Path, manifest: &TemplateManifest) -> Result<ResolvedTemplate> {
    let player = root.join(player_name(&manifest.target));
    let launcher = root.join(launcher_name(&manifest.target));
    for path in [&player, &launcher] {
        ensure!(
            path.is_file(),
            "Template does not contain the required runtime {}",
            path.display()
        );
    }
    Ok(ResolvedTemplate {
        directory: root.to_path_buf(),
        player,
        launcher,
    })
}

fn stage(source: &str, destination: &Path) -> Result<()> {
    let work = tempfile::tempdir()?;
    let archive = work.path().join("template.tar.gz");
    if source.starts_with("http://") || source.starts_with("https://") {
        let status = Command::new("curl")
            .args(["-fsSL", "--retry", "3", "--max-time", "120"])
            .arg(source)
            .arg("-o")
            .arg(&archive)
            .status()
            .context("Downloading target template with curl")?;
        ensure!(
            status.success(),
            "Could not download target template {source}"
        );
    } else {
        let path = Path::new(source);
        let metadata = fs::symlink_metadata(path)
            .with_context(|| format!("Resolving target template {}", path.display()))?;
        ensure!(
            !metadata.file_type().is_symlink() && metadata.is_file(),
            "Target template must be a regular, non-symlinked archive: {}",
            path.display()
        );
        fs::copy(path, &archive)
            .with_context(|| format!("Copying target template from {}", path.display()))?;
    }
    ensure!(
        fs::metadata(&archive)?.len() <= MAX_BYTES,
        "Target template archive is too large"
    );

    reject_archive_links(&archive)?;
    let entries = archive_entries(&archive)?;
    ensure!(
        entries.len() <= MAX_FILES + 1,
        "Target template archive has too many entries"
    );
    for entry in &entries {
        ensure!(
            safe_path(entry) && !entry.ends_with('/'),
            "Unsafe target template archive entry: {entry}"
        );
    }
    let status = Command::new("tar")
        .arg("-xzf")
        .arg(&archive)
        .arg("-C")
        .arg(destination)
        .status()
        .context("Extracting target template with tar")?;
    ensure!(status.success(), "Could not extract target template");
    Ok(())
}

/// Rejects links before extraction so they cannot redirect a later file entry.
fn reject_archive_links(archive: &Path) -> Result<()> {
    let output = Command::new("tar")
        .arg("-tvzf")
        .arg(archive)
        .output()
        .context("Inspecting target template archive with tar")?;
    ensure!(
        output.status.success(),
        "Could not inspect target template archive"
    );
    let text = String::from_utf8(output.stdout).context("Template archive listing is not UTF-8")?;
    for line in text.lines() {
        let kind = line.as_bytes().first().copied();
        ensure!(
            kind != Some(b'l') && kind != Some(b'h'),
            "Target template archive contains a link entry"
        );
    }
    Ok(())
}

fn archive_entries(archive: &Path) -> Result<Vec<String>> {
    let output = Command::new("tar")
        .arg("-tzf")
        .arg(archive)
        .output()
        .context("Listing target template archive with tar")?;
    ensure!(
        output.status.success(),
        "Could not read target template archive: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    let text = String::from_utf8(output.stdout).context("Template archive names are not UTF-8")?;
    Ok(text
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}

fn verify(root: &Path, version: &str, target: &Target) -> Result<TemplateManifest> {
    let path = root.join(MANIFEST);
    let bytes = fs::read(&path)
        .with_context(|| format!("Reading target template manifest {}", path.display()))?;
    ensure!(
        bytes.len() as u64 <= MAX_MANIFEST_BYTES,
        "Target template manifest is too large"
    );
    let manifest: TemplateManifest =
        serde_json::from_slice(&bytes).context("Invalid target template manifest")?;
    ensure!(
        manifest.version == FORMAT_VERSION,
        "Unsupported target template format version {}",
        manifest.version
    );
    ensure!(
        manifest.engine_version == version,
        "Target template was built for engine {} but this CLI is {version}",
        manifest.engine_version
    );
    ensure!(
        manifest.target == target.manifest_target,
        "Target template is for {} but {} was requested",
        manifest.target,
        target.manifest_target
    );
    ensure!(
        manifest.triple == target.triple,
        "Target template triple is {} but {} was expected",
        manifest.triple,
        target.triple
    );
    ensure!(
        !manifest.files.is_empty() && manifest.files.len() <= MAX_FILES,
        "Target template has an invalid file count"
    );

    let mut previous: Option<&str> = None;
    let mut total = 0u64;
    for file in &manifest.files {
        ensure!(
            safe_path(&file.path) && file.path != MANIFEST,
            "Invalid target template file path: {}",
            file.path
        );
        ensure!(
            is_sha256(&file.sha256),
            "Invalid template file hash: {}",
            file.path
        );
        ensure!(
            previous.is_none_or(|path| path < file.path.as_str()),
            "Target template files must be unique and sorted"
        );
        previous = Some(&file.path);
        let source = root.join(&file.path);
        let metadata = fs::symlink_metadata(&source)
            .with_context(|| format!("Resolving template file {}", source.display()))?;
        ensure!(
            !metadata.file_type().is_symlink() && metadata.is_file(),
            "Template file is missing or a symlink: {}",
            file.path
        );
        total = total.saturating_add(metadata.len());
        ensure!(total <= MAX_BYTES, "Target template files are too large");
        ensure!(
            file_hash(&source)? == file.sha256,
            "Target template file hash mismatch: {}",
            file.path
        );
    }
    ensure!(
        is_sha256(&manifest.bundle_digest)
            && manifest.bundle_digest == bundle_digest(&manifest.files),
        "Target template bundle digest mismatch"
    );

    let mut actual = BTreeSet::new();
    collect_names(root, &mut actual)?;
    let mut expected: BTreeSet<String> = manifest
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect();
    expected.insert(MANIFEST.to_owned());
    ensure!(
        actual == expected,
        "Target template contains unexpected files"
    );
    Ok(manifest)
}

fn collect_names(directory: &Path, names: &mut BTreeSet<String>) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let kind = fs::symlink_metadata(entry.path())?.file_type();
        ensure!(
            !kind.is_symlink(),
            "Target template contains a symlink: {}",
            entry.path().display()
        );
        ensure!(
            kind.is_file(),
            "Target template contains an unexpected entry: {}",
            entry.path().display()
        );
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("Template filenames must be UTF-8"))?;
        names.insert(name);
    }
    Ok(())
}

fn file_hash(path: &Path) -> Result<String> {
    let mut reader = BufReader::new(fs::File::open(path)?);
    let mut hash = Sha256::new();
    let mut buffer = [0; 16 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

/// Hashes sorted path/hash pairs, matching the export manifest's bundle digest.
fn bundle_digest(files: &[TemplateFile]) -> String {
    let mut sorted: Vec<_> = files.iter().collect();
    sorted.sort_by(|left, right| left.path.cmp(&right.path));
    let mut hash = Sha256::new();
    for file in sorted {
        hash.update((file.path.len() as u64).to_le_bytes());
        hash.update(file.path.as_bytes());
        hash.update(file.sha256.as_bytes());
    }
    format!("{:x}", hash.finalize())
}

fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(all(test, unix))]
mod tests {
    use anyhow::bail;

    use super::*;

    const VERSION: &str = env!("CARGO_PKG_VERSION");

    fn path_arg(path: &Path) -> Result<&str> {
        path.to_str().context("Test path must be UTF-8")
    }

    fn rejection(result: Result<Option<ResolvedTemplate>>) -> Result<String> {
        match result {
            Ok(_) => bail!("expected the template to be rejected"),
            Err(error) => Ok(format!("{error:#}")),
        }
    }

    fn manifest(
        files: &[(&str, &[u8])],
        engine_version: &str,
        target: &str,
        triple: &str,
    ) -> TemplateManifest {
        let mut entries: Vec<_> = files
            .iter()
            .map(|(path, bytes)| TemplateFile {
                path: (*path).to_owned(),
                sha256: hash(bytes),
            })
            .collect();
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        TemplateManifest {
            version: FORMAT_VERSION,
            engine_version: engine_version.to_owned(),
            target: target.to_owned(),
            triple: triple.to_owned(),
            bundle_digest: bundle_digest(&entries),
            files: entries,
        }
    }

    fn hash(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    fn archive(stage: &Path, names: &[&str], destination: &Path) -> Result<()> {
        let status = Command::new("tar")
            .arg("-czf")
            .arg(destination)
            .arg("-C")
            .arg(stage)
            .args(names)
            .status()?;
        ensure!(status.success(), "Could not create test template archive");
        Ok(())
    }

    fn fixture(
        target: &Target,
        engine_version: &str,
        corrupt: bool,
        triple: Option<&str>,
    ) -> Result<(tempfile::TempDir, PathBuf, Vec<u8>)> {
        let stage = tempfile::tempdir()?;
        let player = player_name(target.manifest_target);
        let launcher = launcher_name(target.manifest_target);
        let player_bytes = b"cross target player".to_vec();
        let launcher_bytes = b"cross target launcher".to_vec();
        fs::write(stage.path().join(player), &player_bytes)?;
        fs::write(stage.path().join(launcher), &launcher_bytes)?;
        let document = manifest(
            &[(player, &player_bytes), (launcher, &launcher_bytes)],
            engine_version,
            target.manifest_target,
            triple.unwrap_or(target.triple),
        );
        fs::write(
            stage.path().join(MANIFEST),
            serde_json::to_vec_pretty(&document)?,
        )?;
        if corrupt {
            fs::write(stage.path().join(player), b"tampered")?;
        }
        let work = tempfile::tempdir()?;
        let path = work.path().join("template.tar.gz");
        archive(stage.path(), &[MANIFEST, player, launcher], &path)?;
        Ok((work, path, player_bytes))
    }

    #[test]
    fn accepts_verifies_and_reuses_a_cached_template() -> Result<()> {
        let target = Target::parse("linux")?;
        let (_work, archive, player_bytes) = fixture(&target, VERSION, false, None)?;
        let project = tempfile::tempdir()?;
        let source = archive.to_str().context("Test path must be UTF-8")?;

        let first = resolve(Some(source), project.path(), &target, false)?
            .context("A valid template must be accepted")?;
        assert_eq!(fs::read(&first.player)?, player_bytes);
        assert!(first.launcher.is_file());
        assert_eq!(
            first.directory,
            project
                .path()
                .join(".aurora/templates")
                .join(format!("{VERSION}-{}", target.manifest_target))
        );

        // The cache is authoritative once populated: a removed or unreachable
        // source must still export from the verified copy.
        fs::remove_file(&archive)?;
        let second = resolve(Some(source), project.path(), &target, false)?
            .context("A verified cache entry must be reused")?;
        assert_eq!(second.directory, first.directory);
        assert_eq!(fs::read(second.player)?, player_bytes);
        Ok(())
    }

    #[test]
    fn rejects_corrupt_wrong_version_and_wrong_target_templates() -> Result<()> {
        let target = Target::parse("linux")?;
        let (_work, corrupt, _) = fixture(&target, VERSION, true, None)?;
        let project = tempfile::tempdir()?;
        let error = rejection(resolve(
            Some(path_arg(&corrupt)?),
            project.path(),
            &target,
            false,
        ))?;
        assert!(error.contains("hash mismatch"), "{error}");

        let project = tempfile::tempdir()?;
        let (_work, wrong_version, _) = fixture(&target, "9.9.9", false, None)?;
        let error = rejection(resolve(
            Some(path_arg(&wrong_version)?),
            project.path(),
            &target,
            false,
        ))?;
        assert!(error.contains("engine"), "{error}");

        let project = tempfile::tempdir()?;
        let (_work, wrong_triple, _) =
            fixture(&target, VERSION, false, Some("unknown-unknown-unknown"))?;
        let error = rejection(resolve(
            Some(path_arg(&wrong_triple)?),
            project.path(),
            &target,
            false,
        ))?;
        assert!(error.contains("triple"), "{error}");

        let project = tempfile::tempdir()?;
        let (_work, other, _) = fixture(&Target::parse("macos")?, VERSION, false, None)?;
        let error = rejection(resolve(
            Some(path_arg(&other)?),
            project.path(),
            &target,
            false,
        ))?;
        assert!(error.contains("macos-aarch64"), "{error}");
        Ok(())
    }

    #[test]
    fn rejects_archive_link_entries_before_extraction() -> Result<()> {
        use std::os::unix::fs::symlink;

        let staging = tempfile::tempdir()?;
        symlink("/etc/passwd", staging.path().join("link"))?;
        let work = tempfile::tempdir()?;
        let archive_path = work.path().join("links.tar.gz");
        archive(staging.path(), &["link"], &archive_path)?;
        let destination = tempfile::tempdir()?;
        assert!(stage(path_arg(&archive_path)?, destination.path()).is_err());
        Ok(())
    }

    #[test]
    fn dry_run_never_downloads_or_caches_a_template() -> Result<()> {
        let project = tempfile::tempdir()?;
        let target = Target::parse("linux")?;
        let result = resolve(
            Some("https://127.0.0.1:1/not-reachable.tar.gz"),
            project.path(),
            &target,
            true,
        )?;
        assert!(result.is_none());
        assert!(!project.path().join(".aurora").exists());
        Ok(())
    }

    #[test]
    fn runtime_names_follow_the_manifest_target() {
        assert_eq!(player_name("windows-x86_64"), "aurora-player.exe");
        assert_eq!(player_name("linux-x86_64"), "aurora-player");
        assert_eq!(launcher_name("windows-x86_64"), "run-game.cmd");
        assert_eq!(launcher_name("macos-aarch64"), "run-game.sh");
    }
}
