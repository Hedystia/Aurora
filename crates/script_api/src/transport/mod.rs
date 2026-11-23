//! Unix socket framing and Linux memfd SPSC rings.

use std::{
    fs::File,
    io::{ErrorKind, Read, Write},
    os::fd::{AsFd, BorrowedFd},
    os::unix::net::UnixStream,
    sync::atomic::{AtomicU32, Ordering},
    time::{Duration, Instant},
};

use anyhow::{Result, ensure};
use memmap2::{MmapMut, MmapOptions};
use nix::fcntl::{FcntlArg, SealFlag, fcntl};
use nix::sys::memfd::{MFdFlags, memfd_create};

use crate::{ENVELOPE_BYTES, Envelope, HEADER_BYTES, PAYLOAD_BYTES};

const CAPACITY: u32 = 8;
const CURSORS: usize = 8;
const RING_BYTES: usize = CURSORS + CAPACITY as usize * PAYLOAD_BYTES;
const REGION_BYTES: usize = 2 * RING_BYTES;
const DEADLINE: Duration = Duration::from_millis(250);

/// Which channel carries payload bytes.
#[derive(Clone, Copy)]
pub enum Mode {
    /// Native client uses shared memory for payloads.
    Shared,
    /// Development client uses the same full record on the socket.
    Socket,
}

/// Player endpoint, with exactly one in-flight request.
pub struct Link {
    socket: UnixStream,
    region: Option<Region>,
    limit: Option<Instant>,
}

impl Link {
    /// Allocates the transport and returns its child socket.
    pub fn pair(mode: Mode) -> Result<(Self, UnixStream)> {
        let (socket, child) = UnixStream::pair()?;
        let region = match mode {
            Mode::Shared => Some(Region::new()?),
            Mode::Socket => None,
        };
        Ok((
            Self {
                socket,
                region,
                limit: None,
            },
            child,
        ))
    }

    /// Returns the shared-memory descriptor to be passed to the native child.
    pub fn shared_fd(&self) -> Option<BorrowedFd<'_>> {
        self.region.as_ref().map(|region| region.file.as_fd())
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
        self.socket.set_write_timeout(Some(remaining))?;
        let bytes = message.encode()?;
        let mut reply = [0; ENVELOPE_BYTES];
        if let Some(region) = &self.region {
            region.push(0, bytes[HEADER_BYTES..].try_into()?)?;
            self.socket.write_all(&bytes[..HEADER_BYTES])?;
            read_deadline(&mut self.socket, &mut reply[..HEADER_BYTES], deadline)?;
            reply[HEADER_BYTES..].copy_from_slice(&region.pop(RING_BYTES)?);
        } else {
            self.socket.write_all(&bytes)?;
            read_deadline(&mut self.socket, &mut reply, deadline)?;
        }
        Ok(Envelope::decode(&reply)?)
    }
}

fn read_deadline(socket: &mut UnixStream, mut bytes: &mut [u8], deadline: Instant) -> Result<()> {
    while !bytes.is_empty() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        ensure!(!remaining.is_zero(), "Script reply timed out");
        socket.set_read_timeout(Some(remaining))?;
        match socket.read(bytes) {
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                anyhow::bail!("Script reply timed out");
            }
            result => {
                let read = result?;
                ensure!(read > 0, "Script disconnected");
                bytes = &mut bytes[read..];
            }
        }
    }
    Ok(())
}

struct Region {
    file: File,
    // Retains the mapping. Cross-process slot writes always use raw pointers.
    _mapping: MmapMut,
    base: *mut u8,
}

impl Region {
    fn new() -> Result<Self> {
        let file = File::from(memfd_create(
            "aurora-script",
            MFdFlags::MFD_CLOEXEC | MFdFlags::MFD_ALLOW_SEALING,
        )?);
        file.set_len(REGION_BYTES as u64)?;
        fcntl(
            &file,
            FcntlArg::F_ADD_SEALS(
                SealFlag::F_SEAL_GROW | SealFlag::F_SEAL_SHRINK | SealFlag::F_SEAL_SEAL,
            ),
        )?;
        // SAFETY: The new file has the exact sealed size and is private until child launch.
        let mut mapping = unsafe { MmapOptions::new().len(REGION_BYTES).map_mut(&file)? };
        let base = mapping.as_mut_ptr();
        Ok(Self {
            file,
            _mapping: mapping,
            base,
        })
    }

    fn cursor(&self, offset: usize) -> &AtomicU32 {
        // SAFETY: mmap is page aligned, offsets select aligned u32 cursors, and all
        // participants access those locations atomically for the life of the mapping.
        unsafe { AtomicU32::from_ptr(self.base.add(offset).cast()) }
    }

    fn slot(&self, ring: usize, index: u32) -> *mut u8 {
        // SAFETY: Masking the index selects one complete slot inside the mapping.
        unsafe {
            self.base
                .add(ring + CURSORS + (index % CAPACITY) as usize * PAYLOAD_BYTES)
        }
    }

    fn push(&self, ring: usize, bytes: &[u8; PAYLOAD_BYTES]) -> Result<()> {
        let producer = self.cursor(ring);
        let write = producer.load(Ordering::Relaxed);
        let read = self.cursor(ring + 4).load(Ordering::Acquire);
        ensure!(
            write.wrapping_sub(read) < CAPACITY,
            "Script ring full or corrupt"
        );
        // SAFETY: Acquire of the consumer cursor frees this slot. The only producer
        // of this ring owns the write position and publishes it after the copy.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.slot(ring, write), PAYLOAD_BYTES)
        };
        producer.store(write.wrapping_add(1), Ordering::Release);
        Ok(())
    }

    fn pop(&self, ring: usize) -> Result<[u8; PAYLOAD_BYTES]> {
        let consumer = self.cursor(ring + 4);
        let read = consumer.load(Ordering::Relaxed);
        let write = self.cursor(ring).load(Ordering::Acquire);
        ensure!(
            (1..=CAPACITY).contains(&write.wrapping_sub(read)),
            "Script ring empty or corrupt"
        );
        let mut bytes = [0; PAYLOAD_BYTES];
        // SAFETY: The producer's release published this slot, and cannot reuse it
        // until the sole consumer releases the read cursor after copying it.
        unsafe {
            std::ptr::copy_nonoverlapping(self.slot(ring, read), bytes.as_mut_ptr(), PAYLOAD_BYTES)
        };
        consumer.store(read.wrapping_add(1), Ordering::Release);
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_saturation_and_cursor_wrap() -> Result<()> {
        let ring = Region::new()?;
        let start = u32::MAX - 2;
        ring.cursor(0).store(start, Ordering::Relaxed);
        ring.cursor(4).store(start, Ordering::Relaxed);
        assert!(ring.pop(0).is_err());
        for index in 0..CAPACITY {
            ring.push(0, &[index as u8; PAYLOAD_BYTES])?;
        }
        assert!(ring.push(0, &[0; PAYLOAD_BYTES]).is_err());
        for index in 0..CAPACITY {
            assert_eq!(ring.pop(0)?, [index as u8; PAYLOAD_BYTES]);
        }
        assert!(ring.pop(0).is_err());
        Ok(())
    }
}
