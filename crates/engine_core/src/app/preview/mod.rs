//! Offscreen editor preview using the player's sprite pipeline.

use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};

/// Reads one pointer position command from the editor's input channel.
fn read_pointer(
    input: &mut impl Read,
    phase: PointerPhase,
    viewport: [u32; 2],
) -> Option<InputEvent> {
    let mut coordinates = [0_u8; 4];
    input.read_exact(&mut coordinates).ok()?;
    let x = u16::from_le_bytes([coordinates[0], coordinates[1]]);
    let y = u16::from_le_bytes([coordinates[2], coordinates[3]]);
    let position = [
        f32::from(x) / f32::from(u16::MAX),
        f32::from(y) / f32::from(u16::MAX),
    ];
    let id = crate::input::MOUSE_POINTER;
    Some(match phase {
        PointerPhase::Start => InputEvent::PointerStart {
            position,
            viewport,
            id,
        },
        PointerPhase::Move => InputEvent::PointerMove {
            position,
            viewport,
            id,
        },
        _ => InputEvent::PointerEnd {
            position,
            viewport,
            id,
        },
    })
}

/// Reads one wheel command: two u16 coordinates and two i16 deltas.
fn read_touch(
    input: &mut impl Read,
    phase: PointerPhase,
    viewport: [u32; 2],
) -> Option<InputEvent> {
    let mut payload = [0_u8; 6];
    input.read_exact(&mut payload).ok()?;
    let x = u16::from_le_bytes([payload[0], payload[1]]);
    let y = u16::from_le_bytes([payload[2], payload[3]]);
    // Preview touch ids start at one so they never collide with the mouse.
    let id = u64::from(u16::from_le_bytes([payload[4], payload[5]])) + 1;
    let position = [
        f32::from(x) / f32::from(u16::MAX),
        f32::from(y) / f32::from(u16::MAX),
    ];
    Some(match phase {
        PointerPhase::Start => InputEvent::PointerStart {
            position,
            viewport,
            id,
        },
        PointerPhase::Move => InputEvent::PointerMove {
            position,
            viewport,
            id,
        },
        PointerPhase::End => InputEvent::PointerEnd {
            position,
            viewport,
            id,
        },
        PointerPhase::Cancel => InputEvent::PointerCancel { id },
    })
}

fn read_touch_cancel(input: &mut impl Read) -> Option<InputEvent> {
    let mut id = [0_u8; 2];
    input.read_exact(&mut id).ok()?;
    Some(InputEvent::PointerCancel {
        id: u64::from(u16::from_le_bytes(id)) + 1,
    })
}

fn read_wheel(input: &mut impl Read, viewport: [u32; 2]) -> Option<InputEvent> {
    let mut payload = [0_u8; 8];
    input.read_exact(&mut payload).ok()?;
    let x = u16::from_le_bytes([payload[0], payload[1]]);
    let y = u16::from_le_bytes([payload[2], payload[3]]);
    let dx = i16::from_le_bytes([payload[4], payload[5]]);
    let dy = i16::from_le_bytes([payload[6], payload[7]]);
    Some(InputEvent::Wheel {
        position: [
            f32::from(x) / f32::from(u16::MAX),
            f32::from(y) / f32::from(u16::MAX),
        ],
        delta: [f32::from(dx), f32::from(dy)],
        viewport,
    })
}

use anyhow::Result;

use crate::{
    input::{InputEvent, Key, PointerPhase},
    render::Offscreen,
    scripting::Script,
};

use super::{Scenario, clock::Clock, runtime::Runtime};

