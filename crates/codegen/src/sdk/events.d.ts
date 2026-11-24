import type { NodeRef } from "./index.js";
export declare const EventPhase: {
    NONE: number;
    CAPTURING_PHASE: number;
    AT_TARGET: number;
    BUBBLING_PHASE: number;
};
/** A local custom event or a player input notification. */
export declare class ScriptEvent {
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
    target: NodeRef | null;
    currentTarget: NodeRef | null;
    eventPhase: number;
    private propagationStopped;
    private immediateStopped;
    private dispatching;
    constructor(type: string, value?: number, bubbles?: boolean, x?: number, y?: number, pointerId?: number, scrollX?: number, scrollY?: number, nodeX?: number, nodeY?: number);
    stopPropagation(): void;
    stopImmediatePropagation(): void;
    /** @internal */
    _begin(target: NodeRef): void;
    /** @internal */
    _finish(): void;
    /** @internal */
    _isPropagationStopped(): boolean;
    /** @internal */
    _isImmediatePropagationStopped(): boolean;
}
export type EventCallback = (event: ScriptEvent) => void;
/** Dispatches typed events synchronously, in registration order. */
export declare class EventTarget {
    private listeners;
    private add;
    on(type: string, callback: EventCallback, capture?: boolean): EventCallback;
    once(type: string, callback: EventCallback, capture?: boolean): EventCallback;
    off(type: string, callback?: EventCallback, capture?: boolean): void;
    hasEventListener(type: string, callback?: EventCallback, capture?: boolean): boolean;
    emit(type: string, value?: number): void;
    /** @internal Invokes the listeners for one phase without restarting propagation. */
    dispatch(event: ScriptEvent, capture: boolean): void;
    clear(): void;
}
/** Player input routed to a component under its own script binding. */
export declare class Input extends EventTarget {
    static readonly EventType: {
        readonly KEY_DOWN: "key-down";
        readonly KEY_UP: "key-up";
        readonly TOUCH_START: "touch-start";
        readonly TOUCH_MOVE: "touch-move";
        readonly TOUCH_END: "touch-end";
        readonly TOUCH_CANCEL: "touch-cancel";
        readonly MOUSE_DOWN: "mouse-down";
        readonly MOUSE_MOVE: "mouse-move";
        readonly MOUSE_UP: "mouse-up";
        readonly MOUSE_WHEEL: "mouse-wheel";
    };
}
/** Event names used for node-targeted pointer delivery. */
export declare const PointerEventType: {
    readonly TOUCH_START: "touch-start";
    readonly TOUCH_MOVE: "touch-move";
    readonly TOUCH_END: "touch-end";
    readonly TOUCH_CANCEL: "touch-cancel";
    readonly MOUSE_DOWN: "mouse-down";
    readonly MOUSE_MOVE: "mouse-move";
    readonly MOUSE_UP: "mouse-up";
    readonly MOUSE_WHEEL: "mouse-wheel";
};
export type PointerPhase = "touch-start" | "touch-move" | "touch-end" | "touch-cancel";
/** Routes subscriptions declared by a component to its player input binding. */
export declare class InputRouter {
    private owner;
    /** @internal */
    _select(owner: Input | null): void;
    private current;
    on(type: string, callback: EventCallback): EventCallback;
    once(type: string, callback: EventCallback): EventCallback;
    off(type: string, callback?: EventCallback): void;
}
export declare const input: InputRouter;
export declare const InputEventType: {
    readonly KEY_DOWN: "key-down";
    readonly KEY_UP: "key-up";
    readonly TOUCH_START: "touch-start";
    readonly TOUCH_MOVE: "touch-move";
    readonly TOUCH_END: "touch-end";
    readonly TOUCH_CANCEL: "touch-cancel";
    readonly MOUSE_DOWN: "mouse-down";
    readonly MOUSE_MOVE: "mouse-move";
    readonly MOUSE_UP: "mouse-up";
    readonly MOUSE_WHEEL: "mouse-wheel";
};
/** Key codes supplied by the current player transport. */
export declare const KeyCode: {
    SPACE: number;
    ARROW_UP: number;
    BACKSPACE: number;
    ENTER: number;
    ESCAPE: number;
    ARROW_LEFT: number;
    ARROW_RIGHT: number;
    ARROW_DOWN: number;
    DIGIT_0: number;
    DIGIT_1: number;
    DIGIT_2: number;
    DIGIT_3: number;
    DIGIT_4: number;
    DIGIT_5: number;
    DIGIT_6: number;
    DIGIT_7: number;
    DIGIT_8: number;
    DIGIT_9: number;
    KEY_A: number;
    KEY_B: number;
    KEY_C: number;
    KEY_D: number;
    KEY_E: number;
    KEY_F: number;
    KEY_G: number;
    KEY_H: number;
    KEY_I: number;
    KEY_J: number;
    KEY_K: number;
    KEY_L: number;
    KEY_M: number;
    KEY_N: number;
    KEY_O: number;
    KEY_P: number;
    KEY_Q: number;
    KEY_R: number;
    KEY_S: number;
    KEY_T: number;
    KEY_U: number;
    KEY_V: number;
    KEY_W: number;
    KEY_X: number;
    KEY_Y: number;
    KEY_Z: number;
    TAB: number;
    SHIFT: number;
    CONTROL: number;
    ALT: number;
    PAGE_UP: number;
    PAGE_DOWN: number;
    END: number;
    HOME: number;
    INSERT: number;
    DELETE: number;
    F1: number;
    F2: number;
    F3: number;
    F4: number;
    F5: number;
    F6: number;
    F7: number;
    F8: number;
    F9: number;
    F10: number;
    F11: number;
    F12: number;
    SEMICOLON: number;
    EQUAL: number;
    COMMA: number;
    MINUS: number;
    PERIOD: number;
    SLASH: number;
    BACKQUOTE: number;
    BRACKET_LEFT: number;
    BACKSLASH: number;
    BRACKET_RIGHT: number;
    QUOTE: number;
};
