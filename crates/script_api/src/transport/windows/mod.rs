//! Windows named-pipe framing for the fixed-width script envelope.
//!
//! Each transaction writes one full 60-byte frame and reads one full 60-byte
//! frame. The server name is unique per session and is passed to the script
//! child, so no handle inheritance and no shared memory are required.

use std::{
    ffi::{OsStr, OsString},
    mem::size_of,
    os::windows::ffi::OsStrExt,
    ptr,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use anyhow::{Result, bail, ensure};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, FALSE, GENERIC_READ, GENERIC_WRITE,
        GetLastError, HANDLE, INVALID_HANDLE_VALUE, TRUE, WAIT_OBJECT_0,
    },
    Security::{
        ACL, ACL_REVISION, AddAccessAllowedAce, GetLengthSid, GetTokenInformation, InitializeAcl,
        InitializeSecurityDescriptor, SECURITY_ATTRIBUTES, SECURITY_DESCRIPTOR,
        SetSecurityDescriptorDacl, TOKEN_QUERY, TOKEN_USER, TokenUser,
    },
    Storage::FileSystem::{FILE_FLAG_OVERLAPPED, ReadFile, WriteFile},
    System::{
        IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
        Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, PIPE_ACCESS_DUPLEX, PIPE_READMODE_BYTE,
            PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
        },
        Threading::{CreateEventW, GetCurrentProcess, OpenProcessToken, WaitForSingleObject},
    },
};

use crate::{ENVELOPE_BYTES, Envelope};

const DEADLINE: Duration = Duration::from_millis(250);
const CONNECT_DEADLINE: Duration = Duration::from_secs(10);
const PIPE_BUFFER: u32 = 4096;
static NEXT_PIPE: AtomicU64 = AtomicU64::new(0);

/// Player endpoint with exactly one in-flight request.
pub struct Link {
    handle: HANDLE,
    limit: Option<Instant>,
}

impl Link {
    /// Creates a uniquely named pipe server and returns its name for the child.
    pub fn pair() -> Result<(Self, OsString)> {
        let name = OsString::from(format!(
            r"\\.\pipe\aurora-script-{}-{}",
            std::process::id(),
            NEXT_PIPE.fetch_add(1, Ordering::Relaxed)
        ));
        let wide: Vec<u16> = name.encode_wide().chain(std::iter::once(0u16)).collect();
        let mut security = PipeSecurity::new()?;
        // SAFETY: `wide` is NUL-terminated and `security` outlives this call.
        let handle = unsafe {
            CreateNamedPipeW(
                wide.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                PIPE_BUFFER,
                PIPE_BUFFER,
                0,
                &security.attributes(),
            )
        };
        ensure!(valid(handle), "Creating script pipe failed");
        Ok((
            Self {
                handle,
                limit: None,
            },
            name,
        ))
    }

    /// Waits for the script child to open the named pipe.
    pub fn connect(&mut self) -> Result<()> {
        let mut operation = Operation::new()?;
        // SAFETY: `handle` is a server pipe opened for overlapped I/O and the
        // overlapped record owns a live event for the whole operation.
        let connected =
            unsafe { ConnectNamedPipe(self.handle, &mut operation.overlapped) } != FALSE;
        if !connected {
            match GetLastError() {
                ERROR_IO_PENDING => {
                    operation.wait(self.handle, Instant::now() + CONNECT_DEADLINE)?;
                }
                ERROR_PIPE_CONNECTED => {}
                error => bail!("Connecting script pipe failed: {error}"),
            }
        }
        Ok(())
    }

    /// Applies a cumulative deadline to the remaining exchanges during graceful shutdown.
    pub fn limit_until(&mut self, deadline: Instant) {
        self.limit = Some(deadline);
    }

