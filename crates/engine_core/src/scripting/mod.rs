//! Game-script child configuration and per-frame command exchange.

use std::path::PathBuf;

pub(crate) struct Reply {
    pub(crate) command: Option<Command>,
    pub(crate) enabled: bool,
}

pub(crate) enum Command {
    SetPosition([f32; 2]),
    SetLinearVelocity([f32; 2]),
    SetLinearVelocity3D([f32; 3]),
    PlayOneShot(u64),
    Pause,
    Resume,
    ChangeScene(usize),
    MoveNode(usize, [f32; 2]),
    ScaleNode(usize, [f32; 2]),
    RotateNode(usize, f32),
    SetTextStart(u32),
    SetTextChunk(u32, [u8; 12], u8),
    SetTextEnd(u32),
    ShowLoading(bool),
    ConfigSetStart(u32),
    ConfigSetChunk(u32, [u8; 12], u8),
    ConfigSetEnd(u32),
    ShowNode(usize, bool),
    SelectFrame(usize, usize),
    QueueSound(u64),
    RecordScore(u32),
    CreateNode(u32, Option<u32>),
    InstantiatePrefab(u32, Option<u32>, u32),
    ReparentNode(u32, Option<u32>),
    DestroyNode(u32),
    AddComponent(u32, u32),
    RemoveComponent(u32, u32),
    LoadSpriteStart(u32),
    LoadSpriteChunk(u32, [u8; 12], u8),
    LoadSpriteEnd(u32),
    ReleaseAsset(u64),
    LoadAudioStart(u32),
    LoadAudioChunk(u32, [u8; 12], u8),
    LoadAudioEnd(u32),
    LoadPrefabStart(u32),
    LoadPrefabChunk(u32, [u8; 12], u8),
    LoadPrefabEnd(u32),
    SetAngularVelocity3D([f32; 3]),
    RaycastStart(u32),
    RaycastChunk(u32, [u8; 12], u8),
    RaycastEnd(u32),
}

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::Session;

/// Configures the game logic process launched by the player.
pub enum Script {
    /// Run a scriptc executable with the native shared-memory transport.
    Native(PathBuf),
    /// Run a Bun or Node launcher with the development socket transport.
    Development(PathBuf),
}

#[cfg(not(unix))]
pub(crate) struct Session;

#[cfg(not(unix))]
impl Session {
    pub(crate) fn begin_shutdown(&mut self) {}

    pub(crate) fn spawn(_: &Script) -> anyhow::Result<Self> {
        anyhow::bail!("Script IPC is currently available on Unix hosts (Linux and macOS)")
    }

    pub(crate) fn exchange_control(
        &mut self,
        _: script_api::Kind,
        _: u32,
        _: u64,
        _: [f64; 3],
        _: u64,
    ) -> anyhow::Result<Reply> {
        anyhow::bail!("Script IPC is currently available on Unix hosts (Linux and macOS)")
    }

    pub(crate) fn exchange(
        &mut self,
        _: script_api::Kind,
        _: u32,
        _: u64,
        _: [f32; 2],
        _: u64,
    ) -> anyhow::Result<Reply> {
        anyhow::bail!("Script IPC is currently available on Unix hosts (Linux and macOS)")
    }
}
