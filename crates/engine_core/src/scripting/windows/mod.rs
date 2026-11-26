//! Windows named-pipe child supervision for the frame-locked script.

use std::{
    mem::size_of,
    os::windows::{io::AsRawHandle, process::CommandExt},
    process::{Child, Command, Stdio},
    ptr,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use script_api::{BINDING_ENABLED, Envelope, Kind, VERSION, transport::Link};
use windows_sys::Win32::{
    Foundation::{CloseHandle, FALSE, HANDLE, INVALID_HANDLE_VALUE},
    System::{
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject, TerminateJobObject,
        },
        Threading::{CREATE_NO_WINDOW, OpenProcess, PROCESS_TERMINATE, TerminateProcess},
    },
};

use super::{Reply, Script, decode::decode_command};

pub(crate) struct Session {
    child: Child,
    job: Option<Job>,
    link: Link,
    seq: u64,
}

impl Session {
    pub(crate) fn spawn(script: &Script) -> Result<Self> {
        let path = match script {
            Script::Native(path) | Script::Development(path) => path,
        };
        let path = path
            .canonicalize()
            .with_context(|| format!("Resolving script {}", path.display()))?;
        let (mut link, name) = Link::pair()?;
        let mut command = Command::new(&path);
        command
            .env("AURORA_SCRIPT_PIPE", name)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .creation_flags(CREATE_NO_WINDOW);
        let child = command
            .spawn()
            .with_context(|| format!("Launching script {}", path.display()))?;
        let job = create_job(&child);
        link.connect()?;
        Ok(Self {
            child,
            job,
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
        self.exchange_control(kind, binding, node, [x.into(), y.into(), 0.0], detail)
    }

    pub(crate) fn exchange_control(
        &mut self,
        kind: Kind,
        binding: u32,
        node: u64,
        [x, y, z]: [f64; 3],
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
                z,
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

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(job) = &self.job {
            // SAFETY: the job handle is owned by this session and still open.
            unsafe {
                TerminateJobObject(job.0, 1);
            }
        } else {
            // SAFETY: the child id is still valid because the child is not reaped yet.
            unsafe {
                let process = OpenProcess(PROCESS_TERMINATE, FALSE, self.child.id());
                if valid(process) {
                    TerminateProcess(process, 1);
                    CloseHandle(process);
                }
            }
        }
        let _ = self.child.wait();
    }
}

/// Owns a kill-on-close job so the whole script process tree is terminated.
struct Job(HANDLE);

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: the handle is owned by this wrapper and closed exactly once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn create_job(child: &Child) -> Option<Job> {
    // SAFETY: unnamed job object with default security.
    let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
    if !valid(handle) {
        return None;
    }
    let job = Job(handle);
    let mut information = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    information.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: the information block is correctly typed and sized for the API.
    let configured = unsafe {
        SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            ptr::from_ref(&information).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    // SAFETY: the child process handle remains owned by `Child` for this call.
    let assigned = unsafe { AssignProcessToJobObject(job.0, child.as_raw_handle()) };
    if configured == FALSE || assigned == FALSE {
        return None;
    }
    Some(job)
}

fn valid(handle: HANDLE) -> bool {
    !handle.is_null() && handle != INVALID_HANDLE_VALUE
}
