//! Game-script child configuration and per-frame command exchange.

use std::path::PathBuf;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub(crate) use linux::Session;

/// Configures the game logic process launched by the player.
pub enum Script {
    /// Run a scriptc executable with the native shared-memory transport.
    Native(PathBuf),
    /// Run a Bun or Node launcher with the development socket transport.
    Development(PathBuf),
}

#[cfg(not(target_os = "linux"))]
pub(crate) struct Session;

#[cfg(not(target_os = "linux"))]
impl Session {
    pub(crate) fn spawn(_: &Script) -> anyhow::Result<Self> {
        anyhow::bail!("Script IPC is currently available on Linux")
    }

    pub(crate) fn exchange(
        &mut self,
        _: script_api::Kind,
        _: u64,
        _: [f32; 2],
    ) -> anyhow::Result<Option<[f32; 2]>> {
        anyhow::bail!("Script IPC is currently available on Linux")
    }
}
