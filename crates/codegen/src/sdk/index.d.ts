import { AudioClip, Resources, SpriteFrame } from "./resources.js";
export { AssetBundle, AudioClip, Resources, SpriteFrame, registerScriptResources } from "./resources.js";
export type { AudioClipCallback, PrefabCallback, SpriteFrameCallback } from "./resources.js";
import { EventCallback, EventTarget, Input, PointerPhase, ScriptEvent } from "./events.js";
import { ScheduledCallback } from "./scheduler.js";
export { EventPhase, EventTarget, Input, InputEventType, InputRouter, KeyCode, PointerEventType, ScriptEvent, input } from "./events.js";
export type { EventCallback } from "./events.js";
export type { PointerPhase } from "./events.js";
export type { ScheduledCallback } from "./scheduler.js";
export interface Property {
    type: "number" | "integer" | "boolean" | "string" | "text" | "vector2" | "vector3" | "size" | "rect" | "color" | "bitmask" | "enum" | "node" | "prefab" | "audio_clip" | "sprite_frame" | "number_array" | "boolean_array" | "string_array" | "node_array" | "component";
    tooltip?: string;
    component?: string;
    choices?: string[];
    readonly?: boolean;
    node_filter?: "text" | "image";
    min?: number;
    max?: number;
    step?: number;
    group?: string;
    slider?: boolean;
}
export interface ComponentMetadata {
    props: Record<string, Property>;
}
export declare function registerComponent(entry: [string, ComponentMetadata]): [string, ComponentMetadata];
export type ComponentConstructor<T extends Component> = abstract new () => T;
export declare class Node {
    x: number;
    y: number;
    changed: boolean;
    private sceneIndex;
    private sceneCommands;
    bindScene(index: number, commands: SceneCommands): void;
    get name(): string;
    get parent(): NodeRef | null;
    setActive(visible: boolean): void;
    setText(value: string): void;
    addComponent(type: string | ComponentConstructor<Component>): void;
    removeComponent(component: Component): void;
    getComponent<T extends Component>(type: ComponentConstructor<T>): T | null;
    getComponent(name: string): Component | null;
    getComponents<T extends Component>(type: ComponentConstructor<T>): T[];
    getComponents(name: string): Component[];
    on(type: string, callback: EventCallback, capture?: boolean): EventCallback;
    once(type: string, callback: EventCallback, capture?: boolean): EventCallback;
    off(type: string, callback?: EventCallback, capture?: boolean): void;
    emit(type: string, value?: number): void;
    dispatchEvent(event: ScriptEvent): void;
    /** Returns a script reference for this node when it is bound to a scene. */
    nodeRef(): NodeRef | null;
    get children(): NodeRef[];
    get childCount(): number;
    removeFromParent(): void;
    getChildByName(name: string): NodeRef | null;
    getChildByPath(path: string): NodeRef | null;
    setPosition(x: number, y: number): void;
    get scaleX(): number;
    set scaleX(value: number);
    get scaleY(): number;
    set scaleY(value: number);
    /** Local rotation in degrees. */
    get angle(): number;
    set angle(value: number);
    setScale(x: number, y: number): void;
    /** Sets the local rotation. The value is in degrees. */
    setAngle(angle: number): void;
}
export declare class NodeHandle {
    readonly low: number;
    readonly high: number;
    constructor(low: number, high: number);
}
export declare class RigidBody2D {
    changed: boolean;
    vx: number;
    vy: number;
    setLinearVelocity(x: number, y: number): void;
}
export declare class Collider2D {
    readonly node: NodeHandle;
    constructor(node: NodeHandle);
}
export declare class AudioSource {
    pending: number;
    playClip(clip: AudioClip): void;
    playOneShot(index: number): void;
}
/** @internal Registers the authored scene order before script instances are constructed. */
export declare function registerScriptScenes(names: string[]): void;
export declare class Director {
    action: number;
    scene: number;
    pause(): void;
    resume(): void;
    loadScene(name: string): boolean;
    changeScene(index: number): void;
}
export declare class SceneEdit {
    kind: number;
    target: number;
    x: number;
    y: number;
    constructor(kind: number, target: number, x: number, y: number);
}
export declare class SceneCommands {
    edits: SceneEdit[];
    cursor: number;
    private prefabs;
    clear(): void;
    private push;
    move(node: number, x: number, y: number): void;
    scale(node: number, x: number, y: number): void;
    rotate(node: number, angle: number): void;
    show(node: number, visible: boolean): void;
    frame(node: number, frame: number): void;
    setSpriteFrame(node: NodeRef, frame: SpriteFrame): void;
    setText(node: number, value: string): void;
    sound(clip: number): void;
    bestScore(score: number): void;
    addComponent(node: number, type: string | ComponentConstructor<Component>): void;
    removeComponent(node: number, component: Component): void;
    registerPrefab(path: string, assetIndex: number): void;
    loadPrefab(path: string): Prefab | null;
    createNode(parent?: NodeRef): NodeRef;
    instantiate(prefab: Prefab, parent?: NodeRef): NodeRef;
    private create;
    reparent(node: number, parent: NodeRef | null): void;
    destroy(node: number): void;
}
export declare class NodeRef {
    readonly index: number;
    private readonly commands;
    constructor(index: number, commands: SceneCommands);
    get name(): string;
    get x(): number;
    get y(): number;
    get scaleX(): number;
    set scaleX(value: number);
    get scaleY(): number;
    set scaleY(value: number);
    /** Local rotation in degrees. */
    get angle(): number;
    set angle(value: number);
    setScale(x: number, y: number): void;
    /** Sets the local rotation. The value is in degrees. */
    setAngle(angle: number): void;
    get parent(): NodeRef | null;
    addComponent(type: string | ComponentConstructor<Component>): void;
    removeComponent(component: Component): void;
    getComponent<T extends Component>(type: ComponentConstructor<T>): T | null;
    getComponent(name: string): Component | null;
    getComponents<T extends Component>(type: ComponentConstructor<T>): T[];
    getComponents(name: string): Component[];
    on(type: string, callback: EventCallback, capture?: boolean): EventCallback;
    once(type: string, callback: EventCallback, capture?: boolean): EventCallback;
    off(type: string, callback?: EventCallback, capture?: boolean): void;
    emit(type: string, value?: number): void;
    dispatchEvent(event: ScriptEvent): void;
    get children(): NodeRef[];
    get childCount(): number;
    getChildByName(name: string): NodeRef | null;
    getChildByPath(path: string): NodeRef | null;
    setPosition(x: number, y: number): void;
    setActive(visible: boolean): void;
    setText(value: string): void;
    setSpriteFrame(frame: SpriteFrame): void;
    setParent(parent: NodeRef | null): void;
    removeFromParent(): void;
    destroy(): void;
}
export declare class Prefab {
    readonly path: string;
    readonly assetIndex: number;
    constructor(path: string, assetIndex?: number);
}
export declare class TextRef extends NodeRef {
    get content(): string;
    set content(value: string);
}
export declare class ImageRef extends NodeRef {
}
export declare const Ref: {
    readonly Node: typeof NodeRef;
    readonly Text: typeof TextRef;
    readonly Image: typeof ImageRef;
    readonly Prefab: typeof Prefab;
    readonly AudioClip: typeof AudioClip;
    readonly SpriteFrame: typeof SpriteFrame;
};
export declare class NodePool {
    private available;
    private checked;
    constructor(slots: number[]);
    get(): number;
    reset(scene: SceneCommands, slot: number, x: number, y: number): void;
    put(scene: SceneCommands, slot: number): void;
}
export declare class Integer {
}
export declare class Text {
}
export declare class Vector2 {
}
export declare class Vector3 {
}
export declare class Size {
}
export declare class Rect {
}
export declare class Color {
}
export declare class Bitmask {
}
export declare class Image {
}
export declare class Component {
    readonly node: Node;
    readonly audio: AudioSource;
    readonly body: RigidBody2D;
    readonly director: Director;
    readonly scene: SceneCommands;
    readonly resources: Resources;
    /** Subscribe to keyboard or touch events delivered to this component. */
    readonly input: Input;
    /** Custom events scoped to this component. */
    readonly events: EventTarget;
    private readonly scheduler;
    private readonly scriptTimers;
    private scriptTime;
    schedule(callback: ScheduledCallback, interval?: number, repeat?: number, delay?: number): void;
    scheduleOnce(callback: ScheduledCallback, delay?: number): void;
    unschedule(callback: ScheduledCallback): void;
    unscheduleAllCallbacks(): void;
    /** @internal Called by the wire scheduler phase. */
    _tickScheduler(dt: number): void;
    /** @internal Schedules a global timer against simulation time. */
    _scheduleScriptTimer(callback: () => void, delay: number, interval: number | null): number;
    /** @internal Cancels a global timer owned by this component. */
    _clearScriptTimer(id: number): void;
    /** @internal Returns simulation time for requestAnimationFrame callbacks. */
    _scriptTimeMilliseconds(): number;
    /** @internal Delivers a resource completion in its own callback transaction. */
    _tickResources(): void;
    /** @internal Routes one player key notification. */
    _dispatchKeyDown(key: number): void;
    /** @internal Routes one player key release. */
    _dispatchKeyUp(key: number): void;
    /** @internal Routes one player touch notification. */
    _dispatchTouchStart(x?: number, y?: number, pointerId?: number): void;
    /** @internal Routes one mouse wheel notification. */
    _dispatchWheel(deltaX: number, deltaY: number): void;
    /** @internal Routes a pointer phase to input subscriptions and legacy callbacks. */
    _dispatchTouch(phase: PointerPhase, x: number, y: number, pointerId: number): void;
    /** @internal Releases listeners and timers on destruction. */
    _clearScriptServices(): void;
    getComponent<T extends Component>(type: ComponentConstructor<T>): T | null;
    getComponent(name: string): Component | null;
    getComponents<T extends Component>(type: ComponentConstructor<T>): T[];
    getComponents(name: string): Component[];
    enabled: boolean;
    private _active;
    get enabledInHierarchy(): boolean;
    _setLifecycleActive(active: boolean): void;
    onLoad(): void;
    onEnable(): void;
    onDisable(): void;
    onDestroy(): void;
    start(): void;
    update(dt: number): void;
    fixedUpdate(dt: number): void;
    lateUpdate(_dt: number): void;
    onStart(): void;
    onUpdate(_dt: number): void;
    onFixedUpdate(_dt: number): void;
    onKeyDown(_key: number): void;
    onKeyUp(_key: number): void;
    onTouchStart(): void;
    onTouchMove(_event: ScriptEvent): void;
    onTouchEnd(_event: ScriptEvent): void;
    onTouchCancel(_event: ScriptEvent): void;
    onBeginContact(_other: Collider2D): void;
    onEndContact(_other: Collider2D): void;
    onAnimationEvent(_code: number): void;
    onBestScore(_score: number): void;
}
type PrefabFactory = (references: number[]) => Component;
type PrefabBuilder = (references: number[]) => Component[];
/** @internal Selects the binding that owns subscriptions and queued edits for this callback. */
/** @internal Dispatches one player hit result under the current callback owner. */
export declare function dispatchPointerEvent(owner: Component, reference: number, phase: PointerPhase, x: number, y: number, pointerId: number, nodeX?: number, nodeY?: number): void;
/** @internal Dispatches one wheel event to a hit node and its ancestors. */
export declare function dispatchWheelEvent(owner: Component, reference: number, deltaX: number, deltaY: number): void;
export declare function setScriptCallbackOwner(component: Component | null): void;
/** Schedules a one-shot callback in simulation milliseconds. */
export declare function auroraSetTimeout<Arguments extends unknown[]>(callback: (...args: Arguments) => void, delay?: number, ...args: Arguments): number;
/** Schedules a repeating callback in simulation milliseconds. */
export declare function auroraSetInterval<Arguments extends unknown[]>(callback: (...args: Arguments) => void, delay?: number, ...args: Arguments): number;
/** Cancels a one-shot or repeating simulation timer. */
export declare function auroraClearTimer(id: number | undefined): void;
/** Schedules a callback for the next simulation frame. */
export declare function auroraRequestAnimationFrame(callback: (timestamp: number) => void): number;
export declare function bindScriptScene(nodes: string[], parents: number[], instances: Component[], owners: number[], names: string[], prefabPaths: string[], textContents?: (string | null)[]): void;
export declare function registerComponentFactory(name: string, factory: PrefabFactory): void;
/** @internal Associates a component constructor with its registered factory name. */
export declare function registerComponentClass(type: unknown, name: string): void;
/** @internal Caches one authored node transform before callbacks run. */
export declare function setScriptNodeTransform(reference: number, x: number, y: number): void;
/** @internal Caches one authored node scale and rotation. */
export declare function setScriptNodeScaleRotation(reference: number, x: number, y: number, angle: number): void;
/** @internal Resets scene-local handles before a scene's bindings load. */
export declare function resetScriptScene(): void;
export declare function registerRuntimeComponent(binding: number, owner: number, factory: number): Component;
export declare function registerScriptPrefab(asset: number, names: string[], build: PrefabBuilder, componentNames: string[], textContents?: (string | null)[]): void;
export declare function registerPrefabNode(reference: number, root: number, asset: number, parent: number): void;
export declare function registerPrefabComponent(binding: number, root: number, asset: number, ordinal: number, owner: number): Component;
export declare function prefabComponent(binding: number): Component | null;
export declare function releasePrefabComponent(binding: number): void;
export declare function releasePrefabNode(reference: number): void;
export declare function updatePrefabParent(reference: number, parent: number): void;
export type EasingName = "linear" | "quadIn" | "quadOut" | "quadInOut" | "cubicIn" | "cubicOut" | "cubicInOut";
/** Channels a tween can drive. Omitting a channel leaves it unchanged. */
export interface TweenValues {
    x?: number;
    y?: number;
    position?: {
        x?: number;
        y?: number;
    };
    scaleX?: number;
    scaleY?: number;
    scale?: {
        x?: number;
        y?: number;
    };
    angle?: number;
}
/** A chainable node tween advanced once per simulation tick. */
export declare class Tween {
    private readonly target;
    private steps;
    private index;
    private stepElapsed;
    private remainingRepeats;
    private forever;
    private running;
    constructor(target: NodeRef);
    to(duration: number, values: TweenValues, options?: {
        easing?: EasingName;
    }): this;
    by(duration: number, values: TweenValues, options?: {
        easing?: EasingName;
    }): this;
    delay(seconds: number): this;
    call(callback: () => void): this;
    /** Repeats the whole sequence this many additional times. */
    repeat(times?: number): this;
    repeatForever(): this;
    start(): this;
    stop(): this;
    private restart;
    private apply;
    /** @internal Advances this tween. Returns false when it has finished. */
    _advance(dt: number): boolean;
}
/** Creates a node tween for a script or component node. */
export declare function tween(target: NodeRef | Node): Tween;
/** @internal Advances every active tween once per simulation tick. */
export declare function advanceTweens(dt: number): void;
export declare function executionOrder(_order: number): ClassDecorator;
type PropertyToken = NumberConstructor | typeof Integer | BooleanConstructor | StringConstructor | typeof Text | typeof Vector2 | typeof Vector3 | typeof Size | typeof Rect | typeof Color | typeof Bitmask | typeof Image | typeof TextRef | typeof ImageRef | typeof NodeRef | typeof Prefab | typeof AudioClip | typeof SpriteFrame;
type PropertyOptions = {
    type?: PropertyToken;
    tooltip?: string;
    enum?: readonly string[];
    readonly?: boolean;
    min?: number;
    max?: number;
    step?: number;
    group?: string;
    slider?: boolean;
};
export declare function ccclass(target: Function): void;
export declare function ccclass(name?: string): ClassDecorator;
export declare function property(target: object, key: string | symbol): void;
export declare function property(options?: PropertyToken | PropertyOptions): PropertyDecorator;
