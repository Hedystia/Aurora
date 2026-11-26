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
    OverlapStart(u32),
    OverlapChunk(u32, [u8; 12], u8),
    OverlapEnd(u32),
}

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::Session;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::Session;

/// Reply decoding shared by the Windows named-pipe session.
///
/// The Unix session keeps its own copy because that module is owned elsewhere;
/// both implementations decode the same 60-byte envelope identically.
#[cfg(windows)]
pub(crate) mod decode {
    use anyhow::{Result, ensure};
    use script_api::{Envelope, Kind};

    use super::Command as ScriptCommand;

    pub(crate) fn decode_command(reply: &Envelope) -> Result<Option<ScriptCommand>> {
        match reply.kind {
            Kind::Ack => Ok(None),
            Kind::SetPosition | Kind::SetLinearVelocity => {
                let vector = [reply.x as f32, reply.y as f32];
                ensure!(
                    vector.iter().all(|value| value.is_finite()),
                    "Script vector must be finite"
                );
                if reply.kind == Kind::SetPosition {
                    Ok(Some(ScriptCommand::SetPosition(vector)))
                } else {
                    Ok(Some(ScriptCommand::SetLinearVelocity(vector)))
                }
            }
            Kind::SetLinearVelocity3D => {
                let vector = [reply.x as f32, reply.y as f32, reply.z as f32];
                ensure!(
                    vector.iter().all(|value| value.is_finite()),
                    "Script vector must be finite"
                );
                Ok(Some(ScriptCommand::SetLinearVelocity3D(vector)))
            }
            Kind::PlayOneShot => Ok(Some(ScriptCommand::PlayOneShot(reply.detail))),
            Kind::Pause => Ok(Some(ScriptCommand::Pause)),
            Kind::Resume => Ok(Some(ScriptCommand::Resume)),
            Kind::ChangeScene => Ok(Some(ScriptCommand::ChangeScene(reply.detail.try_into()?))),
            Kind::MoveNode => {
                let position = [reply.x as f32, reply.y as f32];
                ensure!(
                    position.iter().all(|value| value.is_finite()),
                    "Invalid node position"
                );
                Ok(Some(ScriptCommand::MoveNode(
                    reply.detail.try_into()?,
                    position,
                )))
            }
            Kind::ScaleNode => {
                let scale = [reply.x as f32, reply.y as f32];
                ensure!(
                    scale.iter().all(|value| value.is_finite()),
                    "Invalid node scale"
                );
                Ok(Some(ScriptCommand::ScaleNode(
                    reply.detail.try_into()?,
                    scale,
                )))
            }
            Kind::RotateNode => {
                let angle = reply.x as f32;
                ensure!(angle.is_finite(), "Invalid node rotation");
                Ok(Some(ScriptCommand::RotateNode(
                    reply.detail.try_into()?,
                    angle,
                )))
            }
            Kind::SetText => {
                let target = reply.detail as u32;
                let operation = (reply.detail >> 32) as u32;
                match operation {
                    0 => Ok(Some(ScriptCommand::SetTextStart(target))),
                    13 => Ok(Some(ScriptCommand::SetTextEnd(target))),
                    length @ 1..=12 => {
                        Ok(Some(decode_text_chunk(target, length, reply.x, reply.y)?))
                    }
                    _ => anyhow::bail!("Invalid script text operation"),
                }
            }
            Kind::SetLoading if reply.detail <= 1 => {
                Ok(Some(ScriptCommand::ShowLoading(reply.detail == 1)))
            }
            Kind::ConfigSet => {
                let entry = reply.detail as u32;
                let operation = (reply.detail >> 32) as u32;
                match operation {
                    0 => Ok(Some(ScriptCommand::ConfigSetStart(entry))),
                    13 => Ok(Some(ScriptCommand::ConfigSetEnd(entry))),
                    length @ 1..=12 => {
                        let (bytes, length) = decode_chunk_bytes(length, reply.x, reply.y)?;
                        Ok(Some(ScriptCommand::ConfigSetChunk(entry, bytes, length)))
                    }
                    _ => anyhow::bail!("Invalid script config operation"),
                }
            }
            Kind::LoadSprite => {
                let request = reply.detail as u32;
                let operation = (reply.detail >> 32) as u32;
                match operation {
                    0 => Ok(Some(ScriptCommand::LoadSpriteStart(request))),
                    13 => Ok(Some(ScriptCommand::LoadSpriteEnd(request))),
                    length @ 1..=12 => {
                        let (bytes, length) = decode_chunk_bytes(length, reply.x, reply.y)?;
                        Ok(Some(ScriptCommand::LoadSpriteChunk(request, bytes, length)))
                    }
                    _ => anyhow::bail!("Invalid script sprite load operation"),
                }
            }
            Kind::LoadAudio | Kind::LoadPrefab | Kind::Raycast3D | Kind::OverlapQuery => {
                let request = reply.detail as u32;
                let operation = (reply.detail >> 32) as u32;
                let chunk = |length: u32| -> Result<([u8; 12], u8)> {
                    decode_chunk_bytes(length, reply.x, reply.y)
                };
                match (reply.kind, operation) {
                    (_, 0) if reply.kind == Kind::LoadAudio => {
                        Ok(Some(ScriptCommand::LoadAudioStart(request)))
                    }
                    (_, 0) if reply.kind == Kind::LoadPrefab => {
                        Ok(Some(ScriptCommand::LoadPrefabStart(request)))
                    }
                    (Kind::OverlapQuery, 0) => Ok(Some(ScriptCommand::OverlapStart(request))),
                    (_, 0) => Ok(Some(ScriptCommand::RaycastStart(request))),
                    (_, 13) if reply.kind == Kind::LoadAudio => {
                        Ok(Some(ScriptCommand::LoadAudioEnd(request)))
                    }
                    (_, 13) if reply.kind == Kind::LoadPrefab => {
                        Ok(Some(ScriptCommand::LoadPrefabEnd(request)))
                    }
                    (Kind::OverlapQuery, 13) => Ok(Some(ScriptCommand::OverlapEnd(request))),
                    (_, 13) => Ok(Some(ScriptCommand::RaycastEnd(request))),
                    (Kind::LoadAudio, length @ 1..=12) => {
                        let (bytes, length) = chunk(length)?;
                        Ok(Some(ScriptCommand::LoadAudioChunk(request, bytes, length)))
                    }
                    (Kind::LoadPrefab, length @ 1..=12) => {
                        let (bytes, length) = chunk(length)?;
                        Ok(Some(ScriptCommand::LoadPrefabChunk(request, bytes, length)))
                    }
                    (Kind::Raycast3D, length @ 1..=12) => {
                        let (bytes, length) = chunk(length)?;
                        Ok(Some(ScriptCommand::RaycastChunk(request, bytes, length)))
                    }
                    (Kind::OverlapQuery, length @ 1..=12) => {
                        let (bytes, length) = chunk(length)?;
                        Ok(Some(ScriptCommand::OverlapChunk(request, bytes, length)))
                    }
                    _ => anyhow::bail!("Invalid script chunk operation"),
                }
            }
            Kind::SetAngularVelocity3D => {
                let vector = [reply.x as f32, reply.y as f32, reply.z as f32];
                ensure!(
                    vector.iter().all(|value| value.is_finite()),
                    "Script angular velocity must be finite"
                );
                Ok(Some(ScriptCommand::SetAngularVelocity3D(vector)))
            }
            Kind::ReleaseAsset => Ok(Some(ScriptCommand::ReleaseAsset(reply.detail))),
            Kind::ShowNode if reply.x == 0.0 || reply.x == 1.0 => Ok(Some(
                ScriptCommand::ShowNode(reply.detail.try_into()?, reply.x == 1.0),
            )),
            Kind::SelectFrame
                if (0.0..=f64::from(u16::MAX)).contains(&reply.x) && reply.x.fract() == 0.0 =>
            {
                Ok(Some(ScriptCommand::SelectFrame(
                    reply.detail.try_into()?,
                    reply.x as usize,
                )))
            }
            Kind::QueueSound => Ok(Some(ScriptCommand::QueueSound(reply.detail))),
            Kind::RecordScore => Ok(Some(ScriptCommand::RecordScore(reply.detail.try_into()?))),
            Kind::QueueCreateNode => Ok(Some(ScriptCommand::CreateNode(
                reply.detail.try_into()?,
                optional_index(reply.x)?,
            ))),
            Kind::QueueInstantiatePrefab => Ok(Some(ScriptCommand::InstantiatePrefab(
                reply.detail.try_into()?,
                optional_index(reply.x)?,
                index(reply.y)?,
            ))),
            Kind::QueueReparentNode => Ok(Some(ScriptCommand::ReparentNode(
                reply.detail.try_into()?,
                optional_index(reply.x)?,
            ))),
            Kind::QueueDestroyNode => {
                Ok(Some(ScriptCommand::DestroyNode(reply.detail.try_into()?)))
            }
            Kind::QueueAddComponent => Ok(Some(ScriptCommand::AddComponent(
                reply.detail.try_into()?,
                index(reply.x)?,
            ))),
            Kind::QueueRemoveComponent => Ok(Some(ScriptCommand::RemoveComponent(
                reply.detail.try_into()?,
                index(reply.x)?,
            ))),
            _ => anyhow::bail!("Unexpected script reply kind {:?}", reply.kind),
        }
    }

