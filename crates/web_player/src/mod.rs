#![cfg(target_arch = "wasm32")]

//! Browser player groundwork: a wasm-bindgen shell that owns a WebGPU canvas
//! surface and drives fixed simulation ticks from `requestAnimationFrame`.
//!
//! This is deliberately minimal. Browsers cannot spawn the supervised native
//! script child the desktop player uses, so the runtime here is scriptless: it
//! advances a fixed-step clock, records forwarded input and clears the canvas.
//! There is no scene loading, physics, sprite batching, audio or script IPC.
//! The native player in `crates/player` remains the authoritative runtime.

pub mod surface;

use std::{cell::RefCell, collections::HashMap};

use engine_core::input::{InputEvent, Key, PointerPhase};
use js_sys::Uint8Array;
use wasm_bindgen::{JsCast, JsValue, prelude::*};
use wasm_bindgen_futures::JsFuture;
use web_sys::{HtmlCanvasElement, Response, Window};

use surface::SurfaceRenderer;

/// Fixed simulation step, matching the desktop player's 60 Hz clock.
const STEP_MS: f64 = 1000.0 / 60.0;
/// Bounds catch-up work after a long frame so the browser stays responsive.
const MAX_CATCHUP_TICKS: u32 = 5;
/// Mirrors the desktop player's per-frame input backlog limit.
const INPUT_LIMIT: usize = 64;

// wasm is single-threaded; thread-local slots keep the RAF callback and the
// JS-facing input hooks pointed at the same app without shared ownership.
thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static RAF: RefCell<Option<Closure<dyn FnMut(f64)>>> = const { RefCell::new(None) };
}

/// Browser shell state: surface, scriptless runtime, placeholder VFS and input.
struct App {
    canvas: HtmlCanvasElement,
    renderer: SurfaceRenderer,
    runtime: ScriptlessRuntime,
    assets: AssetVfs,
    pending: Vec<InputEvent>,
    last_frame_ms: Option<f64>,
    accumulator_ms: f64,
}

impl App {
    async fn new(canvas: HtmlCanvasElement) -> Result<Self, String> {
        let renderer = SurfaceRenderer::new(canvas.clone()).await?;
        Ok(Self {
            canvas,
            renderer,
            runtime: ScriptlessRuntime::default(),
            assets: AssetVfs::default(),
            pending: Vec::new(),
            last_frame_ms: None,
            accumulator_ms: 0.0,
        })
    }

    fn frame(&mut self, now_ms: f64) -> Result<(), String> {
        self.renderer
            .resize([self.canvas.width(), self.canvas.height()]);
        let elapsed = match self.last_frame_ms.replace(now_ms) {
            Some(previous) => (now_ms - previous).clamp(0.0, 250.0),
            None => 0.0,
        };
        self.accumulator_ms += elapsed;
        let mut ticks = 0;
        while self.accumulator_ms >= STEP_MS && ticks < MAX_CATCHUP_TICKS {
            self.accumulator_ms -= STEP_MS;
            ticks += 1;
            // Input is consumed once at a tick boundary, as in the native loop.
            let input = std::mem::take(&mut self.pending);
            self.runtime.tick(&input);
        }
        if ticks == MAX_CATCHUP_TICKS {
            self.accumulator_ms = self.accumulator_ms.min(STEP_MS);
        }
        self.renderer.draw(self.runtime.tint())
    }

    fn push(&mut self, event: InputEvent) {
        if self.pending.len() < INPUT_LIMIT {
            self.pending.push(event);
        }
    }
}

/// Stand-in for the desktop runtime until scene and script support is ported.
///
/// Only tick and input accounting are retained. No child process, shared
/// memory, audio device or physics step is started in the browser.
#[derive(Default)]
struct ScriptlessRuntime {
    ticks: u64,
    inputs: u64,
}

impl ScriptlessRuntime {
    fn tick(&mut self, input: &[InputEvent]) {
        self.ticks += 1;
        self.inputs += input.len() as u64;
    }

    fn tint(&self) -> [f32; 4] {
        [0.0, 0.0, 0.0, 1.0]
    }
}

/// Placeholder virtual filesystem backed by `fetch`.
///
/// Fetched bytes are stored verbatim; PNG decoding and scene deserialization
/// are future work. Paths are used as given, so callers mount URLs they trust.
#[derive(Default)]
struct AssetVfs {
    files: HashMap<String, Vec<u8>>,
}

impl AssetVfs {
    async fn fetch(path: &str) -> Result<Vec<u8>, String> {
        let window = web_window()?;
        let response = JsFuture::from(window.fetch_with_str(path))
            .await
            .map_err(|error| format!("fetch {path}: {}", js_text(&error)))?;
        let response: Response = response
            .dyn_into()
            .map_err(|_| format!("fetch {path}: response is not a Response"))?;
        if !response.ok() {
            return Err(format!("fetch {path}: HTTP {}", response.status()));
        }
        let buffer = JsFuture::from(
            response
                .array_buffer()
                .map_err(|error| format!("fetch {path}: {}", js_text(&error)))?,
        )
        .await
        .map_err(|error| format!("fetch {path}: {}", js_text(&error)))?;
        Ok(Uint8Array::new(&buffer).to_vec())
    }

    fn insert(&mut self, path: &str, bytes: Vec<u8>) {
        self.files.insert(path.to_owned(), bytes);
    }
}

