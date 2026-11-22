//! Linux descriptor passing and supervision for the frame-locked child.

use std::{
    io,
    os::{
        fd::{AsFd, AsRawFd, FromRawFd, OwnedFd},
        unix::process::CommandExt,
    },
    process::{Child, Command, Stdio},
};

use anyhow::{Context, Result, ensure};
use nix::{
    fcntl::{FcntlArg, fcntl},
    libc,
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use script_api::{
    Envelope, Kind, VERSION,
    transport::{Link, Mode},
};

use super::{Command as ScriptCommand, Script};

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

    pub(crate) fn exchange(
        &mut self,
        kind: Kind,
        node: u64,
        [x, y]: [f32; 2],
        detail: u64,
    ) -> Result<Option<ScriptCommand>> {
        let reply = self
            .link
            .exchange(Envelope {
                protocol: VERSION,
                seq: self.seq,
                kind,
                node,
                x: x.into(),
                y: y.into(),
                detail,
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
        self.seq = self
            .seq
            .checked_add(1)
            .context("Script sequence exhausted")?;
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
            _ => anyhow::bail!("Unexpected script reply kind {:?}", reply.kind),
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Reap after signalling so the process-group ID cannot have been reused.
        let _ = killpg(Pid::from_raw(self.child.id() as i32), Signal::SIGKILL);
        let _ = self.child.wait();
    }
}
