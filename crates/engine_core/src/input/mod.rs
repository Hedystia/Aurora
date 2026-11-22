//! Discrete input events delivered at simulation boundaries.

use winit::{
    event::{ElementState, TouchPhase, WindowEvent},
    keyboard::{KeyCode, PhysicalKey},
};

/// Supported physical keys for the M3 game controls.
#[derive(Clone, Copy, Debug)]
pub enum Key {
    /// Space bar.
    Space = 1,
    /// Up arrow.
    ArrowUp = 2,
}

/// Player input independent of the window backend.
#[derive(Clone, Copy, Debug)]
pub enum InputEvent {
    /// Initial physical key press, excluding auto-repeat.
    KeyDown(Key),
    /// Initial finger contact.
    TouchStart,
}

impl InputEvent {
    /// Maps supported winit window events to discrete simulation input.
    pub fn from_window(event: &WindowEvent) -> Option<Self> {
        match event {
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } if event.state == ElementState::Pressed && !event.repeat => {
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::Space) => Some(Self::KeyDown(Key::Space)),
                    PhysicalKey::Code(KeyCode::ArrowUp) => Some(Self::KeyDown(Key::ArrowUp)),
                    _ => None,
                }
            }
            WindowEvent::Touch(touch) if touch.phase == TouchPhase::Started => {
                Some(Self::TouchStart)
            }
            _ => None,
        }
    }
}
