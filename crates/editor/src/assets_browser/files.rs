//! Clipboard and undo helpers for project files.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

/// Names a copy the way a Linux file manager does: `file (copy).png`, then
/// `file (copy 2).png`, and so on until the destination is free.
pub(crate) fn copy_name(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path
        .file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "copy".to_owned());
    let extension = path
        .extension()
        .map(|value| value.to_string_lossy().into_owned());
    for number in 1..10_000 {
        let suffix = if number == 1 {
            " (copy)".to_owned()
        } else {
            format!(" (copy {number})")
        };
        let name = match &extension {
            Some(extension) => format!("{stem}{suffix}.{extension}"),
            None => format!("{stem}{suffix}"),
        };
        let candidate = path.with_file_name(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    path.with_file_name(format!("{stem} (copy).tmp"))
}

/// Copies a file or a whole directory into `destination`, returning the new root path.
pub(crate) fn copy_entry(source: &Path, destination: &Path) -> Result<PathBuf> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.is_dir() {
        fs::create_dir_all(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_entry(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, destination).with_context(|| format!("Copying {}", source.display()))?;
    }
    Ok(destination.to_path_buf())
}

/// One reversible FileSystem change.
pub(crate) enum FileChange {
    /// Paths created by a copy or duplicate; undo removes them.
    Created(Vec<PathBuf>),
    /// Paths deleted, with their contents preserved in a temporary directory.
    Removed {
        backup: tempfile::TempDir,
        entries: Vec<(PathBuf, PathBuf)>,
    },
}

impl FileChange {
    /// Captures files before they are removed so the deletion can be undone.
    pub(crate) fn snapshot_removed(root: &Path, paths: &[PathBuf]) -> Result<Self> {
        let backup = tempfile::tempdir()?;
        let mut entries = Vec::new();
        for path in paths {
            let relative = path
                .strip_prefix(root)
                .with_context(|| format!("{} escapes the project", path.display()))?;
            copy_entry(path, &backup.path().join(relative))?;
            entries.push((path.clone(), relative.to_path_buf()));
        }
        Ok(Self::Removed { backup, entries })
    }

    /// Reverses the change.
    pub(crate) fn undo(&self) -> Result<()> {
        match self {
            Self::Created(paths) => {
                for path in paths.iter().rev() {
                    remove(path)?;
                }
                Ok(())
            }
            Self::Removed { backup, entries } => {
                for (original, relative) in entries {
                    copy_entry(&backup.path().join(relative), original)?;
                }
                Ok(())
            }
        }
    }
}

fn remove(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    }
    .with_context(|| format!("Removing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_get_numbered_linux_style_names() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("level.scene");
        fs::write(&path, "{}")?;
        let first = copy_name(&path);
        assert_eq!(
            first.file_name(),
            Some(std::ffi::OsStr::new("level (copy).scene"))
        );
        fs::write(&first, "{}")?;
        assert_eq!(
            copy_name(&path).file_name(),
            Some(std::ffi::OsStr::new("level (copy 2).scene"))
        );
        Ok(())
    }

    #[test]
    fn deleting_a_file_can_be_undone() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("notes.txt");
        fs::write(&path, "backup")?;
        let change = FileChange::snapshot_removed(temp.path(), std::slice::from_ref(&path))?;
        fs::remove_file(&path)?;
        change.undo()?;
        assert_eq!(fs::read_to_string(&path)?, "backup");
        Ok(())
    }
}
