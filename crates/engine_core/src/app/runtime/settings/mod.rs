//! Host-owned project config and per-user secrets for script sessions.

use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

const MAX_FILE: u64 = 1024 * 1024;
const MAX_VALUE: usize = 1512;
const MAX_ENTRIES: usize = 256;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Store {
    version: u32,
    values: BTreeMap<String, String>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            version: 1,
            values: BTreeMap::new(),
        }
    }
}

/// Config and secrets loaded by the host and never written into script sources.
pub(super) struct GameSettings {
    config_path: Option<PathBuf>,
    config: Store,
    secrets: Store,
}

impl GameSettings {
    /// Loads both stores. Missing files are treated as empty stores.
    pub(super) fn load(config: Option<&Path>, secrets: Option<&Path>) -> Result<Self> {
        let mut loaded = Self {
            config_path: config.map(Path::to_path_buf),
            config: read(config)?,
            secrets: read(secrets)?,
        };
        loaded
            .config
            .values
            .retain(|_, value| validate_value(value).is_ok());
        loaded
            .secrets
            .values
            .retain(|_, value| validate_value(value).is_ok());
        Ok(loaded)
    }

    /// Config entries in deterministic order for session delivery.
    pub(super) fn config_entries(&self) -> impl Iterator<Item = (&str, &str)> {
        pairs(&self.config)
    }

    /// Secret entries in deterministic order for session delivery.
    pub(super) fn secret_entries(&self) -> impl Iterator<Item = (&str, &str)> {
        pairs(&self.secrets)
    }

    /// Persists one config value with a lock, fsync, and atomic replacement.
    pub(super) fn set_config(&mut self, name: &str, value: &str) -> Result<()> {
        validate_value(value)?;
        ensure!(name.len() <= MAX_VALUE, "Config name is too long");
        let Some(path) = &self.config_path else {
            anyhow::bail!("No config path was supplied to the player");
        };
        let directory = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(directory)?;
        let mut lock_name = path
            .file_name()
            .context("Missing config filename")?
            .to_os_string();
        lock_name.push(".lock");
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(directory.join(lock_name))?;
        lock.lock()?;
        let mut stored = read(Some(path.as_path()))?;
        stored.values.insert(name.to_owned(), value.to_owned());
        ensure!(
            stored.values.len() <= MAX_ENTRIES,
            "Config has too many entries"
        );
        write(directory, path, &stored, false)?;
        self.config = stored;
        Ok(())
    }
}

fn validate_value(value: &str) -> Result<()> {
    ensure!(
        value.len() <= MAX_VALUE,
        "Value exceeds the 1512-byte limit"
    );
    Ok(())
}

fn pairs(store: &Store) -> impl Iterator<Item = (&str, &str)> {
    store
        .values
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
}

fn read(path: Option<&Path>) -> Result<Store> {
    let Some(path) = path else {
        return Ok(Store::default());
    };
    let mut bytes = Vec::new();
    match File::open(path) {
        Ok(file) => {
            file.take(MAX_FILE + 1).read_to_end(&mut bytes)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Store::default()),
        Err(error) => return Err(error.into()),
    }
    ensure!(bytes.len() as u64 <= MAX_FILE, "Settings file is too large");
    let store: Store =
        serde_json::from_slice(&bytes).with_context(|| format!("Reading {}", path.display()))?;
    ensure!(store.version == 1, "Unsupported settings version");
    ensure!(
        store.values.len() <= MAX_ENTRIES,
        "Settings have too many entries"
    );
    Ok(store)
}

fn write(directory: &Path, path: &Path, store: &Store, secret: bool) -> Result<()> {
    let mut staged = tempfile::NamedTempFile::new_in(directory)?;
    staged.write_all(&serde_json::to_vec_pretty(store)?)?;
    staged.write_all(b"\n")?;
    staged.as_file().sync_all()?;
    if secret {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(staged.path(), fs::Permissions::from_mode(0o600))?;
        }
    }
    staged
        .persist(path)
        .with_context(|| format!("Saving {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_values_round_trip_and_secrets_stay_in_their_own_file() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let config = scratch.path().join("config.json");
        let secrets = scratch.path().join(".secrets/secrets.json");
        fs::create_dir_all(secrets.parent().context("Missing secrets directory")?)?;
        let mut settings = GameSettings::load(Some(&config), Some(&secrets))?;
        assert_eq!(settings.config_entries().count(), 0);
        settings.set_config("token", "abc")?;
        assert_eq!(
            settings.config_entries().collect::<Vec<_>>(),
            vec![("token", "abc")]
        );
        assert_eq!(
            GameSettings::load(Some(&config), Some(&secrets))?
                .config_entries()
                .collect::<Vec<_>>(),
            vec![("token", "abc")]
        );
        assert_eq!(settings.secret_entries().count(), 0);
        assert!(
            settings
                .set_config("token", &"x".repeat(MAX_VALUE + 1))
                .is_err()
        );
        Ok(())
    }
}
