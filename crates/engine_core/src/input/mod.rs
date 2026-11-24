//! Discrete input events delivered at simulation boundaries.

use winit::{
    event::{ElementState, TouchPhase, WindowEvent},
    keyboard::{KeyCode, PhysicalKey},
};

// The same table drives window input and the editor's single-byte input channel.
macro_rules! keys {
    ($($key:ident = $value:literal, [$($physical:ident),+], $description:literal;)+) => {
        /// Physical keyboard keys supported by Aurora's script and preview input.
        #[derive(Clone, Copy, Debug)]
        #[repr(u8)]
        pub enum Key {
            $(#[doc = $description] $key = $value,)+
        }

        impl Key {
            fn from_physical(code: KeyCode) -> Option<Self> {
                match code {
                    $($(KeyCode::$physical)|+ => Some(Self::$key),)+
                    _ => None,
                }
            }

            /// Encodes a key for the editor preview channel.
            pub fn preview_byte(self) -> u8 {
                self as u8 | 0x80
            }

            /// Decodes an editor preview key, with room for the channel's control bytes.
            pub fn from_preview_byte(byte: u8) -> Option<Self> {
                match byte {
                    $(code if code == Self::$key.preview_byte() => Some(Self::$key),)+
                    _ => None,
                }
            }
        }
    };
}

keys! {
    Space = 1, [Space], "Space bar.";
    ArrowUp = 2, [ArrowUp], "Up arrow.";
    Backspace = 8, [Backspace], "Backspace.";
    Tab = 9, [Tab], "Tab.";
    Enter = 13, [Enter], "Enter.";
    Shift = 16, [ShiftLeft, ShiftRight], "Shift.";
    Control = 17, [ControlLeft, ControlRight], "Control.";
    Alt = 18, [AltLeft, AltRight], "Alt.";
    Escape = 27, [Escape], "Escape.";
    PageUp = 33, [PageUp], "Page up.";
    PageDown = 34, [PageDown], "Page down.";
    End = 35, [End], "End.";
    Home = 36, [Home], "Home.";
    ArrowLeft = 37, [ArrowLeft], "Left arrow.";
    ArrowRight = 39, [ArrowRight], "Right arrow.";
    ArrowDown = 40, [ArrowDown], "Down arrow.";
    Insert = 45, [Insert], "Insert.";
    Delete = 46, [Delete], "Delete.";
    Digit0 = 48, [Digit0], "Digit 0.";
    Digit1 = 49, [Digit1], "Digit 1.";
    Digit2 = 50, [Digit2], "Digit 2.";
    Digit3 = 51, [Digit3], "Digit 3.";
    Digit4 = 52, [Digit4], "Digit 4.";
    Digit5 = 53, [Digit5], "Digit 5.";
    Digit6 = 54, [Digit6], "Digit 6.";
    Digit7 = 55, [Digit7], "Digit 7.";
    Digit8 = 56, [Digit8], "Digit 8.";
    Digit9 = 57, [Digit9], "Digit 9.";
    A = 65, [KeyA], "Letter A.";
    B = 66, [KeyB], "Letter B.";
    C = 67, [KeyC], "Letter C.";
    D = 68, [KeyD], "Letter D.";
    E = 69, [KeyE], "Letter E.";
    F = 70, [KeyF], "Letter F.";
    G = 71, [KeyG], "Letter G.";
    H = 72, [KeyH], "Letter H.";
    I = 73, [KeyI], "Letter I.";
    J = 74, [KeyJ], "Letter J.";
    K = 75, [KeyK], "Letter K.";
    L = 76, [KeyL], "Letter L.";
    M = 77, [KeyM], "Letter M.";
    N = 78, [KeyN], "Letter N.";
    O = 79, [KeyO], "Letter O.";
    P = 80, [KeyP], "Letter P.";
    Q = 81, [KeyQ], "Letter Q.";
    R = 82, [KeyR], "Letter R.";
    S = 83, [KeyS], "Letter S.";
    T = 84, [KeyT], "Letter T.";
    U = 85, [KeyU], "Letter U.";
    V = 86, [KeyV], "Letter V.";
    W = 87, [KeyW], "Letter W.";
    X = 88, [KeyX], "Letter X.";
    Y = 89, [KeyY], "Letter Y.";
    Z = 90, [KeyZ], "Letter Z.";
    F1 = 112, [F1], "Function key F1.";
    F2 = 113, [F2], "Function key F2.";
    F3 = 114, [F3], "Function key F3.";
    F4 = 115, [F4], "Function key F4.";
    F5 = 116, [F5], "Function key F5.";
    F6 = 117, [F6], "Function key F6.";
    F7 = 118, [F7], "Function key F7.";
    F8 = 119, [F8], "Function key F8.";
    F9 = 120, [F9], "Function key F9.";
    F10 = 121, [F10], "Function key F10.";
    F11 = 122, [F11], "Function key F11.";
    F12 = 123, [F12], "Function key F12.";
    Semicolon = 186, [Semicolon], "Semicolon.";
    Equal = 187, [Equal], "Equal sign.";
    Comma = 188, [Comma], "Comma.";
    Minus = 189, [Minus], "Minus sign.";
    Period = 190, [Period], "Period.";
    Slash = 191, [Slash], "Slash.";
    Backquote = 192, [Backquote], "Backquote.";
    BracketLeft = 219, [BracketLeft], "Left bracket.";
    Backslash = 220, [Backslash], "Backslash.";
    BracketRight = 221, [BracketRight], "Right bracket.";
    Quote = 222, [Quote], "Single quote.";
}

/// Pointer identity used for mouse input. Touch identities start above it.
pub const MOUSE_POINTER: u64 = 0;

/// Player input independent of the window backend.
#[derive(Clone, Copy, Debug)]
pub enum InputEvent {
    /// Initial physical key press, excluding auto-repeat.
    KeyDown(Key),
    /// Physical key release.
    KeyUp(Key),
    /// Initial finger contact without a position, kept for legacy headless input.
    TouchStart,
    /// Pointer press in normalized screen coordinates and physical viewport dimensions.
    PointerStart {
        /// Horizontal and vertical coordinates in the range 0 through 1, measured from the top left.
        position: [f32; 2],
        /// Physical dimensions used for camera fitting.
        viewport: [u32; 2],
        /// Stable pointer identity within the contact.
        id: u64,
    },
    /// Pointer movement while the contact is active.
    PointerMove {
        /// Horizontal and vertical coordinates in the range 0 through 1, measured from the top left.
        position: [f32; 2],
        /// Physical dimensions used for camera fitting.
        viewport: [u32; 2],
        /// Stable pointer identity within the contact.
        id: u64,
    },
    /// Pointer release at the given position.
    PointerEnd {
        /// Horizontal and vertical coordinates in the range 0 through 1, measured from the top left.
        position: [f32; 2],
        /// Physical dimensions used for camera fitting.
        viewport: [u32; 2],
        /// Stable pointer identity within the contact.
        id: u64,
    },
    /// Pointer contact lost without a final position, such as focus loss.
    PointerCancel {
        /// Stable pointer identity within the contact.
        id: u64,
    },
    /// Every active contact lost at once, such as when the window loses focus.
    CancelPointers,
    /// Mouse wheel movement at the cursor, with horizontal and vertical deltas.
    Wheel {
        /// Horizontal and vertical coordinates in the range 0 through 1, measured from the top left.
        position: [f32; 2],
        /// Horizontal and vertical scroll deltas in window units.
        delta: [f32; 2],
        /// Physical dimensions used for camera fitting.
        viewport: [u32; 2],
    },
}

impl InputEvent {
    /// Builds a pointer event for mouse input in normalized coordinates.
    pub fn mouse(phase: PointerPhase, position: [f32; 2], viewport: [u32; 2]) -> Option<Self> {
        let position = [position[0].clamp(0.0, 1.0), position[1].clamp(0.0, 1.0)];
        Some(match phase {
            PointerPhase::Start => Self::PointerStart {
                position,
                viewport,
                id: MOUSE_POINTER,
            },
            PointerPhase::Move => Self::PointerMove {
                position,
                viewport,
                id: MOUSE_POINTER,
            },
            PointerPhase::End => Self::PointerEnd {
                position,
                viewport,
                id: MOUSE_POINTER,
            },
            PointerPhase::Cancel => Self::PointerCancel { id: MOUSE_POINTER },
        })
    }
}

/// Pointer movement phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerPhase {
    /// Contact began.
    Start,
    /// Contact moved.
    Move,
    /// Contact ended.
    End,
    /// Contact was cancelled.
    Cancel,
}