/// Streams offscreen frames to a private directory until the input pipe closes.
pub fn run_preview(
    dir: &Path,
    cache: Option<PathBuf>,
    script: Option<&Script>,
    scenario: Scenario<'_>,
) -> Result<()> {
    let mut runtime = Runtime::new(None, cache, &scenario, false)?;
    runtime.attach_script(script)?;
    let mut gpu = pollster::block_on(Offscreen::new(runtime.preview_size()))?;
    let (send, receive) = mpsc::sync_channel(64);
    let viewport = runtime.preview_size();
    std::thread::spawn(move || {
        let mut input = std::io::stdin().lock();
        let mut command = [0u8; 1];
        // Pointer commands carry two little-endian u16 coordinates. The key-up
        // command carries the key byte that follows it.
        while input.read_exact(&mut command).is_ok() {
            let byte = command[0];
            let message = match byte {
                b'm' => read_pointer(&mut input, PointerPhase::Start, viewport),
                b'n' => read_pointer(&mut input, PointerPhase::Move, viewport),
                b'o' => read_pointer(&mut input, PointerPhase::End, viewport),
                b'c' => Some(InputEvent::PointerCancel {
                    id: crate::input::MOUSE_POINTER,
                }),
                b'M' => read_touch(&mut input, PointerPhase::Start, viewport),
                b'N' => read_touch(&mut input, PointerPhase::Move, viewport),
                b'O' => read_touch(&mut input, PointerPhase::End, viewport),
                b'C' => read_touch_cancel(&mut input),
                b'F' => Some(InputEvent::CancelPointers),
                b'w' => read_wheel(&mut input, viewport),
                0x02 => {
                    let mut key = [0_u8; 1];
                    if input.read_exact(&mut key).is_err() {
                        break;
                    }
                    Key::from_preview_byte(key[0]).map(InputEvent::KeyUp)
                }
                _ => None,
            };
            if send.send((byte, message)).is_err() {
                break;
            }
        }
    });
    let mut clock = Clock::default();
    let mut last = Instant::now();
    let mut next_frame = last;
    let mut paused = false;
    let mut input = Vec::new();
    loop {
        match receive.recv_timeout(Duration::from_millis(8)) {
            Ok((b'p', _)) => paused = !paused,
            Ok((b'w', None)) => {}
            Ok((b' ', _)) if input.len() < 64 => input.push(InputEvent::KeyDown(Key::Space)),
            Ok((b'u', _)) if input.len() < 64 => input.push(InputEvent::KeyDown(Key::ArrowUp)),
            Ok((b't', _)) if input.len() < 64 => input.push(InputEvent::TouchStart),
            Ok((_, Some(event))) if input.len() < 64 => input.push(event),
            Ok((byte, None)) if input.len() < 64 => {
                if byte != 0x02
                    && let Some(key) = Key::from_preview_byte(byte)
                {
                    input.push(InputEvent::KeyDown(key));
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return runtime.stop_script(),
            _ => {}
        }
        let now = Instant::now();
        let ticks = clock.advance(now - last);
        last = now;
        if !paused {
            for _ in 0..ticks {
                if let Err(error) = runtime.tick(&input) {
                    if runtime.spatial_state().is_some() {
                        return Err(error);
                    }
                    eprintln!("Game script stopped: {error:#}");
                    runtime.disconnect_script();
                }
                input.clear();
            }
        } else {
            input.clear();
        }
        if now >= next_frame {
            let pixels = gpu.frame(runtime.frame()?)?;
            let mut output = tempfile::NamedTempFile::new_in(dir)?;
            output.write_all(&pixels)?;
            output.persist(dir.join("frame.rgba"))?;
            next_frame = now + Duration::from_millis(50);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn touch_messages_keep_stable_ids_and_positions() {
        let payload = [0x00, 0x80, 0x00, 0x40, 0x03, 0x00];
        let Some(event) = read_touch(&mut Cursor::new(payload), PointerPhase::Start, [640, 360])
        else {
            panic!("touch start should decode");
        };
        match event {
            InputEvent::PointerStart {
                position,
                viewport,
                id,
            } => {
                assert!((position[0] - 0.500_007_6).abs() < 1e-6);
                assert!((position[1] - 0.250_003_8).abs() < 1e-6);
                assert_eq!(viewport, [640, 360]);
                assert_eq!(id, 4);
            }
            _ => panic!("expected a pointer start"),
        }
    }

    #[test]
    fn touch_cancel_matches_the_started_id() {
        let Some(event) = read_touch_cancel(&mut Cursor::new([0x07, 0x00])) else {
            panic!("touch cancel should decode");
        };
        assert!(matches!(event, InputEvent::PointerCancel { id: 8 }));
    }
}