    fn packed_text_value(value: f64, maximum: u64) -> Result<u64> {
        ensure!(
            value.is_finite() && (0.0..=maximum as f64).contains(&value) && value.fract() == 0.0,
            "Invalid packed script text"
        );
        Ok(value as u64)
    }

    fn decode_chunk_bytes(length: u32, x: f64, y: f64) -> Result<([u8; 12], u8)> {
        ensure!((1..=12).contains(&length), "Invalid script chunk length");
        let x = packed_text_value(x, (1_u64 << 48) - 1)?;
        let y = packed_text_value(y, (1_u64 << 48) - 1)?;
        let mut bytes = [0; 12];
        for (index, byte) in bytes[..6].iter_mut().enumerate() {
            *byte = (x >> (index * 8)) as u8;
        }
        for (index, byte) in bytes[6..].iter_mut().enumerate() {
            *byte = (y >> (index * 8)) as u8;
        }
        Ok((bytes, length as u8))
    }

    fn decode_text_chunk(target: u32, length: u32, x: f64, y: f64) -> Result<ScriptCommand> {
        let (bytes, length) = decode_chunk_bytes(length, x, y)?;
        Ok(ScriptCommand::SetTextChunk(target, bytes, length))
    }

    fn index(value: f64) -> Result<u32> {
        ensure!(
            (0.0..=f64::from(u32::MAX)).contains(&value) && value.fract() == 0.0,
            "Invalid script index"
        );
        Ok(value as u32)
    }

    fn optional_index(value: f64) -> Result<Option<u32>> {
        if value == -1.0 {
            Ok(None)
        } else {
            index(value).map(Some)
        }
    }
}

/// Configures the game logic process launched by the player.
pub enum Script {
    /// Run a scriptc executable with the native shared-memory transport.
    Native(PathBuf),
    /// Run a Bun or Node launcher with the development socket transport.
    Development(PathBuf),
}

#[cfg(not(any(unix, windows)))]
pub(crate) struct Session;

#[cfg(not(any(unix, windows)))]
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