impl InputEvent {
    /// Maps supported winit events to input, retaining coordinate-free touch compatibility.
    pub fn from_window(event: &WindowEvent) -> Option<Self> {
        if matches!(event, WindowEvent::Touch(touch) if touch.phase == TouchPhase::Started) {
            return Some(Self::TouchStart);
        }
        Self::from_window_in_view(event, [0, 0])
    }

    /// Maps winit events using physical viewport dimensions for touch coordinates.
    ///
    /// Keyboard releases map to [`InputEvent::KeyUp`]. Touch contacts carry winit's
    /// stable identifier offset past [`MOUSE_POINTER`] so multiple fingers remain
    /// distinct. Mouse movement is supplied by the caller because it depends on
    /// whether a button is held.
    pub fn from_window_in_view(event: &WindowEvent, viewport: [u32; 2]) -> Option<Self> {
        match event {
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return None;
                };
                let key = Key::from_physical(code)?;
                match event.state {
                    ElementState::Pressed if !event.repeat => Some(Self::KeyDown(key)),
                    ElementState::Released => Some(Self::KeyUp(key)),
                    _ => None,
                }
            }
            WindowEvent::Touch(touch) => {
                let id = touch.id.checked_add(1)?;
                if touch.phase == TouchPhase::Cancelled {
                    return Some(Self::PointerCancel { id });
                }
                if viewport[0] == 0 || viewport[1] == 0 {
                    return None;
                }
                let x = (touch.location.x / f64::from(viewport[0])) as f32;
                let y = (touch.location.y / f64::from(viewport[1])) as f32;
                if !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) {
                    return None;
                }
                let position = [x, y];
                match touch.phase {
                    TouchPhase::Started => Some(Self::PointerStart {
                        position,
                        viewport,
                        id,
                    }),
                    TouchPhase::Moved => Some(Self::PointerMove {
                        position,
                        viewport,
                        id,
                    }),
                    TouchPhase::Ended => Some(Self::PointerEnd {
                        position,
                        viewport,
                        id,
                    }),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}
