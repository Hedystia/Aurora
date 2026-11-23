//! Per-user preferences for the project manager.

use std::{
    env, fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub(super) struct Settings {
    pub default_project_directory: PathBuf,
}

mod paths;
use paths::home;
pub(super) use paths::{catalog_file, config_file};

pub(super) fn suggested_directory() -> PathBuf {
    home()
        .or_else(|| env::current_dir().ok())
        .unwrap_or_else(env::temp_dir)
        .join("Aurora")
}

impl Settings {
    pub(super) fn load(file: &Path) -> Result<Option<Self>> {
        match fs::read(file) {
            Ok(bytes) => {
                let saved: Self = serde_json::from_slice(&bytes)?;
                ensure!(
                    saved.default_project_directory.is_absolute(),
                    "Invalid default project folder"
                );
                Ok(Some(saved))
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub(super) fn save(file: &Path, directory: &Path) -> Result<Self> {
        ensure!(
            directory.is_absolute(),
            "{}",
            crate::strings::ABSOLUTE_FOLDER
        );
        fs::create_dir_all(directory)
            .with_context(|| format!("Creating project directory {}", directory.display()))?;
        let saved = Self {
            default_project_directory: directory.canonicalize()?,
        };
        let parent = file.parent().context("Missing settings directory")?;
        fs::create_dir_all(parent)?;
        let mut staged = tempfile::NamedTempFile::new_in(parent)?;
        staged.write_all(&serde_json::to_vec_pretty(&saved)?)?;
        staged.persist(file)?;
        Ok(saved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_and_reloads_changed_directory() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let file = temp.path().join(".aurora/config");
        assert!(Settings::load(&file)?.is_none());
        let first = temp.path().join("First");
        Settings::save(&file, &first)?;
        assert!(first.is_dir());
        let second = temp.path().join("Next");
        Settings::save(&file, &second)?;
        assert_eq!(
            Settings::load(&file)?
                .context("Missing settings")?
                .default_project_directory,
            second.canonicalize()?
        );
        assert!(Settings::save(&file, Path::new("relative")).is_err());
        assert!(first.is_dir());
        Ok(())
    }
}