    /// Publishes a request and reads one reply within a bounded timeout.
    pub fn exchange(&mut self, message: Envelope) -> Result<Envelope> {
        let now = Instant::now();
        let deadline = self.limit.unwrap_or(now + DEADLINE).min(now + DEADLINE);
        let remaining = deadline.saturating_duration_since(now);
        ensure!(!remaining.is_zero(), "Script transaction timed out");
        let bytes = message.encode()?;
        write_all(self.handle, &bytes, deadline)?;
        let mut reply = [0; ENVELOPE_BYTES];
        read_all(self.handle, &mut reply, deadline)?;
        Ok(Envelope::decode(&reply)?)
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        // SAFETY: the handle is owned by this link and closed exactly once.
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

fn valid(handle: HANDLE) -> bool {
    !handle.is_null() && handle != INVALID_HANDLE_VALUE
}

fn write_all(handle: HANDLE, bytes: &[u8], deadline: Instant) -> Result<()> {
    let mut done = 0;
    while done < bytes.len() {
        let mut operation = Operation::new()?;
        let mut written = 0;
        // SAFETY: the buffer remains alive until the operation is waited on.
        let completed = unsafe {
            WriteFile(
                handle,
                bytes[done..].as_ptr(),
                (bytes.len() - done) as u32,
                &mut written,
                &mut operation.overlapped,
            )
        } != FALSE;
        let transferred = if completed {
            written as usize
        } else {
            match GetLastError() {
                ERROR_IO_PENDING => operation.wait(handle, deadline)?,
                error => bail!("Writing script frame failed: {error}"),
            }
        };
        ensure!(transferred > 0, "Script pipe closed");
        done += transferred;
    }
    Ok(())
}

fn read_all(handle: HANDLE, bytes: &mut [u8], deadline: Instant) -> Result<()> {
    let mut done = 0;
    while done < bytes.len() {
        let mut operation = Operation::new()?;
        let mut read = 0;
        // SAFETY: the buffer remains alive until the operation is waited on.
        let completed = unsafe {
            ReadFile(
                handle,
                bytes[done..].as_mut_ptr(),
                (bytes.len() - done) as u32,
                &mut read,
                &mut operation.overlapped,
            )
        } != FALSE;
        let transferred = if completed {
            read as usize
        } else {
            match GetLastError() {
                ERROR_IO_PENDING => operation.wait(handle, deadline)?,
                error => bail!("Reading script frame failed: {error}"),
            }
        };
        ensure!(transferred > 0, "Script disconnected");
        done += transferred;
    }
    Ok(())
}

/// One overlapped pipe operation with its own completion event.
struct Operation {
    overlapped: OVERLAPPED,
    event: HANDLE,
}

impl Operation {
    fn new() -> Result<Self> {
        // SAFETY: manual-reset, initially unsignalled, unnamed event.
        let event = unsafe { CreateEventW(ptr::null(), TRUE, FALSE, ptr::null()) };
        ensure!(valid(event), "Creating script pipe event failed");
        Ok(Self {
            overlapped: OVERLAPPED {
                hEvent: event,
                ..Default::default()
            },
            event,
        })
    }

    fn wait(&mut self, handle: HANDLE, deadline: Instant) -> Result<usize> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        ensure!(!remaining.is_zero(), "Script transaction timed out");
        let millis = remaining.as_millis().clamp(1, u32::MAX as u128) as u32;
        // SAFETY: the event is valid until this operation is dropped.
        let status = unsafe { WaitForSingleObject(self.event, millis) };
        if status != WAIT_OBJECT_0 {
            // SAFETY: cancel the still-pending operation before releasing its event.
            unsafe {
                CancelIoEx(handle, &self.overlapped);
            }
            bail!("Script transaction timed out");
        }
        let mut transferred = 0;
        // SAFETY: the signalled event records a completed overlapped operation.
        let ok = unsafe { GetOverlappedResult(handle, &self.overlapped, &mut transferred, FALSE) }
            != FALSE;
        ensure!(ok, "Script pipe operation failed");
        Ok(transferred as usize)
    }
}

impl Drop for Operation {
    fn drop(&mut self) {
        // SAFETY: the event is owned by this operation.
        unsafe {
            CloseHandle(self.event);
        }
    }
}

/// Security descriptor granting pipe access only to the current user's SID.
struct PipeSecurity {
    token: Vec<u64>,
    acl: Vec<u64>,
    descriptor: SECURITY_DESCRIPTOR,
}

impl PipeSecurity {
    fn new() -> Result<Self> {
        let mut token_handle = std::ptr::null_mut();
        // SAFETY: the pseudo-handle does not need to be closed.
        let opened =
            unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token_handle) };
        ensure!(opened != FALSE, "Opening script access token failed");
        let mut length = 0;
        unsafe { GetTokenInformation(token_handle, TokenUser, ptr::null_mut(), 0, &mut length) };
        ensure!(length > 0, "Reading script access token failed");
        let mut token = vec![0u64; length.div_ceil(8) as usize];
        // SAFETY: the buffer is aligned and sized for the reported token user.
        let read = unsafe {
            GetTokenInformation(
                token_handle,
                TokenUser,
                token.as_mut_ptr().cast(),
                length,
                &mut length,
            )
        };
        // SAFETY: the opened token handle is owned by this call.
        unsafe {
            CloseHandle(token_handle);
        }
        ensure!(read != FALSE, "Reading script access token failed");
        let user = unsafe { ptr::read_unaligned(token.as_ptr().cast::<TOKEN_USER>()) };
        let sid = user.User.Sid;
        let sid_length = unsafe { GetLengthSid(sid) };
        ensure!(sid_length > 0, "Invalid script access token SID");

        let acl_length = (size_of::<ACL>() + sid_length as usize + 15) & !15;
        let mut acl = vec![0u64; acl_length.div_ceil(8)];
        // SAFETY: the buffer is aligned and large enough for one SID ACE.
        let acl_ptr = acl.as_mut_ptr().cast::<ACL>();
        let initialized = unsafe { InitializeAcl(acl_ptr, acl_length as u32, ACL_REVISION) };
        ensure!(initialized != FALSE, "Initialising script pipe ACL failed");
        let allowed = unsafe {
            AddAccessAllowedAce(acl_ptr, ACL_REVISION, GENERIC_READ | GENERIC_WRITE, sid)
        };
        ensure!(allowed != FALSE, "Populating script pipe ACL failed");

        let mut descriptor = SECURITY_DESCRIPTOR::default();
        // SAFETY: the descriptor is aligned and initialised by the API before use.
        let descriptor_ptr = ptr::from_mut(&mut descriptor).cast();
        let initialized = unsafe { InitializeSecurityDescriptor(descriptor_ptr, 1) };
        ensure!(
            initialized != FALSE,
            "Initialising script pipe security failed"
        );
        let assigned = unsafe { SetSecurityDescriptorDacl(descriptor_ptr, TRUE, acl_ptr, FALSE) };
        ensure!(assigned != FALSE, "Securing script pipe failed");
        Ok(Self {
            token,
            acl,
            descriptor,
        })
    }

    fn attributes(&mut self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: ptr::from_mut(&mut self.descriptor).cast(),
            bInheritHandle: FALSE,
        }
    }
}
