//! Loads portable native bundles without exposing manifest-supplied paths.

use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use engine_core::app::{SceneProgram, bundle};

pub(super) fn load(directory: &Path) -> Result<(Vec<PathBuf>, Vec<SceneProgram>)> {
    let root = directory.canonicalize()?;
    let export_manifest_path = root.join(bundle::EXPORT_MANIFEST);
    if export_manifest_path.try_exists()? {
        let manifest: bundle::ExportManifest =
            serde_json::from_slice(&read_manifest(&root, &export_manifest_path)?)?;
        manifest.verify(&root)?;
    }

    let manifest_path = root.join(bundle::MANIFEST);
    if !manifest_path.try_exists()? {
        let bindings = serde_json::from_slice(&read_manifest(&root, &root.join("bindings.json"))?)?;
        let (scene, program) = member(&root, &root, bindings)?;
        return Ok((vec![scene], vec![program]));
    }

    let manifest: bundle::Manifest =
        serde_json::from_slice(&read_manifest(&root, &manifest_path)?)?;
    manifest.validate()?;
    manifest
        .scenes
        .into_iter()
        .enumerate()
        .map(|(index, scene)| {
            member(
                &root,
                &root.join(bundle::member_directory(index)),
                scene.bindings,
            )
            .with_context(|| format!("Opening bundled scene {:?}", scene.name))
        })
        .collect()
}

fn member(root: &Path, directory: &Path, bindings: Vec<usize>) -> Result<(PathBuf, SceneProgram)> {
    let scene = bundled_file(root, &directory.join("main.scene"))?;
    let executable = if bindings.is_empty() {
        None
    } else {
        Some(bundled_file(root, &directory.join("game"))?)
    };
    Ok((
        scene,
        SceneProgram {
            executable,
            bindings,
        },
    ))
}

fn read_manifest(root: &Path, path: &Path) -> Result<Vec<u8>> {
    let mut manifest = Vec::new();
    File::open(bundled_file(root, path)?)?
        .take(bundle::MAX_MANIFEST_BYTES as u64 + 1)
        .read_to_end(&mut manifest)?;
    ensure!(
        manifest.len() <= bundle::MAX_MANIFEST_BYTES,
        "Bundle manifest is too large"
    );
    Ok(manifest)
}

/// Finds a bundle beside the executable in a desktop app layout.
///
/// For `Game.app/Contents/MacOS/aurora-player`, the package lives at
/// `Game.app/Contents/Resources/game`. Explicit command-line modes never use
/// this fallback.
pub(super) fn adjacent() -> Option<PathBuf> {
    adjacent_to(&std::env::current_exe().ok()?)
}

fn adjacent_to(executable: &Path) -> Option<PathBuf> {
    let game = executable
        .parent()?
        .parent()?
        .join("Resources")
        .join("game");
    [bundle::EXPORT_MANIFEST, bundle::MANIFEST, "bindings.json"]
        .iter()
        .any(|name| game.join(name).is_file())
        .then_some(game)
}

fn bundled_file(root: &Path, path: &Path) -> Result<PathBuf> {
    let resolved = path
        .canonicalize()
        .with_context(|| format!("Resolving bundle file {}", path.display()))?;
    ensure!(
        resolved.starts_with(root) && resolved.is_file(),
        "Bundle file must be inside the bundle: {}",
        path.display()
    );
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_a_bundle_beside_a_desktop_executable() -> Result<()> {
        let root = tempfile::tempdir()?;
        let macos = root.path().join("Game.app/Contents/MacOS");
        let game = root.path().join("Game.app/Contents/Resources/game");
        std::fs::create_dir_all(&macos)?;
        std::fs::create_dir_all(&game)?;
        std::fs::write(game.join(bundle::EXPORT_MANIFEST), b"{}")?;
        let executable = macos.join("aurora-player");
        std::fs::write(&executable, b"player")?;
        assert_eq!(adjacent_to(&executable), Some(game));
        Ok(())
    }

    #[test]
    fn ignores_layouts_without_a_bundle() -> Result<()> {
        let root = tempfile::tempdir()?;
        let executable = root.path().join("bin/aurora-player");
        std::fs::create_dir_all(executable.parent().context("parent")?)?;
        std::fs::write(&executable, b"player")?;
        assert_eq!(adjacent_to(&executable), None);
        Ok(())
    }
}
