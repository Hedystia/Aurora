import type { NodeRef } from "./index.ts";

const keyDown = "key-down";
const keyUp = "key-up";
const touchStart = "touch-start";
const touchMove = "touch-move";
const touchEnd = "touch-end";
const touchCancel = "touch-cancel";
const mouseDown = "mouse-down";
const mouseMove = "mouse-move";
const mouseUp = "mouse-up";
const mouseWheel = "mouse-wheel";

export const EventPhase = { NONE: 0, CAPTURING_PHASE: 1, AT_TARGET: 2, BUBBLING_PHASE: 3 };

/** A local custom event or a player input notification. */
export class ScriptEvent {
  readonly type: string;
  readonly value: number;
  readonly keyCode: number;
  readonly bubbles: boolean;
  /** Normalized pointer position, measured from the top left of the game frame. */
  readonly x: number;
  /** Normalized pointer position, measured from the top left of the game frame. */
  readonly y: number;
  /** Pointer identity for touch contacts; zero for the mouse and for non-pointer events. */
  readonly pointerId: number;
  /** Horizontal wheel delta; zero for non-wheel events. */
  readonly scrollX: number;
  /** Vertical wheel delta; positive scrolls down; zero for non-wheel events. */
  readonly scrollY: number;
  /** Pointer position in the target node's local pixels; zero outside node events. */
  readonly nodeX: number;
  /** Pointer position in the target node's local pixels; zero outside node events. */
  readonly nodeY: number;
  target: NodeRef | null = null;
  currentTarget: NodeRef | null = null;
  eventPhase = EventPhase.NONE;
  private propagationStopped = false;
  private immediateStopped = false;
  private dispatching = false;

  constructor(
    type: string,
    value = 0,
    bubbles = false,
    x = 0,
    y = 0,
    pointerId = 0,
    scrollX = 0,
    scrollY = 0,
    nodeX = 0,
    nodeY = 0,
  ) {
    this.type = type;
    this.value = value;
    this.keyCode = type === keyDown || type === keyUp ? value : 0;
    this.bubbles = bubbles;
    this.x = x;
    this.y = y;
    this.pointerId = pointerId;
    this.scrollX = scrollX;
    this.scrollY = scrollY;
    this.nodeX = nodeX;
    this.nodeY = nodeY;
  }

  stopPropagation(): void { this.propagationStopped = true; }

  stopImmediatePropagation(): void {
    this.propagationStopped = true;
    this.immediateStopped = true;
  }

  /** @internal */
  _begin(target: NodeRef): void {
    if (this.dispatching) throw new Error("The event is already being dispatched");
    this.dispatching = true;
    this.target = target;
    this.propagationStopped = false;
    this.immediateStopped = false;
  }

  /** @internal */
  _finish(): void {
    this.currentTarget = null;
    this.eventPhase = EventPhase.NONE;
    this.dispatching = false;
  }

  /** @internal */
  _isPropagationStopped(): boolean { return this.propagationStopped; }

  /** @internal */
  _isImmediatePropagationStopped(): boolean { return this.immediateStopped; }
}

export type EventCallback = (event: ScriptEvent) => void;

class Listener {
  readonly type: string;
  readonly callback: EventCallback;
  readonly once: boolean;
  readonly capture: boolean;
  active = true;

  constructor(type: string, callback: EventCallback, once: boolean, capture: boolean) {
    this.type = type;
    this.callback = callback;
    this.once = once;
    this.capture = capture;
  }
}

/** Dispatches typed events synchronously, in registration order. */
export class EventTarget {
  private listeners: Listener[] = [];

  private add(type: string, callback: EventCallback, once: boolean, capture: boolean): EventCallback {
    if (!this.hasEventListener(type, callback, capture)) {
      this.listeners.push(new Listener(type, callback, once, capture));
    }
    return callback;
  }

  on(type: string, callback: EventCallback, capture = false): EventCallback {
    return this.add(type, callback, false, capture);
  }

  once(type: string, callback: EventCallback, capture = false): EventCallback {
    return this.add(type, callback, true, capture);
  }

  off(type: string, callback?: EventCallback, capture?: boolean): void {
    for (let index = this.listeners.length - 1; index >= 0; index--) {
      const listener = this.listeners[index];
      if (listener.type === type && (callback === undefined || listener.callback === callback)
          && (capture === undefined || listener.capture === capture)) {
        listener.active = false;
        this.listeners.splice(index, 1);
      }
    }
  }

  hasEventListener(type: string, callback?: EventCallback, capture?: boolean): boolean {
    for (const listener of this.listeners) {
      if (listener.type === type && (callback === undefined || listener.callback === callback)
          && (capture === undefined || listener.capture === capture)) return true;
    }
    return false;
  }

