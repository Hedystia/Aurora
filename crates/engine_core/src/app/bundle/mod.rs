//! Manifest for a portable bundle with multiple scene-local native programs.

use std::{collections::BTreeSet, path::PathBuf};

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// Filename of the manifest in a multi-scene bundle.
pub const MANIFEST: &str = "scenes.json";

/// Maximum encoded manifest size accepted by the loader.
pub const MAX_MANIFEST_BYTES: usize = 8 * 1024 * 1024;

/// Scene manifest format version.
pub const VERSION: u32 = 1;

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
