// Aurora browser shell glue.
//
// The wasm-bindgen `--target web` glue (aurora_web_player.js and
// aurora_web_player_bg.wasm) is generated next to this file; it is not checked
// in. This module sets up the canvas, forwards DOM input into the engine_core
// input model exposed by the wasm crate, and polls diagnostic counters.

import init, {
  start,
  mount_asset,
  on_key,
  on_pointer,
  on_wheel,
  cancel_pointers,
  tick_count,
  input_count,
  asset_bytes,
} from './aurora_web_player.js';

// Raw discriminants from engine_core::input::Key. The wasm side adds the
// editor preview marker bit before decoding them.
const KEY_CODES = Object.assign(
  {
    Space: 1,
    ArrowUp: 2,
    Backspace: 8,
    Tab: 9,
    Enter: 13,
    ShiftLeft: 16,
    ShiftRight: 16,
    ControlLeft: 17,
    ControlRight: 17,
    AltLeft: 18,
    AltRight: 18,
    Escape: 27,
    PageUp: 33,
    PageDown: 34,
    End: 35,
    Home: 36,
    ArrowLeft: 37,
    ArrowRight: 39,
    ArrowDown: 40,
    Insert: 45,
    Delete: 46,
    Semicolon: 186,
    Equal: 187,
    Comma: 188,
    Minus: 189,
    Period: 190,
    Slash: 191,
    Backquote: 192,
    BracketLeft: 219,
    Backslash: 220,
    BracketRight: 221,
    Quote: 222,
  },
  Object.fromEntries(Array.from({ length: 10 }, (_, i) => [`Digit${i}`, 48 + i])),
  Object.fromEntries(
    Array.from({ length: 26 }, (_, i) => [`Key${String.fromCharCode(65 + i)}`, 65 + i]),
  ),
  Object.fromEntries(Array.from({ length: 12 }, (_, i) => [`F${i + 1}`, 112 + i])),
);

// Pointer phases expected by the wasm `on_pointer` export.
const POINTER_START = 0;
const POINTER_MOVE = 1;
const POINTER_END = 2;
const POINTER_CANCEL = 3;

// Placeholder manifest for the fetch-backed VFS. Add URLs that the page serves
// (for example '/assets/aurora.png') once a scene loader consumes them.
const ASSET_MANIFEST = [];

const canvas = document.getElementById('aurora-canvas');
const status = document.getElementById('status');
const activePointers = new Set();

function fail(error) {
  const message = error instanceof Error ? error.message : String(error);
  status.textContent = `Aurora shell failed: ${message}`;
  console.error(error);
}

function pointerPosition(event) {
  const bounds = canvas.getBoundingClientRect();
  const x = bounds.width > 0 ? (event.clientX - bounds.left) / bounds.width : 0;
  const y = bounds.height > 0 ? (event.clientY - bounds.top) / bounds.height : 0;
  return [x, y];
}

// Mouse contacts use engine_core's MOUSE_POINTER (0); touch identities start at
// one so they never collide with it.
function pointerId(event) {
  return event.pointerType === 'mouse' ? 0 : event.pointerId + 1;
}

function sendPointer(phase, event) {
  const [x, y] = pointerPosition(event);
  on_pointer(phase, x, y, pointerId(event), canvas.width, canvas.height);
}

function installInput() {
  canvas.addEventListener('keydown', (event) => {
    const code = KEY_CODES[event.code];
    if (code === undefined || event.repeat) return;
    on_key(code, true);
    if (event.code === 'Space' || event.code.startsWith('Arrow') || event.code === 'Tab') {
      event.preventDefault();
    }
  });

  canvas.addEventListener('keyup', (event) => {
    const code = KEY_CODES[event.code];
    if (code === undefined) return;
    on_key(code, false);
    if (event.code === 'Space' || event.code.startsWith('Arrow') || event.code === 'Tab') {
      event.preventDefault();
    }
  });

  canvas.addEventListener('pointerdown', (event) => {
    activePointers.add(event.pointerId);
    canvas.setPointerCapture(event.pointerId);
    canvas.focus();
    sendPointer(POINTER_START, event);
  });

  canvas.addEventListener('pointermove', (event) => {
    if (activePointers.has(event.pointerId)) sendPointer(POINTER_MOVE, event);
  });

  const release = (phase) => (event) => {
    if (!activePointers.delete(event.pointerId)) return;
    sendPointer(phase, event);
  };
  canvas.addEventListener('pointerup', release(POINTER_END));
  canvas.addEventListener('pointercancel', release(POINTER_CANCEL));
  canvas.addEventListener('lostpointercapture', release(POINTER_CANCEL));

  canvas.addEventListener('wheel', (event) => {
    const [x, y] = pointerPosition(event);
    // DOM deltas are CSS pixels; the native loop uses wheel line units.
    on_wheel(x, y, event.deltaX, event.deltaY, canvas.width, canvas.height);
    event.preventDefault();
  }, { passive: false });

  window.addEventListener('blur', cancel_pointers);
}

async function mountAssets() {
  for (const path of ASSET_MANIFEST) {
    try {
      await mount_asset(path);
    } catch (error) {
      console.warn(`Could not mount ${path}:`, error);
    }
  }
}

async function main() {
  await init();
  await start(canvas);
  installInput();
  await mountAssets();
  status.textContent =
    'Running groundwork loop (audio disabled: browser audio needs a user gesture).';
  setInterval(() => {
    status.textContent =
      `ticks: ${tick_count()} | input: ${input_count()} | assets: ${asset_bytes()} bytes\n` +
      'groundwork only: no scene, physics, script child or audio.';
  }, 250);
}

main().catch(fail);
