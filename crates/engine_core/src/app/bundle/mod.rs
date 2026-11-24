//! Manifests for portable scene bundles and their package integrity metadata.

use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{BufReader, Read},
    path::{Path, PathBuf},
};

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Filename of the manifest in a multi-scene bundle.
pub const MANIFEST: &str = "scenes.json";

/// Filename of the package metadata and integrity manifest.
pub const EXPORT_MANIFEST: &str = "export.json";

/// Maximum encoded manifest size accepted by the loader.
pub const MAX_MANIFEST_BYTES: usize = 8 * 1024 * 1024;

/// Scene manifest format version.
pub const VERSION: u32 = 1;

/// Export package manifest format version.
pub const EXPORT_VERSION: u32 = 1;

/// Versioned package metadata emitted by the editor and verified by the player.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExportManifest {
    /// The package manifest format version.
    pub version: u32,
    /// Aurora engine version used to create the package.
    pub engine_version: String,
    /// Operating system and architecture of the exporting editor.
    pub target: String,
    /// Hash of the ordered paths and content hashes in `files`.
    pub source_bundle_sha256: String,
    /// Packaged files, excluding this manifest.
    pub files: Vec<ExportFile>,
}

/// Integrity record for one file in an exported package.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExportFile {
    /// Package-relative path with `/` separators.
    pub path: String,
    /// Lowercase hexadecimal SHA-256 digest of the file contents.
    pub sha256: String,
}

impl ExportManifest {
    /// Creates package metadata by hashing every regular file below `root`.
    pub fn create(root: &Path, engine_version: &str, target: &str) -> Result<Self> {
        ensure!(!engine_version.trim().is_empty(), "Missing engine version");
        ensure!(!target.trim().is_empty(), "Missing export target");
        let root = root.canonicalize()?;
        let mut files = Vec::new();
        collect_files(&root, &root, &mut files)?;
        files.sort_by(|left, right| left.path.cmp(&right.path));
        let source_bundle_sha256 = bundle_hash(&files);
        Ok(Self {
            version: EXPORT_VERSION,
            engine_version: engine_version.to_owned(),
            target: target.to_owned(),
            source_bundle_sha256,
            files,
        })
    }

    /// Validates the package metadata and confirms all listed files match their hashes.
    pub fn verify(&self, root: &Path) -> Result<()> {
        ensure!(
            self.version == EXPORT_VERSION,
            "Unsupported export package version"
        );
        ensure!(
            !self.engine_version.trim().is_empty(),
            "Missing engine version"
        );
        ensure!(!self.target.trim().is_empty(), "Missing export target");
        ensure!(!self.files.is_empty(), "Export package contains no files");

        let mut expected = BTreeSet::new();
        let mut previous = None;
        for file in &self.files {
            ensure!(
                safe_relative_path(&file.path),
                "Invalid export package path: {}",
                file.path
            );
            ensure!(
                is_sha256(&file.sha256),
                "Invalid SHA-256 for export package file: {}",
                file.path
            );
            ensure!(
                previous.is_none_or(|path: &str| path < file.path.as_str()),
                "Export package files must be unique and sorted"
            );
            previous = Some(file.path.as_str());
            expected.insert(file.path.as_str());
        }
        ensure!(
            is_sha256(&self.source_bundle_sha256)
                && self.source_bundle_sha256 == bundle_hash(&self.files),
            "Invalid export package source hash"
        );

        let root = root.canonicalize()?;
        let mut actual = Vec::new();
        collect_files(&root, &root, &mut actual)?;
        actual.sort_by(|left, right| left.path.cmp(&right.path));
        ensure!(
            actual.len() == self.files.len()
                && actual
                    .iter()
                    .map(|file| file.path.as_str())
                    .eq(expected.iter().copied()),
            "Export package file list does not match the manifest"
        );
        for (actual, expected) in actual.iter().zip(&self.files) {
            ensure!(
                actual.sha256 == expected.sha256,
                "Export package file hash mismatch: {}",
                expected.path
            );
        }
        Ok(())
    }
}

fn collect_files(root: &Path, directory: &Path, files: &mut Vec<ExportFile>) -> Result<()> {
    let mut entries = fs::read_dir(directory)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let kind = fs::symlink_metadata(&path)?.file_type();
        if kind.is_symlink() {
            anyhow::bail!(
                "Export packages cannot contain symlinks: {}",
                path.display()
            );
        }
        if kind.is_dir() {
            collect_files(root, &path, files)?;
        } else if kind.is_file() {
            let relative = path.strip_prefix(root)?;
            let name = relative
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Export package paths must be UTF-8"))?
                .replace(std::path::MAIN_SEPARATOR, "/");
            if name == EXPORT_MANIFEST {
                continue;
            }
            files.push(ExportFile {
                path: name,
                sha256: file_hash(&path)?,
            });
        } else {
            anyhow::bail!("Unsupported export package entry: {}", path.display());
        }
    }
    Ok(())
}

fn file_hash(path: &Path) -> Result<String> {
    let mut reader = BufReader::new(File::open(path)?);
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

fn bundle_hash(files: &[ExportFile]) -> String {
    let mut hash = Sha256::new();
    for file in files {
        hash.update((file.path.len() as u64).to_le_bytes());
        hash.update(file.path.as_bytes());
        hash.update(file.sha256.as_bytes());
    }
    format!("{:x}", hash.finalize())
}

fn safe_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && path
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
        && path != EXPORT_MANIFEST
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Bundle scenes in switching order. The first member is loaded on startup.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// The manifest format version.
    pub version: u32,
    /// Scene descriptors indexed by the script director.
    pub scenes: Vec<Member>,
}

/// Descriptor for one exported scene.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    /// Name exposed to scripts.
    pub name: String,
    /// Authored node indices in the corresponding native program's binding order.
    pub bindings: Vec<usize>,
}

impl Manifest {
    /// Validates the version and scene names before the bundle can be opened.
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == VERSION, "Unsupported scene bundle version");
        ensure!(!self.scenes.is_empty(), "Scene bundle has no entry scene");
        let mut names = BTreeSet::new();
        for scene in &self.scenes {
            ensure!(
                !scene.name.trim().is_empty() && names.insert(&scene.name),
                "Empty or duplicate scene name: {:?}",
                scene.name
            );
        }
        Ok(())
    }
}

/// Returns the portable member directory from its validated manifest index.
pub fn member_directory(index: usize) -> PathBuf {
    PathBuf::from("scenes").join(index.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_manifest_verifies_content_and_exact_file_list() -> Result<()> {
        let directory = tempfile::tempdir()?;
        fs::create_dir(directory.path().join("assets"))?;
        fs::write(directory.path().join("main.scene"), b"scene")?;
        fs::write(directory.path().join("assets/image.png"), b"image")?;
        let manifest = ExportManifest::create(directory.path(), "0.1.0", "linux-x86_64")?;
        manifest.verify(directory.path())?;

        fs::write(directory.path().join("main.scene"), b"changed")?;
        assert!(manifest.verify(directory.path()).is_err());
        fs::write(directory.path().join("main.scene"), b"scene")?;
        fs::write(directory.path().join("extra"), b"unlisted")?;
        assert!(manifest.verify(directory.path()).is_err());
        Ok(())
    }

    #[test]
    fn export_manifest_rejects_unsafe_paths() {
        assert!(!safe_relative_path("../outside"));
        assert!(!safe_relative_path("/absolute"));
        assert!(!safe_relative_path("assets\\image.png"));
        assert!(!safe_relative_path(EXPORT_MANIFEST));
        assert!(safe_relative_path("assets/image.png"));
    }
}
