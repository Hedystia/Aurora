export interface Property {
  type:
    | "number"
    | "boolean"
    | "string"
    | "node"
    | "prefab"
    | "number_array"
    | "boolean_array"
    | "string_array"
    | "node_array"
    | "component";
  tooltip?: string;
  component?: string;
}
export interface ComponentMetadata {
  props: Record<string, Property>;
}
export function registerComponent(entry: [string, ComponentMetadata]): [string, ComponentMetadata];
export const EventPhase: {
  readonly NONE: 0;
  readonly CAPTURING_PHASE: 1;
  readonly AT_TARGET: 2;
  readonly BUBBLING_PHASE: 3;
};
export class ScriptEvent {
  constructor(
    type: string,
    value?: number,
    bubbles?: boolean,
    x?: number,
    y?: number,
    pointerId?: number,
    scrollX?: number,
    scrollY?: number,
    nodeX?: number,
    nodeY?: number,
  );
  readonly type: string;
  readonly value: number;
  readonly keyCode: number;
  readonly bubbles: boolean;
  readonly x: number;
  readonly y: number;
  readonly pointerId: number;
  readonly scrollX: number;
  readonly scrollY: number;
  readonly nodeX: number;
  readonly nodeY: number;
  readonly target: NodeRef | null;
  readonly currentTarget: NodeRef | null;
  readonly eventPhase: number;
  stopPropagation(): void;
  stopImmediatePropagation(): void;
}
export type EventCallback = (event: ScriptEvent) => void;
export class EventTarget {
  on(type: string, callback: EventCallback, capture?: boolean): EventCallback;
  once(type: string, callback: EventCallback, capture?: boolean): EventCallback;
  off(type: string, callback?: EventCallback, capture?: boolean): void;
  hasEventListener(type: string, callback?: EventCallback, capture?: boolean): boolean;
  emit(type: string, value?: number): void;
  clear(): void;
}
export class Input extends EventTarget {
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
export const InputEventType: typeof Input.EventType;
export const PointerEventType: {
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
export class InputRouter {
  on(type: string, callback: EventCallback): EventCallback;
  once(type: string, callback: EventCallback): EventCallback;
  off(type: string, callback?: EventCallback): void;
}
export const input: InputRouter;
export const KeyCode: {
  readonly SPACE: 1;
  readonly ARROW_UP: 2;
  readonly BACKSPACE: 8;
  readonly ENTER: 13;
  readonly ESCAPE: 27;
  readonly ARROW_LEFT: 37;
  readonly ARROW_RIGHT: 39;
  readonly ARROW_DOWN: 40;
  readonly DIGIT_0: 48;
  readonly DIGIT_1: 49;
  readonly DIGIT_2: 50;
  readonly DIGIT_3: 51;
  readonly DIGIT_4: 52;
  readonly DIGIT_5: 53;
  readonly DIGIT_6: 54;
  readonly DIGIT_7: 55;
  readonly DIGIT_8: 56;
  readonly DIGIT_9: 57;
  readonly KEY_A: 65;
  readonly KEY_B: 66;
  readonly KEY_C: 67;
  readonly KEY_D: 68;
  readonly KEY_E: 69;
  readonly KEY_F: 70;
  readonly KEY_G: 71;
  readonly KEY_H: 72;
  readonly KEY_I: 73;
  readonly KEY_J: 74;
  readonly KEY_K: 75;
  readonly KEY_L: 76;
  readonly KEY_M: 77;
  readonly KEY_N: 78;
  readonly KEY_O: 79;
  readonly KEY_P: 80;
  readonly KEY_Q: 81;
  readonly KEY_R: 82;
  readonly KEY_S: 83;
  readonly KEY_T: 84;
  readonly KEY_U: 85;
  readonly KEY_V: 86;
  readonly KEY_W: 87;
  readonly KEY_X: 88;
  readonly KEY_Y: 89;
  readonly KEY_Z: 90;
  readonly TAB: 9;
  readonly SHIFT: 16;
  readonly CONTROL: 17;
  readonly ALT: 18;
  readonly PAGE_UP: 33;
  readonly PAGE_DOWN: 34;
  readonly END: 35;
  readonly HOME: 36;
  readonly INSERT: 45;
  readonly DELETE: 46;
  readonly F1: 112;
  readonly F2: 113;
  readonly F3: 114;
  readonly F4: 115;
  readonly F5: 116;
  readonly F6: 117;
  readonly F7: 118;
  readonly F8: 119;
  readonly F9: 120;
  readonly F10: 121;
  readonly F11: 122;
  readonly F12: 123;
  readonly SEMICOLON: 186;
  readonly EQUAL: 187;
  readonly COMMA: 188;
  readonly MINUS: 189;
  readonly PERIOD: 190;
  readonly SLASH: 191;
  readonly BACKQUOTE: 192;
  readonly BRACKET_LEFT: 219;
  readonly BACKSLASH: 220;
  readonly BRACKET_RIGHT: 221;
  readonly QUOTE: 222;
};
export type ScheduledCallback = (elapsed: number) => void;
export type ComponentConstructor<T extends Component> = abstract new () => T;
export class Node {
  addComponent<T extends Component>(type: ComponentConstructor<T> | string): void;
  removeComponent(component: Component): void;
  readonly name: string;
  readonly parent: NodeRef | null;
  setActive(visible: boolean): void;
  getComponent<T extends Component>(type: ComponentConstructor<T>): T | null;
  getComponent(name: string): Component | null;
  getComponents<T extends Component>(type: ComponentConstructor<T>): T[];
  getComponents(name: string): Component[];
  readonly children: NodeRef[];
  readonly childCount: number;
  nodeRef(): NodeRef | null;
  getChildByName(name: string): NodeRef | null;
  getChildByPath(path: string): NodeRef | null;
  on(type: string, callback: EventCallback, capture?: boolean): EventCallback;
  once(type: string, callback: EventCallback, capture?: boolean): EventCallback;
  off(type: string, callback?: EventCallback, capture?: boolean): void;
  emit(type: string, value?: number): void;
  dispatchEvent(event: ScriptEvent): void;
  removeFromParent(): void;
  x: number;
  y: number;
  scaleX: number;
  scaleY: number;
  angle: number;
  setPosition(x: number, y: number): void;
  setScale(x: number, y: number): void;
  setAngle(angle: number): void;
}
export class NodeHandle {
  constructor(low: number, high: number);
  readonly low: number;
  readonly high: number;
}
export class RigidBody2D {
  setLinearVelocity(x: number, y: number): void;
}
export class Collider2D {
  constructor(node: NodeHandle);
  readonly node: NodeHandle;
}
export class AudioSource {
  playClip(clip: AudioClip): void;
  playOneShot(index: number): void;
}
export class Director {
  pause(): void;
  resume(): void;
  changeScene(index: number): void;
  loadScene(name: string): boolean;
}
export class SceneCommands {
  addComponent(node: number, type: ComponentConstructor<Component> | string): void;
  removeComponent(node: number, component: Component): void;
  move(node: number, x: number, y: number): void;
  scale(node: number, x: number, y: number): void;
  rotate(node: number, angle: number): void;
  show(node: number, visible: boolean): void;
  frame(node: number, frame: number): void;
  setSpriteFrame(node: NodeRef, frame: SpriteFrame): void;
  sound(clip: number): void;
  bestScore(score: number): void;
  registerPrefab(path: string, assetIndex: number): void;
  loadPrefab(path: string): Prefab | null;
  createNode(parent?: NodeRef): NodeRef;
  instantiate(prefab: Prefab, parent?: NodeRef): NodeRef;
  reparent(node: number, parent: NodeRef | null): void;
  destroy(node: number): void;
}
export class NodeRef {
  addComponent<T extends Component>(type: ComponentConstructor<T> | string): void;
  removeComponent(component: Component): void;
  readonly name: string;
  readonly parent: NodeRef | null;
  getComponent<T extends Component>(type: ComponentConstructor<T>): T | null;
  getComponent(name: string): Component | null;
  getComponents<T extends Component>(type: ComponentConstructor<T>): T[];
  getComponents(name: string): Component[];
  readonly children: NodeRef[];
  readonly childCount: number;
  getChildByName(name: string): NodeRef | null;
  getChildByPath(path: string): NodeRef | null;
  on(type: string, callback: EventCallback, capture?: boolean): EventCallback;
  once(type: string, callback: EventCallback, capture?: boolean): EventCallback;
  off(type: string, callback?: EventCallback, capture?: boolean): void;
  emit(type: string, value?: number): void;
  dispatchEvent(event: ScriptEvent): void;
  constructor(index: number, commands: SceneCommands);
  readonly index: number;
  x: number;
  y: number;
  scaleX: number;
  scaleY: number;
  angle: number;
  setPosition(x: number, y: number): void;
  setScale(x: number, y: number): void;
  setAngle(angle: number): void;
  setActive(visible: boolean): void;
  setParent(parent: NodeRef | null): void;
  removeFromParent(): void;
  destroy(): void;
}
export class Prefab {
  constructor(path: string, assetIndex?: number);
  readonly path: string;
  readonly assetIndex: number;
}
export class NodePool {
  constructor(slots: number[]);
  get(): number;
  reset(scene: SceneCommands, slot: number, x: number, y: number): void;
  put(scene: SceneCommands, slot: number): void;
}
export class Component {
  readonly node: Node;
  readonly audio: AudioSource;
  readonly body: RigidBody2D;
  readonly director: Director;
  readonly scene: SceneCommands;
  readonly resources: Resources;
  readonly input: Input;
  readonly events: EventTarget;
  schedule(callback: ScheduledCallback, interval?: number, repeat?: number, delay?: number): void;
  scheduleOnce(callback: ScheduledCallback, delay?: number): void;
  unschedule(callback: ScheduledCallback): void;
  unscheduleAllCallbacks(): void;
  getComponent<T extends Component>(type: ComponentConstructor<T>): T | null;
  getComponent(name: string): Component | null;
  getComponents<T extends Component>(type: ComponentConstructor<T>): T[];
  getComponents(name: string): Component[];
  enabled: boolean;
  readonly enabledInHierarchy: boolean;
  onLoad(): void;
  onEnable(): void;
  onDisable(): void;
  onDestroy(): void;
  start(): void;
  update(dt: number): void;
  fixedUpdate(dt: number): void;
  lateUpdate(dt: number): void;
  onStart(): void;
  onUpdate(dt: number): void;
  onFixedUpdate(dt: number): void;
  onKeyDown(key: number): void;
  onKeyUp(key: number): void;
  onTouchStart(): void;
  onTouchMove(event: ScriptEvent): void;
  onTouchEnd(event: ScriptEvent): void;
  onTouchCancel(event: ScriptEvent): void;
  onBeginContact(other: Collider2D): void;
  onEndContact(other: Collider2D): void;
  onAnimationEvent(code: number): void;
  onBestScore(score: number): void;
}
type PropertyToken =
  | NumberConstructor
  | BooleanConstructor
  | StringConstructor
  | typeof NodeRef
  | typeof Prefab
  | typeof AudioClip
  | typeof SpriteFrame
  | ComponentConstructor<Component>;
type PropertyOptions = { type?: PropertyToken; tooltip?: string };
export function ccclass(target: Function): void;
export function ccclass(name?: string): ClassDecorator;
export function property(target: object, key: string | symbol): void;
export function property(options?: PropertyToken | PropertyOptions): PropertyDecorator;
export type EasingName =
  | "linear"
  | "quadIn"
  | "quadOut"
  | "quadInOut"
  | "cubicIn"
  | "cubicOut"
  | "cubicInOut";
export interface TweenValues {
  x?: number;
  y?: number;
  position?: { x?: number; y?: number };
  scaleX?: number;
  scaleY?: number;
  scale?: { x?: number; y?: number };
  angle?: number;
}
export class Tween {
  constructor(target: NodeRef);
  to(
    duration: number,
    values: TweenValues,
    options?: { easing?: EasingName },
  ): this;
  by(
    duration: number,
    values: TweenValues,
    options?: { easing?: EasingName },
  ): this;
  delay(seconds: number): this;
  call(callback: () => void): this;
  repeat(times?: number): this;
  repeatForever(): this;
  start(): this;
  stop(): this;
}
export function tween(target: NodeRef | Node): Tween;
export function executionOrder(order: number): ClassDecorator;

export class AudioClip {
  constructor(path: string, index: number);
  readonly path: string;
  readonly index: number;
}
export class SpriteFrame {
  constructor(path: string, nodes: number[], slots: number[]);
  readonly path: string;
  frameFor(node: number): number;
}
export type AudioClipCallback = (error: Error | null, clip: AudioClip | null) => void;
export type SpriteFrameCallback = (error: Error | null, frame: SpriteFrame | null) => void;
export type PrefabCallback = (error: Error | null, prefab: Prefab | null) => void;
export class Resources {
  constructor(scene: SceneCommands);
  bundle(root?: string): AssetBundle;
  releaseAudioClip(path: string): void;
  releaseSpriteFrame(path: string): void;
  getAudioClip(path: string): AudioClip | null;
  requireAudioClip(path: string): AudioClip;
  getSpriteFrame(path: string): SpriteFrame | null;
  requireSpriteFrame(path: string): SpriteFrame;
  getPrefab(path: string): Prefab | null;
  loadAudioClip(path: string, callback: AudioClipCallback): void;
  loadSpriteFrame(path: string, callback: SpriteFrameCallback): void;
  loadPrefab(path: string, callback: PrefabCallback): void;
  releaseAll(): void;
  cancelLoads(): void;
  readonly pendingCount: number;
}


/** A directory-scoped view of resources registered in the current scene. */
export class AssetBundle {
  constructor(resources: Resources, root: string);
  getAudioClip(path: string): AudioClip | null;
  requireAudioClip(path: string): AudioClip;
  getSpriteFrame(path: string): SpriteFrame | null;
  requireSpriteFrame(path: string): SpriteFrame;
  getPrefab(path: string): Prefab | null;
  loadAudioClip(path: string, callback: AudioClipCallback): void;
  loadSpriteFrame(path: string, callback: SpriteFrameCallback): void;
  loadPrefab(path: string, callback: PrefabCallback): void;
  preloadAudioClip(path: string, callback: AudioClipCallback): void;
  preloadSpriteFrame(path: string, callback: SpriteFrameCallback): void;
  preloadPrefab(path: string, callback: PrefabCallback): void;
  releaseAudioClip(path: string): void;
  releaseSpriteFrame(path: string): void;
  releaseAll(): void;
  cancelLoads(): void;
  release(): void;
}
