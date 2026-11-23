//! Native user configuration and catalog locations.
use std::{env, path::PathBuf};

pub(crate) fn config_file() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let location = absolute_env("APPDATA").map(|dir| dir.join("Aurora"));
    #[cfg(target_os = "macos")]
    let location = home().map(|dir| dir.join("Library/Application Support/Aurora"));
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let location = home().map(|dir| dir.join(".aurora"));
    location.map(|dir| dir.join("config"))
}

pub(crate) fn catalog_file() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    let location = absolute_env("XDG_DATA_HOME")
        .or_else(|| home().map(|dir| dir.join(".local/share")))
        .map(|dir| dir.join("aurora"));
    #[cfg(not(target_os = "linux"))]
    let location = config_file().and_then(|file| file.parent().map(|dir| dir.to_path_buf()));
    location.map(|dir| dir.join("projects.json"))
}

fn absolute_env(key: &str) -> Option<PathBuf> {
    env::var_os(key)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

pub(crate) fn home() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let key = "USERPROFILE";
    #[cfg(not(target_os = "windows"))]
    let key = "HOME";
    absolute_env(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_configuration_and_catalog() -> Result<(), &'static str> {
        #[cfg(target_os = "windows")]
        let directory = absolute_env("APPDATA").ok_or("Missing APPDATA")?.join("Aurora");
        #[cfg(target_os = "macos")]
        let directory = home().ok_or("Missing home")?.join("Library/Application Support/Aurora");
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let directory = home().ok_or("Missing home")?.join(".aurora");
        assert_eq!(config_file(), Some(directory.join("config")));
        #[cfg(target_os = "linux")]
        let directory = absolute_env("XDG_DATA_HOME")
            .unwrap_or(home().ok_or("Missing home")?.join(".local/share"))
            .join("aurora");
        assert_eq!(catalog_file(), Some(directory.join("projects.json")));
        Ok(())
    }
}
