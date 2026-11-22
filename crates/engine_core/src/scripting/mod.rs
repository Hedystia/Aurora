//! Game-script child configuration and per-frame command exchange.

use std::path::PathBuf;

pub(crate) enum Command {
    SetPosition([f32; 2]),
    SetLinearVelocity([f32; 2]),
    PlayOneShot(usize),
    Pause,
    Resume,
    ChangeScene(usize),
}

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
        _: u64,
    ) -> anyhow::Result<Option<Command>> {
        anyhow::bail!("Script IPC is currently available on Linux")
    }
}