/// Creates or reattaches the WebGPU canvas shell and starts the RAF loop.
///
/// The shell JavaScript imports this module's `init` first, then awaits this
/// function with the canvas element. Asset mounting is a separate explicit call.
#[wasm_bindgen]
pub async fn start(canvas: HtmlCanvasElement) -> Result<(), JsValue> {
    let app = App::new(canvas)
        .await
        .map_err(|error| JsValue::from_str(&error))?;
    APP.with(|slot| *slot.borrow_mut() = Some(app));

    let window = web_window().map_err(|error| JsValue::from_str(&error))?;
    let callback_window = window.clone();
    let callback = Closure::wrap(Box::new(move |now_ms: f64| {
        let result = APP.with(|slot| {
            let mut slot = slot.borrow_mut();
            let Some(app) = slot.as_mut() else {
                return Ok(());
            };
            app.frame(now_ms)
        });
        if let Err(error) = result {
            web_sys::console::error_1(&JsValue::from_str(&error));
            return;
        }
        RAF.with(|slot| {
            let slot = slot.borrow();
            if let Some(callback) = slot.as_ref() {
                let function: &js_sys::Function = callback.as_js_value().unchecked_ref();
                let _ = callback_window.request_animation_frame(function);
            }
        });
    }) as Box<dyn FnMut(f64)>);
    RAF.with(|slot| *slot.borrow_mut() = Some(callback));
    RAF.with(|slot| -> Result<(), JsValue> {
        let slot = slot.borrow();
        let callback = slot
            .as_ref()
            .ok_or_else(|| JsValue::from_str("missing animation callback"))?;
        let function: &js_sys::Function = callback.as_js_value().unchecked_ref();
        window
            .request_animation_frame(function)
            .map_err(|error| JsValue::from_str(&js_text(&error)))?;
        Ok(())
    })
}

/// Fetches one asset into the placeholder VFS for later use by a scene loader.
#[wasm_bindgen]
pub async fn mount_asset(path: &str) -> Result<(), JsValue> {
    let bytes = AssetVfs::fetch(path)
        .await
        .map_err(|error| JsValue::from_str(&error))?;
    APP.with(|slot| {
        let mut slot = slot.borrow_mut();
        let app = slot
            .as_mut()
            .ok_or_else(|| JsValue::from_str("player is not started"))?;
        app.assets.insert(path, bytes);
        Ok::<(), JsValue>(())
    })?;
    Ok(())
}

/// Forwards a physical key using engine_core's single-byte key table.
///
/// `code` is the raw byte from that table (1 = Space, 65 = A, and so on); the
/// high bit is the editor preview marker and is added here.
#[wasm_bindgen]
pub fn on_key(code: u8, down: bool) {
    let Some(key) = Key::from_preview_byte(code | 0x80) else {
        return;
    };
    let event = if down {
        InputEvent::KeyDown(key)
    } else {
        InputEvent::KeyUp(key)
    };
    with_app(|app| app.push(event));
}

/// Forwards a pointer contact: phase 0 start, 1 move, 2 end, 3 cancel.
///
/// Coordinates are normalized to the canvas and `id` is the stable contact
/// identifier used by `engine_core::input`.
#[wasm_bindgen]
pub fn on_pointer(phase: u8, x: f32, y: f32, id: u32, width: u32, height: u32) {
    let position = [x, y];
    let viewport = [width, height];
    let id = u64::from(id);
    let phase = match phase {
        0 => PointerPhase::Start,
        1 => PointerPhase::Move,
        2 => PointerPhase::End,
        3 => PointerPhase::Cancel,
        _ => return,
    };
    let event = match phase {
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
    };
    with_app(|app| app.push(event));
}

/// Forwards a vertical or horizontal wheel step at a normalized position.
#[wasm_bindgen]
pub fn on_wheel(x: f32, y: f32, dx: f32, dy: f32, width: u32, height: u32) {
    with_app(|app| {
        app.push(InputEvent::Wheel {
            position: [x, y],
            delta: [dx, dy],
            viewport: [width, height],
        })
    });
}

/// Drops every active contact, as the native player does on focus loss.
#[wasm_bindgen]
pub fn cancel_pointers() {
    with_app(|app| app.push(InputEvent::CancelPointers));
}

/// Completed fixed ticks, exposed for shell diagnostics.
#[wasm_bindgen]
pub fn tick_count() -> u64 {
    APP.with(|slot| slot.borrow().as_ref().map_or(0, |app| app.runtime.ticks))
}

/// Number of forwarded input events observed by the scriptless runtime.
#[wasm_bindgen]
pub fn input_count() -> u64 {
    APP.with(|slot| slot.borrow().as_ref().map_or(0, |app| app.runtime.inputs))
}

/// Total bytes mounted by the placeholder VFS.
#[wasm_bindgen]
pub fn asset_bytes() -> usize {
    APP.with(|slot| {
        slot.borrow()
            .as_ref()
            .map_or(0, |app| app.assets.files.values().map(Vec::len).sum())
    })
}

fn with_app(f: impl FnOnce(&mut App)) {
    APP.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some(app) = slot.as_mut() {
            f(app);
        }
    });
}

fn web_window() -> Result<Window, String> {
    web_sys::window().ok_or_else(|| "no browser window is available".to_owned())
}

fn js_text(value: &JsValue) -> String {
    value.as_string().unwrap_or_else(|| format!("{value:?}"))
}
