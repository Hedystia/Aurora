//! Offscreen editor preview using the player's sprite pipeline.

use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};

use anyhow::Result;

use crate::{
    input::{InputEvent, Key},
    render::Offscreen,
    scripting::Script,
};

use super::{Scenario, clock::Clock, demo::CAMERA, runtime::Runtime};

/// Raw RGBA preview frame size shared by the editor and player.
pub const SIZE: [u32; 2] = [640, 384];

/// Streams offscreen frames to a private directory until the input pipe closes.
pub fn run_preview(
    dir: &Path,
    cache: Option<PathBuf>,
    script: Option<&Script>,
    scenario: Scenario<'_>,
) -> Result<()> {
    let mut runtime = Runtime::new(None, cache, &scenario, false)?;
    runtime.attach_script(script)?;
    let mut gpu = pollster::block_on(Offscreen::new(SIZE))?;
    let (send, receive) = mpsc::sync_channel(64);
    std::thread::spawn(move || {
        let mut input = std::io::stdin().lock();
        let mut command = [0u8; 1];
        while input.read_exact(&mut command).is_ok() && send.send(command[0]).is_ok() {}
    });
    let mut clock = Clock::default();
    let mut last = Instant::now();
    let mut next_frame = last;
    let mut paused = false;
    let mut input = Vec::new();
    loop {
        match receive.recv_timeout(Duration::from_millis(8)) {
            Ok(b'p') => paused = !paused,
            Ok(b' ') if input.len() < 64 => input.push(InputEvent::KeyDown(Key::Space)),
            Ok(b't') if input.len() < 64 => input.push(InputEvent::TouchStart),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            _ => {}
        }
        let now = Instant::now();
        let ticks = clock.advance(now - last);
        last = now;
        if !paused {
            for _ in 0..ticks {
                if let Err(error) = runtime.tick(&input) {
                    eprintln!("Game script stopped: {error:#}");
                    runtime.disconnect_script();
                }
                input.clear();
            }
        } else {
            input.clear();
        }
        if now >= next_frame {
            let pixels = gpu.frame(runtime.assets(), &runtime.scene().snapshot()?, CAMERA)?;
            let mut output = tempfile::NamedTempFile::new_in(dir)?;
            output.write_all(&pixels)?;
            output.persist(dir.join("frame.rgba"))?;
            next_frame = now + Duration::from_millis(50);
        }
    }
}
