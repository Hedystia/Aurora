//! Linux descriptor passing and supervision for the frame-locked child.

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
        let command: Result<Option<ScriptCommand>> = match reply.kind {
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
        };
        Ok(Reply {
            command: command?,
            enabled: reply.binding & BINDING_ENABLED != 0,
        })
    }
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
