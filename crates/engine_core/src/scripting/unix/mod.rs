//! Unix descriptor passing and supervision for the frame-locked child.

use std::{
    io,
    os::{
        fd::{AsFd, AsRawFd, FromRawFd, OwnedFd},
        unix::process::CommandExt,
    },
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use nix::{
    fcntl::{FcntlArg, fcntl},
    libc,
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use script_api::{
    BINDING_ENABLED, Envelope, Kind, VERSION,
    transport::{Link, Mode},
};

use super::{Command as ScriptCommand, Reply, Script};

pub(crate) struct Session {
    child: Child,
    link: Link,
    seq: u64,
}

impl Session {
    pub(crate) fn spawn(script: &Script) -> Result<Self> {
        let (path, mode) = match script {
            Script::Native(path) => (path, Mode::Shared),
            Script::Development(path) => (path, Mode::Socket),
        };
        let path = path
            .canonicalize()
            .with_context(|| format!("Resolving script {}", path.display()))?;
        let (link, socket) = Link::pair(mode)?;
        let socket: OwnedFd = socket.into();
        let log = std::io::stderr().as_fd().try_clone_to_owned()?;
        let mut command = Command::new(&path);
        command
            .stdin(Stdio::from(socket))
            .stdout(Stdio::from(log))
            .process_group(0);

        let shared = link
            .shared_fd()
            .map(|fd| {
                let duplicate = fcntl(fd, FcntlArg::F_DUPFD_CLOEXEC(4))?;
                // SAFETY: fcntl returned a new descriptor owned by this invocation.
                Ok::<_, nix::Error>(unsafe { OwnedFd::from_raw_fd(duplicate) })
            })
            .transpose()?;
        if let Some(shared) = shared {
            // SAFETY: This function only invokes async-signal-safe dup2 before exec.
            // The source descriptor is captured to keep it alive through fork.
            unsafe {
                command.pre_exec(move || {
                    if libc::dup2(shared.as_raw_fd(), 3) == -1 {
                        return Err(io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
        let child = command
            .spawn()
            .with_context(|| format!("Launching script {}", path.display()))?;
        Ok(Self {
            child,
            link,
            seq: 0,
        })
    }

    pub(crate) fn begin_shutdown(&mut self) {
        self.link
            .limit_until(Instant::now() + Duration::from_millis(180));
    }

    pub(crate) fn exchange(
        &mut self,
        kind: Kind,
        binding: u32,
        node: u64,
        [x, y]: [f32; 2],
        detail: u64,
    ) -> Result<Reply> {
        self.exchange_control(kind, binding, node, [x.into(), y.into()], detail)
    }

    pub(crate) fn exchange_control(
        &mut self,
        kind: Kind,
        binding: u32,
        node: u64,
        [x, y]: [f64; 2],
        detail: u64,
    ) -> Result<Reply> {
        ensure!(binding < BINDING_ENABLED, "Too many script bindings");
        let reply = self
            .link
            .exchange(Envelope {
                protocol: VERSION,
                seq: self.seq,
                kind,
                node,
                x,
                y,
                detail,
                binding,
            })
            .with_context(|| format!("Script transaction {} ({kind:?})", self.seq))?;
        ensure!(
            reply.protocol == VERSION,
            "Script protocol version {} does not match {VERSION}",
            reply.protocol
        );
        ensure!(
            reply.seq == self.seq,
            "Unexpected script reply sequence {}",
            reply.seq
        );
        ensure!(
            reply.node == node,
            "Unknown script target handle {}",
            reply.node
        );
        ensure!(
            reply.binding & !BINDING_ENABLED == binding,
            "Unexpected script reply binding {}",
            reply.binding
        );
        self.seq = self
            .seq
            .checked_add(1)
            .context("Script sequence exhausted")?;
        let command = decode_command(&reply);
        Ok(Reply {
            command: command?,
            enabled: reply.binding & BINDING_ENABLED != 0,
        })
    }
}

fn decode_command(reply: &Envelope) -> Result<Option<ScriptCommand>> {
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
        Kind::PlayOneShot => Ok(Some(ScriptCommand::PlayOneShot(reply.detail.try_into()?))),
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
                length @ 1..=12 => Ok(Some(decode_text_chunk(target, length, reply.x, reply.y)?)),
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
        Kind::ShowNode if reply.x == 0.0 || reply.x == 1.0 => Ok(Some(ScriptCommand::ShowNode(
            reply.detail.try_into()?,
            reply.x == 1.0,
        ))),
        Kind::SelectFrame
            if (0.0..=f64::from(u16::MAX)).contains(&reply.x) && reply.x.fract() == 0.0 =>
        {
            Ok(Some(ScriptCommand::SelectFrame(
                reply.detail.try_into()?,
                reply.x as usize,
            )))
        }
        Kind::QueueSound => Ok(Some(ScriptCommand::QueueSound(reply.detail.try_into()?))),
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
        Kind::QueueDestroyNode => Ok(Some(ScriptCommand::DestroyNode(reply.detail.try_into()?))),
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

impl Drop for Session {
    fn drop(&mut self) {
        // Reap after signalling so the process-group ID cannot have been reused.
        let _ = killpg(Pid::from_raw(self.child.id() as i32), Signal::SIGKILL);
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack_six(bytes: &[u8]) -> f64 {
        bytes
            .iter()
            .take(6)
            .enumerate()
            .map(|(index, byte)| f64::from(*byte) * 2.0_f64.powi((index * 8) as i32))
            .sum()
    }

    #[test]
    fn decodes_config_chunks_and_loading_commands() -> Result<()> {
        let chunk = *b"Config......";
        let envelope = |kind, detail, x, y| Envelope {
            protocol: VERSION,
            seq: 0,
            kind,
            node: 0,
            x,
            y,
            detail,
            binding: 0,
        };
        assert!(matches!(
            decode_command(&envelope(Kind::SetLoading, 1, 0.0, 0.0))?,
            Some(ScriptCommand::ShowLoading(true))
        ));
        assert!(decode_command(&envelope(Kind::SetLoading, 2, 0.0, 0.0)).is_err());
        assert!(matches!(
            decode_command(&envelope(
                Kind::ConfigSet,
                (6_u64 << 32) | 3,
                pack_six(&chunk[..6]),
                pack_six(&chunk[6..]),
            ))?,
            Some(ScriptCommand::ConfigSetChunk(3, _, 6))
        ));
        Ok(())
    }

    #[test]
    fn decodes_text_chunk_length_from_detail_not_payload() -> Result<()> {
        let chunk = *b"Text 1......";
        for length in [1, 6, 12] {
            let ScriptCommand::SetTextChunk(target, actual, actual_length) =
                decode_text_chunk(42, length, pack_six(&chunk[..6]), pack_six(&chunk[6..]))?
            else {
                anyhow::bail!("Expected a text chunk");
            };
            assert_eq!(target, 42);
            assert_eq!(actual_length, length as u8);
            assert_eq!(actual, chunk);
        }
        assert!(decode_text_chunk(42, 12, 0.0, 12.0 * 2.0_f64.powi(48)).is_err());
        Ok(())
    }
}