  emit(type: string, value = 0): void {
    this.dispatch(new ScriptEvent(type, value), false);
  }

  /** @internal Invokes the listeners for one phase without restarting propagation. */
  dispatch(event: ScriptEvent, capture: boolean): void {
    for (const listener of this.listeners.slice()) {
      if (event._isImmediatePropagationStopped()) break;
      if (!listener.active || listener.type !== event.type || listener.capture !== capture) continue;
      const callback = listener.callback;
      if (listener.once) this.off(event.type, callback, capture);
      callback(event);
    }
  }

  clear(): void {
    for (const listener of this.listeners) listener.active = false;
    this.listeners = [];
  }
}

/** Player input routed to a component under its own script binding. */
export class Input extends EventTarget {
  static readonly EventType = {
    KEY_DOWN: keyDown,
    KEY_UP: keyUp,
    TOUCH_START: touchStart,
    TOUCH_MOVE: touchMove,
    TOUCH_END: touchEnd,
    TOUCH_CANCEL: touchCancel,
    MOUSE_DOWN: mouseDown,
    MOUSE_MOVE: mouseMove,
    MOUSE_UP: mouseUp,
    MOUSE_WHEEL: mouseWheel,
  } as const;
}

/** Event names used for node-targeted pointer delivery. */
export const PointerEventType = {
  TOUCH_START: touchStart,
  TOUCH_MOVE: touchMove,
  TOUCH_END: touchEnd,
  TOUCH_CANCEL: touchCancel,
  MOUSE_DOWN: mouseDown,
  MOUSE_MOVE: mouseMove,
  MOUSE_UP: mouseUp,
  MOUSE_WHEEL: mouseWheel,
} as const;

export type PointerPhase = "touch-start" | "touch-move" | "touch-end" | "touch-cancel";

/** Routes subscriptions declared by a component to its player input binding. */
export class InputRouter {
  private owner: Input | null = null;

  /** @internal */
  _select(owner: Input | null): void {
    this.owner = owner;
  }

  private current(): Input {
    if (this.owner === null) throw new Error("Subscribe to input inside a component callback");
    return this.owner;
  }

  on(type: string, callback: EventCallback): EventCallback {
    return this.current().on(type, callback);
  }

  once(type: string, callback: EventCallback): EventCallback {
    return this.current().once(type, callback);
  }

  off(type: string, callback?: EventCallback): void {
    this.current().off(type, callback);
  }
}

export const input = new InputRouter();

export const InputEventType = Input.EventType;

/** Key codes supplied by the current player transport. */
export const KeyCode = {
  SPACE: 1,
  ARROW_UP: 2,
  BACKSPACE: 8,
  ENTER: 13,
  ESCAPE: 27,
  ARROW_LEFT: 37,
  ARROW_RIGHT: 39,
  ARROW_DOWN: 40,
  DIGIT_0: 48,
  DIGIT_1: 49,
  DIGIT_2: 50,
  DIGIT_3: 51,
  DIGIT_4: 52,
  DIGIT_5: 53,
  DIGIT_6: 54,
  DIGIT_7: 55,
  DIGIT_8: 56,
  DIGIT_9: 57,
  KEY_A: 65,
  KEY_B: 66,
  KEY_C: 67,
  KEY_D: 68,
  KEY_E: 69,
  KEY_F: 70,
  KEY_G: 71,
  KEY_H: 72,
  KEY_I: 73,
  KEY_J: 74,
  KEY_K: 75,
  KEY_L: 76,
  KEY_M: 77,
  KEY_N: 78,
  KEY_O: 79,
  KEY_P: 80,
  KEY_Q: 81,
  KEY_R: 82,
  KEY_S: 83,
  KEY_T: 84,
  KEY_U: 85,
  KEY_V: 86,
  KEY_W: 87,
  KEY_X: 88,
  KEY_Y: 89,
  KEY_Z: 90,
  TAB: 9,
  SHIFT: 16,
  CONTROL: 17,
  ALT: 18,
  PAGE_UP: 33,
  PAGE_DOWN: 34,
  END: 35,
  HOME: 36,
  INSERT: 45,
  DELETE: 46,
  F1: 112,
  F2: 113,
  F3: 114,
  F4: 115,
  F5: 116,
  F6: 117,
  F7: 118,
  F8: 119,
  F9: 120,
  F10: 121,
  F11: 122,
  F12: 123,
  SEMICOLON: 186,
  EQUAL: 187,
  COMMA: 188,
  MINUS: 189,
  PERIOD: 190,
  SLASH: 191,
  BACKQUOTE: 192,
  BRACKET_LEFT: 219,
  BACKSLASH: 220,
  BRACKET_RIGHT: 221,
  QUOTE: 222,
};
