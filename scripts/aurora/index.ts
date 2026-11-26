//! Aurora’s TypeScript component and scene scripting API.

import { AssetBundle, AudioClip, LoadedAudioClip, LoadedSpriteFrame, Resources, SpriteFrame } from "./resources.ts";

export { AssetBundle, AudioClip, LoadedAudioClip, LoadedSpriteFrame, Resources, SpriteFrame, registerScriptResources } from "./resources.ts";
export type { AudioClipCallback, BundleAssets, BundleCallback, BundleProgressCallback, PrefabCallback, SpriteFrameCallback } from "./resources.ts";

import { EventCallback, EventPhase, EventTarget, Input, InputEventType, PointerPhase, ScriptEvent, input } from "./events.ts";
import { ScheduledCallback, Scheduler } from "./scheduler.ts";

export { EventPhase, EventTarget, Input, InputEventType, InputRouter, KeyCode, PointerEventType, ScriptEvent, input } from "./events.ts";
export type { EventCallback } from "./events.ts";
export type { PointerPhase } from "./events.ts";
export type { ScheduledCallback } from "./scheduler.ts";

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

export function registerComponent(entry: [string, ComponentMetadata]): [string, ComponentMetadata] {
  return entry;
}

export type ComponentConstructor<T extends Component> = abstract new () => T;

export class Node {
  x = 0;
  y = 0;
  changed = false;
  private sceneIndex = -1;
  private sceneCommands: SceneCommands | null = null;

  bindScene(index: number, commands: SceneCommands): void {
    this.sceneIndex = index;
    this.sceneCommands = commands;
  }

  get name(): string { return scriptScene.name(this.sceneIndex); }
  get parent(): NodeRef | null {
    const commands = this.sceneCommands;
    const parent = scriptScene.parent(this.sceneIndex);
    return commands === null || parent < 0 ? null : new NodeRef(parent, commands);
  }
  setActive(visible: boolean): void {
    if (this.sceneCommands === null) throw new Error("Node is not bound");
    this.sceneCommands.show(this.sceneIndex, visible);
  }
  setText(value: string): void {
    if (this.sceneCommands === null) throw new Error("Node is not bound");
    this.sceneCommands.setText(this.sceneIndex, value);
  }
  addComponent(type: string | ComponentConstructor<Component>): void {
    if (this.sceneCommands === null) throw new Error("Node is not bound");
    this.sceneCommands.addComponent(this.sceneIndex, type);
  }
  removeComponent(component: Component): void {
    if (this.sceneCommands === null) throw new Error("Node is not bound");
    this.sceneCommands.removeComponent(this.sceneIndex, component);
  }
  getComponent<T extends Component>(type: ComponentConstructor<T>): T | null;
  getComponent(name: string): Component | null;
  getComponent(type: string | ComponentConstructor<Component>): Component | null {
    return firstComponent(this.sceneIndex, type);
  }

  getComponents<T extends Component>(type: ComponentConstructor<T>): T[];
  getComponents(name: string): Component[];
  getComponents(type: string | ComponentConstructor<Component>): Component[] {
    return matchingComponents(this.sceneIndex, type);
  }

  on(type: string, callback: EventCallback, capture = false): EventCallback {
    return scriptScene.nodeEvents(this.sceneIndex).on(type, callback, capture);
  }

  once(type: string, callback: EventCallback, capture = false): EventCallback {
    return scriptScene.nodeEvents(this.sceneIndex).once(type, callback, capture);
  }

  off(type: string, callback?: EventCallback, capture?: boolean): void {
    scriptScene.nodeEvents(this.sceneIndex).off(type, callback, capture);
  }

  emit(type: string, value = 0): void {
    const commands = this.sceneCommands;
    if (commands !== null) {
      scriptScene.dispatchEvent(new NodeRef(this.sceneIndex, commands), commands, new ScriptEvent(type, value), false);
    }
  }

  dispatchEvent(event: ScriptEvent): void {
    const commands = this.sceneCommands;
    if (commands !== null) scriptScene.dispatchEvent(new NodeRef(this.sceneIndex, commands), commands, event, true);
  }
  /** Returns a script reference for this node when it is bound to a scene. */
  nodeRef(): NodeRef | null {
    return this.sceneCommands === null ? null : new NodeRef(this.sceneIndex, this.sceneCommands);
  }

  get children(): NodeRef[] {
    const commands = this.sceneCommands;
    return commands === null
      ? []
      : scriptScene.children(this.sceneIndex).map((index) => new NodeRef(index, commands));
  }
  get childCount(): number { return scriptScene.children(this.sceneIndex).length; }
  removeFromParent(): void {
    if (this.sceneCommands === null) throw new Error("Node is not bound");
    this.sceneCommands.reparent(this.sceneIndex, null);
  }
  getChildByName(name: string): NodeRef | null {
    const commands = this.sceneCommands;
    return commands === null ? null : scriptScene.child(this.sceneIndex, name, commands);
  }

  getChildByPath(path: string): NodeRef | null {
    const commands = this.sceneCommands;
    return commands === null ? null : scriptScene.childPath(this.sceneIndex, path, commands);
  }

  setPosition(x: number, y: number): void {
    if (this.sceneCommands !== null && callbackSceneCommands !== null
        && this.sceneCommands !== callbackSceneCommands) {
      this.sceneCommands.move(this.sceneIndex, x, y);
      return;
    }
    this.x = x;
    this.y = y;
    this.changed = true;
  }

  /** Local Z position of a spatial node, or zero for a 2D node. */
  get z(): number { return scriptScene.nodeZ(this.sceneIndex); }
  /** World-space translation of a spatial node. */
  get worldPosition(): [number, number, number] { return scriptScene.nodeWorld(this.sceneIndex, "position"); }
  /** World-space Euler rotation in degrees. */
  get worldRotation(): [number, number, number] { return scriptScene.nodeWorld(this.sceneIndex, "rotation"); }
  /** @internal Updates the cached Z position from a host transform stream. */
  set z(value: number) {
    const position = scriptScene.nodeSpatial(this.sceneIndex, "position");
    scriptScene.setSpatial(this.sceneIndex, "position", [position[0], position[1], value]);
  }
  get scaleX(): number { return scriptScene.nodeScaleX(this.sceneIndex); }
  set scaleX(value: number) { this.setScale(value, this.scaleY); }
  get scaleY(): number { return scriptScene.nodeScaleY(this.sceneIndex); }
  set scaleY(value: number) { this.setScale(this.scaleX, value); }
  /** Local rotation in degrees. */
  get angle(): number { return scriptScene.nodeAngle(this.sceneIndex) * degreesPerRadian; }
  set angle(value: number) { this.setAngle(value); }

  setScale(x: number, y: number): void {
    if (this.sceneCommands === null) throw new Error("Node is not bound");
    scriptScene.scaleNode(this.sceneIndex, x, y);
    this.sceneCommands.scale(this.sceneIndex, x, y);
  }

  /** Sets the local rotation. The value is in degrees. */
  setAngle(angle: number): void {
    if (this.sceneCommands === null) throw new Error("Node is not bound");
    const radians = angle / degreesPerRadian;
    scriptScene.rotateNode(this.sceneIndex, radians);
    this.sceneCommands.rotate(this.sceneIndex, radians);
  }
}

export class NodeHandle {
  readonly low: number;
  readonly high: number;
  constructor(low: number, high: number) {
    this.low = low;
    this.high = high;
  }
}

export class RigidBody2D {
  changed = false;
  vx = 0;
  vy = 0;

  setLinearVelocity(x: number, y: number): void {
    this.vx = x;
    this.vy = y;
    this.changed = true;
  }
}

/** Script handle for an authored spatial cuboid body. */
export class RigidBody3D {
  changed = false;
  angularChanged = false;
  vx = 0;
  vy = 0;
  vz = 0;
  avx = 0;
  avy = 0;
  avz = 0;

  setLinearVelocity(x: number, y: number, z: number): void {
    this.vx = x;
    this.vy = y;
    this.vz = z;
    this.changed = true;
  }

  /** Sets angular velocity in degrees per second. */
  setAngularVelocity(x: number, y: number, z: number): void {
    this.avx = x;
    this.avy = y;
    this.avz = z;
    this.angularChanged = true;
  }
}

export class Collider2D {
  readonly node: NodeHandle;
  constructor(node: NodeHandle) {
    this.node = node;
  }
}

/** Peer handle delivered with a spatial contact. */
export class Collider3D {
  readonly node: NodeHandle;
  /** True when either collider in the pair is a trigger. */
  readonly trigger: boolean;
  constructor(node: NodeHandle, trigger = false) {
    this.node = node;
    this.trigger = trigger;
  }
}

/** Result of a world-space raycast. */
export class RaycastHit3D {
  constructor(
    readonly distance: number,
    readonly normal: [number, number, number],
    readonly node: NodeHandle | null,
  ) {}
}

export class AudioSource {
  pending = -1;
  pendingHandle = -1;

  playClip(clip: AudioClip): void {
    const commands = callbackSceneCommands;
    const handle = (clip as LoadedAudioClip).handle;
    if (typeof handle === "number" && handle >= 0x80000000) {
      if (commands === null) this.pendingHandle = handle;
      else commands.sound(handle);
      return;
    }
    if (commands === null) this.playOneShot(clip.index);
    else commands.sound(clip.index);
  }

  playOneShot(index: number): void {
    if (!Number.isInteger(index) || index < 0 || index > 0xffffffff) {
      throw new Error("Invalid audio clip index");
    }
    this.pending = index;
  }
}

let scriptSceneNames: string[] = [];

/** @internal Registers the authored scene order before script instances are constructed. */
export function registerScriptScenes(names: string[]): void {
  scriptSceneNames = names;
}

export class Director {
  action = 0;
  scene = 0;
  camera = 0xffffffff;

  pause(): void { this.action = 1; }
  resume(): void { this.action = 2; }
  showLoading(): void { this.action = 4; }
  hideLoading(): void { this.action = 5; }
  /** Renders the scene through a camera node, like a scripted loading screen view. */
  setCamera(node: number | NodeRef): void {
    const reference = typeof node === "number" ? node : node.index;
    if (!Number.isInteger(reference) || reference < 0 || reference > 0xfffffffe) {
      throw new Error("Invalid camera node");
    }
    this.camera = reference;
    this.action = 6;
  }
  /** Returns to the camera selected in the scene. */
  useSceneCamera(): void {
    this.camera = 0xffffffff;
    this.action = 6;
  }
  loadScene(name: string): boolean {
    for (let index = 0; index < scriptSceneNames.length; index++) {
      if (scriptSceneNames[index] === name) {
        this.changeScene(index);
        return true;
      }
    }
    return false;
  }

  changeScene(index: number): void {
    if (!Number.isInteger(index) || index < 0 || index > 0xffffffff) {
      throw new Error("Invalid scene index");
    }
    this.scene = index;
    this.action = 3;
  }
}

type AssetLoadType = 1 | 2 | 3;
type AssetLoadCallback = (error: Error | null, asset: unknown) => void;

/** @internal Backward-compatible alias for the sprite resolver. */
export function resolveSpriteLoad(requestId: number, handle: number, failed: boolean): void {
  resolveAssetLoad(requestId, handle, failed, 1);
}

interface AssetLoad {
  path: string;
  type: AssetLoadType;
  callback: AssetLoadCallback;
}

const pendingAssetLoads = new Map<number, AssetLoad>();
let nextAssetRequest = 1;

/** @internal Resolves one on-demand asset import delivered by the host. */
export function resolveAssetLoad(
  requestId: number,
  handle: number,
  failed: boolean,
  type: AssetLoadType,
): void {
  const request = pendingAssetLoads.get(requestId);
  if (request === undefined) return;
  pendingAssetLoads.delete(requestId);
  const error = failed ? new Error(`Asset import failed: ${request.path}`) : null;
  let asset: SpriteFrame | AudioClip | Prefab | null = null;
  if (!failed && type === 1) asset = new LoadedSpriteFrame(request.path, handle);
  else if (!failed && type === 2) asset = new LoadedAudioClip(request.path, handle);
  else if (!failed && type === 3) asset = new LoadedPrefab(request.path, handle);
  request.callback(error, asset);
}

/** @internal Drops unresolved asset imports during teardown. */
export function clearAssetLoads(): void {
  pendingAssetLoads.clear();
}

/** @internal Delivers one overlap hit to the owning component. */
export function dispatchOverlapHit(request: number, reference: number, last: boolean): void {
  const owner = callbackOwner;
  if (owner === null || !owner._takeOverlap(request)) return;
  owner.onOverlapHit(reference === 0xffffffff ? null : new NodeHandle(reference, 0), last);
}

/** @internal Caches one authored world transform stream. */
export function setScriptWorldTransform(
  reference: number,
  field: "position" | "rotation",
  value: [number, number, number],
): void {
  scriptScene.setWorld(reference, field, value);
}

/** @internal Delivers one raycast result to the owning callback. */
export function dispatchRaycastResult(
  distance: number,
  missed: boolean,
  normal: [number, number, number],
  request: number,
): void {
  const owner = callbackOwner;
  if (owner === null) return;
  const callback = owner._takeRaycast(request);
  if (callback !== null) {
    callback(missed ? null : new RaycastHit3D(distance, normal, null));
  }
}

export class SceneEdit {
  kind: number;
  target: number;
  x: number;
  y: number;

  constructor(kind: number, target: number, x: number, y: number) {
    this.kind = kind;
    this.target = target;
    this.x = x;
    this.y = y;
  }
}

const degreesPerRadian = 180 / Math.PI;

const firstDynamicNode = 0x80000000;
let nextDynamicNode = firstDynamicNode;
let callbackSceneCommands: SceneCommands | null = null;
let callbackOwner: Component | null = null;
let nextScriptTimerId = 1;
const scriptTimerOwners = new Map<number, Component>();

export class SceneCommands {
  edits: SceneEdit[] = [];
  cursor = 0;
  private prefabs = new Map<string, Prefab>();

  clear(): void {
    this.edits = [];
    this.cursor = 0;
  }

  private push(kind: number, target: number, x: number, y: number): void {
    const buffer = callbackSceneCommands ?? this;
    if (!Number.isInteger(target) || target < 0 || target > 0xffffffff || buffer.edits.length >= 128) {
      throw new Error("Invalid scene command");
    }
    buffer.edits.push(new SceneEdit(kind, target, x, y));
  }

  move(node: number, x: number, y: number): void { this.push(0, node, x, y); }
  scale(node: number, x: number, y: number): void { this.push(11, node, x, y); }
  rotate(node: number, angle: number): void { this.push(12, node, angle, 0); }
  show(node: number, visible: boolean): void { this.push(1, node, visible ? 1 : 0, 0); }
  frame(node: number, frame: number): void { this.push(2, node, frame, 0); }

  setSpriteFrame(node: NodeRef, frame: SpriteFrame): void {
    const slot = frame.frameFor(node.index);
    if (slot === -1) throw new Error("Sprite frame is not registered for this node");
    this.frame(node.index, slot);
  }

  private queueChunks(
    kinds: [number, number, number],
    target: number,
    bytes: Uint8Array,
    label: string,
  ): void {
    const buffer = callbackSceneCommands ?? this;
    const edits = Math.ceil(bytes.length / 12) + 2;
    if (buffer.edits.length + edits > 128) {
      throw new Error(`${label} exceeds the scene command limit`);
    }
    this.push(kinds[0], target, 0, 0);
    for (let offset = 0; offset < bytes.length; offset += 12) {
      const chunk = bytes.subarray(offset, offset + 12);
      const packed = (start: number): number => {
        let value = 0;
        for (let index = 0; index < 6; index++) {
          value += (chunk[start + index] ?? 0) * 2 ** (index * 8);
        }
        return value;
      };
      this.push(kinds[1], target, packed(0), packed(6) + chunk.length * 2 ** 48);
    }
    this.push(kinds[2], target, 0, 0);
  }

  /** @internal Allocates a new request identity shared by all host requests. */
  requestId(): number {
    const request = nextAssetRequest++;
    if (request > 0xffffffff) throw new Error("Asset request limit reached");
    return request;
  }

  /** Requests an on-demand PNG import. The callback runs on a later resource phase. */
  loadSprite(path: string, callback: (error: Error | null, frame: SpriteFrame | null) => void): void {
    const bytes = new TextEncoder().encode(path);
    if (bytes.length < 1 || bytes.length > 4096) throw new Error("Sprite path exceeds its size limit");
    const request = this.requestId();
    pendingAssetLoads.set(request, {
      path,
      type: 1,
      callback: (error, asset) => callback(error, asset as SpriteFrame | null),
    });
    this.queueChunks([19, 20, 21], request, bytes, "Sprite import");
  }

  /** Requests an on-demand audio import. */
  loadAudio(path: string, callback: (error: Error | null, clip: AudioClip | null) => void): void {
    const bytes = new TextEncoder().encode(path);
    if (bytes.length < 1 || bytes.length > 4096) throw new Error("Audio path exceeds its size limit");
    const request = this.requestId();
    pendingAssetLoads.set(request, {
      path,
      type: 2,
      callback: (error, asset) => callback(error, asset as AudioClip | null),
    });
    this.queueChunks([23, 24, 25], request, bytes, "Audio import");
  }

  /** Requests an on-demand prefab import. */
  loadPrefabFile(path: string, callback: (error: Error | null, prefab: Prefab | null) => void): void {
    const bytes = new TextEncoder().encode(path);
    if (bytes.length < 1 || bytes.length > 4096) throw new Error("Prefab path exceeds its size limit");
    const request = this.requestId();
    pendingAssetLoads.set(request, {
      path,
      type: 3,
      callback: (error, asset) => callback(error, asset as Prefab | null),
    });
    this.queueChunks([26, 27, 28], request, bytes, "Prefab import");
  }

  /** Requests a world-space raycast; the result arrives as a component event. */
  raycast(origin: [number, number, number], direction: [number, number, number], distance: number): void {
    this.raycastWith(this.requestId(), origin, direction, distance);
  }

  /** @internal Sends a raycast or shape cast with a preallocated request identity. */
  raycastWith(
    request: number,
    origin: [number, number, number],
    direction: [number, number, number],
    distance: number,
    halfExtents: [number, number, number] = [0, 0, 0],
  ): void {
    const bytes = new Uint8Array(44);
    const view = new DataView(bytes.buffer);
    const values = [
      halfExtents[0] > 0 ? 2 : 1,
      ...origin,
      ...direction,
      distance,
      ...halfExtents,
    ];
    for (let index = 0; index < values.length; index++) {
      view.setFloat32(index * 4, values[index], true);
    }
    this.queueChunks([29, 30, 31], request, bytes, "Spatial query");
  }

  /** Requests a box overlap query; hits arrive as component events. */
  overlap(
    halfExtents: [number, number, number],
    position: [number, number, number],
  ): void {
    this.overlapWith(this.requestId(), halfExtents, position);
  }

  /** @internal Sends an overlap query with a preallocated request identity. */
  overlapWith(
    request: number,
    halfExtents: [number, number, number],
    position: [number, number, number],
  ): void {
    const bytes = new Uint8Array(24);
    const view = new DataView(bytes.buffer);
    const values = [...halfExtents, ...position];
    for (let index = 0; index < values.length; index++) {
      view.setFloat32(index * 4, values[index], true);
    }
    this.queueChunks([32, 33, 34], request, bytes, "Overlap query");
  }

  /** Releases one on-demand asset handle. */
  releaseAsset(handle: number): void {
    this.push(22, handle, 0, 0);
  }
  setText(node: number, value: string): void {
    if (typeof value !== "string") throw new Error("Text content must be a string");
    const bytes = new TextEncoder().encode(value);
    if (bytes.length > 1512) throw new Error("Text content exceeds the 1512-byte limit");
    const buffer = callbackSceneCommands ?? this;
    const edits = Math.ceil(bytes.length / 12) + 2;
    if (buffer.edits.length + edits > 128) {
      throw new Error("Text replacement exceeds the scene command limit");
    }
    scriptScene.assertTextNode(node);
    this.push(13, node, 0, 0);
    for (let offset = 0; offset < bytes.length; offset += 12) {
      const chunk = bytes.subarray(offset, offset + 12);
      const packed = (start: number): number => {
        let value = 0;
        for (let index = 0; index < 6; index++) {
          value += (chunk[start + index] ?? 0) * 2 ** (index * 8);
        }
        return value;
      };
      const packedY = packed(6) + chunk.length * 2 ** 48;
      this.push(14, node, packed(0), packedY);
    }
    this.push(15, node, 0, 0);
    scriptScene.setNodeText(node, value);
  }
  /** Persists one project config value as `name\0value` UTF-8 bytes. */
  setConfig(name: string, value: string): void {
    if (typeof name !== "string" || typeof value !== "string") throw new Error("Config requires strings");
    const bytes = new TextEncoder().encode(`${name}\0${value}`);
    if (bytes.length > 1512) throw new Error("Config entry exceeds the 1512-byte limit");
    const buffer = callbackSceneCommands ?? this;
    const edits = Math.ceil(bytes.length / 12) + 2;
    if (buffer.edits.length + edits > 128) {
      throw new Error("Config update exceeds the scene command limit");
    }
    this.push(16, 0, 0, 0);
    for (let offset = 0; offset < bytes.length; offset += 12) {
      const chunk = bytes.subarray(offset, offset + 12);
      const packed = (start: number): number => {
        let value = 0;
        for (let index = 0; index < 6; index++) {
          value += (chunk[start + index] ?? 0) * 2 ** (index * 8);
        }
        return value;
      };
      this.push(17, 0, packed(0), packed(6) + chunk.length * 2 ** 48);
    }
    this.push(18, 0, 0, 0);
    config.set(name, value);
  }
  sound(clip: number): void { this.push(3, clip, 0, 0); }
  bestScore(score: number): void { this.push(4, score, 0, 0); }

  addComponent(node: number, type: string | ComponentConstructor<Component>): void {
    const factory = scriptScene.factoryId(componentName(type));
    if (factory < 0) throw new Error("Unknown component factory");
    this.push(9, node, factory, 0);
  }

  removeComponent(node: number, component: Component): void {
    const binding = scriptScene.bindingFor(node, component);
    if (binding < 0) throw new Error("Component cannot be removed from this node");
    // Authored bindings stay in the launch list, so hide them from lookups as
    // soon as removal is requested. The player stops dispatching after teardown.
    if (scriptScene.authoredBinding(binding)) scriptScene.markRemoved(binding);
    this.push(10, binding, node, 0);
  }

  registerPrefab(path: string, assetIndex: number): void {
    if (path.length === 0 || !Number.isInteger(assetIndex) || assetIndex < 0 || assetIndex > 0xffffffff) {
      throw new Error("Invalid prefab registration");
    }
    this.prefabs.set(path, new Prefab(path, assetIndex));
  }

  loadPrefab(path: string): Prefab | null {
    return this.prefabs.get(path) ?? scriptScene.prefab(path);
  }

  createNode(parent?: NodeRef): NodeRef {
    return this.create(5, parent, 0);
  }

  instantiate(prefab: Prefab, parent?: NodeRef): NodeRef {
    const handle = (prefab as LoadedPrefab).handle;
    const asset = typeof handle === "number" && handle >= 0x80000000
      ? handle
      : prefab.assetIndex;
    if (asset < 0) throw new Error("Prefab has no runtime asset index");
    return this.create(6, parent, asset);
  }

  private create(kind: number, parent: NodeRef | undefined, asset: number): NodeRef {
    const index = nextDynamicNode;
    if (index >= 0xc0000000) throw new Error("Script root reference range exhausted");
    this.push(kind, index, parent?.index ?? -1, asset);
    nextDynamicNode += 1;
    return new NodeRef(index, this);
  }

  reparent(node: number, parent: NodeRef | null): void {
    if (node < firstDynamicNode) throw new Error("Only script-created nodes can be reparented");
    this.push(7, node, parent?.index ?? -1, 0);
  }

  destroy(node: number): void {
    if (node < firstDynamicNode) throw new Error("Only script-created nodes can be destroyed");
    this.push(8, node, 0, 0);
  }
}

/**
 * Scene-wide asset manager for code that is not tied to one component.
 *
 * Handles resolve from the same catalog as `Component.resources`. Bundle-wide
 * preload and load callbacks share the fixed resource phase, and `release`
 * drops the bundle's script handles without touching scene-owned asset data.
 */
export class AssetManager {
  private readonly resources = new Resources(new SceneCommands());
  private readonly bundles = new Map<string, AssetBundle>();

  /** Returns a cached directory-scoped view of the current scene catalog. */
  bundle(root = ""): AssetBundle {
    const key = root.split("\\").join("/").replace(/^\.\/+|\/+$/g, "");
    const cached = this.bundles.get(key);
    if (cached !== undefined) return cached;
    const bundle = new AssetBundle(this.resources, key);
    this.bundles.set(key, bundle);
    return bundle;
  }

  /** Resolves every asset in a bundle and calls back with the bundle. */
  loadBundle(root: string, callback: (error: Error | null, bundle: AssetBundle | null) => void): AssetBundle {
    const bundle = this.bundle(root);
    bundle.preloadAll((error) => callback(error, error === null ? bundle : null));
    return bundle;
  }

  preloadBundle(root: string, callback: (error: Error | null, bundle: AssetBundle | null) => void): AssetBundle {
    return this.loadBundle(root, callback);
  }

  releaseBundle(root: string): void {
    const key = root.split("\\").join("/").replace(/^\.\/+|\/+$/g, "");
    const bundle = this.bundles.get(key);
    if (bundle === undefined) return;
    bundle.release();
    this.bundles.delete(key);
  }

  getAudioClip(path: string): AudioClip | null { return this.resources.getAudioClip(path); }
  getSpriteFrame(path: string): SpriteFrame | null { return this.resources.getSpriteFrame(path); }
  getPrefab(path: string): Prefab | null { return this.resources.getPrefab(path); }

  requireAudioClip(path: string): AudioClip { return this.resources.requireAudioClip(path); }
  requireSpriteFrame(path: string): SpriteFrame { return this.resources.requireSpriteFrame(path); }

  loadAudioClip(path: string, callback: (error: Error | null, clip: AudioClip | null) => void): void {
    this.resources.loadAudioClip(path, callback);
  }

  loadSpriteFrame(path: string, callback: (error: Error | null, frame: SpriteFrame | null) => void): void {
    this.resources.loadSpriteFrame(path, callback);
  }

  loadSpriteFile(path: string, callback: (error: Error | null, frame: SpriteFrame | null) => void): void {
    this.resources.loadSpriteFile(path, callback);
  }

  loadAudioFile(path: string, callback: (error: Error | null, clip: AudioClip | null) => void): void {
    this.resources.loadAudioFile(path, callback);
  }

  loadPrefabFile(path: string, callback: (error: Error | null, prefab: Prefab | null) => void): void {
    this.resources.loadPrefabFile(path, callback);
  }

  /** Releases any on-demand asset handle. */
  releaseAssetHandle(handle: number): void {
    this.resources.releaseAssetHandle(handle);
  }

  loadPrefab(path: string, callback: (error: Error | null, prefab: Prefab | null) => void): void {
    this.resources.loadPrefab(path, callback);
  }

  preloadAudioClip(path: string, callback: (error: Error | null, clip: AudioClip | null) => void): void {
    this.resources.loadAudioClip(path, callback);
  }

  preloadSpriteFrame(path: string, callback: (error: Error | null, frame: SpriteFrame | null) => void): void {
    this.resources.loadSpriteFrame(path, callback);
  }

  preloadPrefab(path: string, callback: (error: Error | null, prefab: Prefab | null) => void): void {
    this.resources.loadPrefab(path, callback);
  }

  releaseAudioClip(path: string): void { this.resources.releaseAudioClip(path); }
  releaseSpriteFrame(path: string): void { this.resources.releaseSpriteFrame(path); }
  releaseAll(): void { this.resources.releaseAll(); this.bundles.clear(); }
  cancelLoads(): void { this.resources.cancelLoads(); }
  get pendingCount(): number { return this.resources.pendingCount; }

  /** @internal Advances one global asset callback per simulation tick. */
  _advance(): void { this.resources._advance(); }

  /** @internal Drops manager handles when a scene starts. */
  _reset(): void { this.releaseAll(); }
}

/** Global asset manager for bundle and non-component resource work. */
export const assetManager = new AssetManager();

/** Host-pushed key/value state. Values arrive once per scene start. */
class KeyValueStore {
  private readonly values = new Map<string, string>();
  private readonly pending = new Map<number, number[]>();

  /** Returns the stored value or null when the host did not supply the key. */
  get(name: string): string | null { return this.values.get(name) ?? null; }

  /** @internal Stores one config value locally. */
  set(name: string, value: string): void { this.values.set(name, value); }

  /** @internal Clears cached values before a new scene's configuration arrives. */
  reset(): void { this.values.clear(); this.pending.clear(); }

  /** @internal Accumulates one `name\0value` chunk stream from the host. */
  chunk(operation: number, entry: number, bytes: number[]): void {
    if (operation === 0) {
      this.pending.set(entry, []);
      return;
    }
    const chunks = this.pending.get(entry);
    if (chunks === undefined) throw new Error("Config chunk has no matching start");
    if (operation >= 1 && operation <= 12) {
      for (const byte of bytes) chunks.push(byte);
      return;
    }
    if (operation !== 13) throw new Error("Invalid config chunk operation");
    this.pending.delete(entry);
    const separator = chunks.indexOf(0);
    if (separator < 0) throw new Error("Config entry is missing its name separator");
    const decoder = new TextDecoder();
    const name = decoder.decode(new Uint8Array(chunks.slice(0, separator)));
    const value = decoder.decode(new Uint8Array(chunks.slice(separator + 1)));
    this.values.set(name, value);
  }
}

/** Project-scoped config values supplied by the host. Writes persist through the host. */
export const config = new KeyValueStore();

/** Host-only secret values supplied by the host. Secrets cannot be written from scripts. */
export const secrets = new KeyValueStore();

export class NodeRef {
  constructor(readonly index: number, private readonly commands: SceneCommands) {}

  get name(): string { return scriptScene.name(this.index); }
  get x(): number { return scriptScene.nodeX(this.index); }
  get y(): number { return scriptScene.nodeY(this.index); }
  /** Local Z position of a spatial node, or zero for a 2D node. */
  get z(): number { return scriptScene.nodeZ(this.index); }
  /** World-space translation of a spatial node. */
  get worldPosition(): [number, number, number] { return scriptScene.nodeWorld(this.index, "position"); }
  /** World-space Euler rotation in degrees. */
  get worldRotation(): [number, number, number] { return scriptScene.nodeWorld(this.index, "rotation"); }
  get scaleX(): number { return scriptScene.nodeScaleX(this.index); }
  set scaleX(value: number) { this.setScale(value, this.scaleY); }
  get scaleY(): number { return scriptScene.nodeScaleY(this.index); }
  set scaleY(value: number) { this.setScale(this.scaleX, value); }
  /** Local rotation in degrees. */
  get angle(): number { return scriptScene.nodeAngle(this.index) * degreesPerRadian; }
  set angle(value: number) { this.setAngle(value); }
  setScale(x: number, y: number): void {
    scriptScene.scaleNode(this.index, x, y);
    this.commands.scale(this.index, x, y);
  }
  /** Sets the local rotation. The value is in degrees. */
  setAngle(angle: number): void {
    const radians = angle / degreesPerRadian;
    scriptScene.rotateNode(this.index, radians);
    this.commands.rotate(this.index, radians);
  }
  get parent(): NodeRef | null {
    const parent = scriptScene.parent(this.index);
    return parent < 0 ? null : new NodeRef(parent, this.commands);
  }
  addComponent(type: string | ComponentConstructor<Component>): void {
    this.commands.addComponent(this.index, type);
  }
  removeComponent(component: Component): void { this.commands.removeComponent(this.index, component); }
  getComponent<T extends Component>(type: ComponentConstructor<T>): T | null;
  getComponent(name: string): Component | null;
  getComponent(type: string | ComponentConstructor<Component>): Component | null {
    return firstComponent(this.index, type);
  }

  getComponents<T extends Component>(type: ComponentConstructor<T>): T[];
  getComponents(name: string): Component[];
  getComponents(type: string | ComponentConstructor<Component>): Component[] {
    return matchingComponents(this.index, type);
  }

  on(type: string, callback: EventCallback, capture = false): EventCallback {
    return scriptScene.nodeEvents(this.index).on(type, callback, capture);
  }

  once(type: string, callback: EventCallback, capture = false): EventCallback {
    return scriptScene.nodeEvents(this.index).once(type, callback, capture);
  }

  off(type: string, callback?: EventCallback, capture?: boolean): void {
    scriptScene.nodeEvents(this.index).off(type, callback, capture);
  }

  emit(type: string, value = 0): void {
    scriptScene.dispatchEvent(this, this.commands, new ScriptEvent(type, value), false);
  }

  dispatchEvent(event: ScriptEvent): void {
    scriptScene.dispatchEvent(this, this.commands, event, true);
  }
  get children(): NodeRef[] {
    return scriptScene.children(this.index).map((index) => new NodeRef(index, this.commands));
  }
  get childCount(): number { return scriptScene.children(this.index).length; }
  getChildByName(name: string): NodeRef | null { return scriptScene.child(this.index, name, this.commands); }
  getChildByPath(path: string): NodeRef | null { return scriptScene.childPath(this.index, path, this.commands); }
  setPosition(x: number, y: number): void {
    scriptScene.moveNode(this.index, x, y);
    this.commands.move(this.index, x, y);
  }
  setActive(visible: boolean): void { this.commands.show(this.index, visible); }
  setText(value: string): void { this.commands.setText(this.index, value); }
  setSpriteFrame(frame: SpriteFrame): void { this.commands.setSpriteFrame(this, frame); }
  setParent(parent: NodeRef | null): void { this.commands.reparent(this.index, parent); }
  removeFromParent(): void { this.commands.reparent(this.index, null); }
  destroy(): void { this.commands.destroy(this.index); }
}

export class Prefab {
  constructor(readonly path: string, readonly assetIndex = -1) {}
}

/** A prefab imported from disk on demand rather than the scene catalog. */
export class LoadedPrefab extends Prefab {
  readonly handle: number;

  constructor(path: string, handle: number) {
    super(path, -1);
    this.handle = handle;
  }
}

export class TextRef extends NodeRef {
  get content(): string { return scriptScene.nodeText(this.index); }
  set content(value: string) { this.setText(value); }
}

export class ImageRef extends NodeRef {}

export const Ref = {
  Node: NodeRef,
  Text: TextRef,
  Image: ImageRef,
  Prefab,
  AudioClip,
  SpriteFrame,
} as const;

// A pool of authored prefab instance indices. Returning a slot hides its subtree.
export class NodePool {
  private available: number[] = [];
  private checked: number[] = [];

  constructor(slots: number[]) {
    for (const slot of slots) {
      if (this.available.includes(slot)) throw new Error("Duplicate prefab pool slot");
      this.available.push(slot);
    }
  }

  get(): number {
    const slot = this.available.pop();
    if (slot === undefined) return -1;
    this.checked.push(slot);
    return slot;
  }

  reset(scene: SceneCommands, slot: number, x: number, y: number): void {
    if (!this.checked.includes(slot)) throw new Error("Prefab slot is not checked out");
    scene.move(slot, x, y);
    scene.show(slot, true);
  }

  put(scene: SceneCommands, slot: number): void {
    const index = this.checked.indexOf(slot);
    if (index < 0) throw new Error("Prefab slot is not checked out");
    this.checked.splice(index, 1);
    this.available.push(slot);
    scene.show(slot, false);
  }
}

export class Integer {}
export class Text {}
export class Vector2 {
  x = 0;
  y = 0;
  constructor(x = 0, y = 0) {
    this.x = x;
    this.y = y;
  }
}
export class Vector3 {
  x = 0;
  y = 0;
  z = 0;
  constructor(x = 0, y = 0, z = 0) {
    this.x = x;
    this.y = y;
    this.z = z;
  }
}
export class Size {}
export class Rect {}
export class Color {}
export class Bitmask {}
export class Image {}
export class Component {
  readonly node = new Node();
  readonly audio = new AudioSource();
  readonly body = new RigidBody2D();
  readonly body3d = new RigidBody3D();
  readonly director = new Director();
  readonly scene = new SceneCommands();
  readonly resources = new Resources(this.scene);
  /** Subscribe to keyboard or touch events delivered to this component. */
  readonly input = new Input();
  /** Custom events scoped to this component. */
  readonly events = new EventTarget();
  private readonly scheduler = new Scheduler();
  private readonly scriptTimers = new Map<number, ScheduledCallback>();
  private scriptTime = 0;

  schedule(callback: ScheduledCallback, interval = 0, repeat = Infinity, delay = 0): void {
    this.scheduler.schedule(callback, interval, repeat, delay);
  }

  scheduleOnce(callback: ScheduledCallback, delay = 0): void {
    this.scheduler.schedule(callback, 0, 0, delay);
  }

  unschedule(callback: ScheduledCallback): void {
    this.scheduler.unschedule(callback);
  }

  unscheduleAllCallbacks(): void {
    this.scheduler.clear();
    for (const id of this.scriptTimers.keys()) scriptTimerOwners.delete(id);
    this.scriptTimers.clear();
  }

  /** @internal Called by the wire scheduler phase. */
  _tickScheduler(dt: number): void {
    this.scriptTime += dt;
    this.scheduler.tick(dt);
  }

  /** @internal Schedules a global timer against simulation time. */
  _scheduleScriptTimer(callback: () => void, delay: number, interval: number | null): number {
    const id = allocateScriptTimer(this);
    const scheduled: ScheduledCallback = () => {
      if (interval === null) {
        this.scriptTimers.delete(id);
        scriptTimerOwners.delete(id);
      }
      callback();
    };
    this.scriptTimers.set(id, scheduled);
    this.scheduler.schedule(scheduled, interval ?? 0, interval === null ? 0 : Infinity, delay);
    return id;
  }

  /** @internal Cancels a global timer owned by this component. */
  _clearScriptTimer(id: number): void {
    const scheduled = this.scriptTimers.get(id);
    if (scheduled === undefined) return;
    this.scheduler.unschedule(scheduled);
    this.scriptTimers.delete(id);
    scriptTimerOwners.delete(id);
  }

  /** @internal Returns simulation time for requestAnimationFrame callbacks. */
  _scriptTimeMilliseconds(): number {
    return this.scriptTime * 1000;
  }

  /** @internal Delivers a resource completion in its own callback transaction. */
  _tickResources(): void {
    this.resources._advance();
    assetManager._advance();
  }

  /** @internal Routes one player key notification. */
  _dispatchKeyDown(key: number): void {
    this.input.emit(Input.EventType.KEY_DOWN, key);
    this.onKeyDown(key);
  }

  /** @internal Routes one player key release. */
  _dispatchKeyUp(key: number): void {
    this.input.emit(Input.EventType.KEY_UP, key);
    this.onKeyUp(key);
  }

  /** @internal Routes one player touch notification. */
  _dispatchTouchStart(x = 0, y = 0, pointerId = 0): void {
    this.input.dispatch(new ScriptEvent(Input.EventType.TOUCH_START, 0, false, x, y, pointerId), false);
    if (pointerId === 0) {
      this.input.dispatch(new ScriptEvent(Input.EventType.MOUSE_DOWN, 0, false, x, y, 0), false);
    }
    this.onTouchStart();
  }

  /** @internal Routes one mouse wheel notification. */
  _dispatchWheel(deltaX: number, deltaY: number): void {
    this.input.dispatch(
      new ScriptEvent(Input.EventType.MOUSE_WHEEL, 0, false, 0, 0, 0, deltaX, deltaY),
      false,
    );
  }

  /** @internal Routes a pointer phase to input subscriptions and legacy callbacks. */
  _dispatchTouch(phase: PointerPhase, x: number, y: number, pointerId: number): void {
    this.input.dispatch(new ScriptEvent(phase, 0, false, x, y, pointerId), false);
    if (pointerId === 0) {
      const mouse = phase === Input.EventType.TOUCH_START ? Input.EventType.MOUSE_DOWN
        : phase === Input.EventType.TOUCH_MOVE ? Input.EventType.MOUSE_MOVE
        : phase === Input.EventType.TOUCH_END ? Input.EventType.MOUSE_UP : Input.EventType.MOUSE_UP;
      this.input.dispatch(new ScriptEvent(mouse, 0, false, x, y, 0), false);
    }
    if (phase === Input.EventType.TOUCH_MOVE) this.onTouchMove(new ScriptEvent(phase, 0, false, x, y, pointerId));
    else if (phase === Input.EventType.TOUCH_END) this.onTouchEnd(new ScriptEvent(phase, 0, false, x, y, pointerId));
    else if (phase === Input.EventType.TOUCH_CANCEL) this.onTouchCancel(new ScriptEvent(phase, 0, false, x, y, pointerId));
  }

  /** @internal Releases listeners and timers on destruction. */
  _clearScriptServices(): void {
    this.unscheduleAllCallbacks();
    this.input.clear();
    this.events.clear();
    this.resources._clear();
  }

  getComponent<T extends Component>(type: ComponentConstructor<T>): T | null;
  getComponent(name: string): Component | null;
  getComponent(type: string | ComponentConstructor<Component>): Component | null {
    if (typeof type === "string") return this.node.getComponent(type);
    return this.node.getComponent(type);
  }

  getComponents<T extends Component>(type: ComponentConstructor<T>): T[];
  getComponents(name: string): Component[];
  getComponents(type: string | ComponentConstructor<Component>): Component[] {
    if (typeof type === "string") return this.node.getComponents(type);
    return this.node.getComponents(type);
  }

  enabled = true;
  private _active = false;

  get enabledInHierarchy(): boolean { return this.enabled && this._active; }

  _setLifecycleActive(active: boolean): void { this._active = active; }

  onLoad(): void {}
  onEnable(): void {}
  onDisable(): void {}
  onDestroy(): void {}
  start(): void { this.onStart(); }
  update(dt: number): void { this.onUpdate(dt); }
  fixedUpdate(dt: number): void { this.onFixedUpdate(dt); }
  lateUpdate(_dt: number): void {}
  onStart(): void {}
  onUpdate(_dt: number): void {}
  onFixedUpdate(_dt: number): void {}
  onKeyDown(_key: number): void {}
  onKeyUp(_key: number): void {}
  onTouchStart(): void {}
  onTouchMove(_event: ScriptEvent): void {}
  onTouchEnd(_event: ScriptEvent): void {}
  onTouchCancel(_event: ScriptEvent): void {}
  onBeginContact(_other: Collider2D): void {}
  onEndContact(_other: Collider2D): void {}
  onBeginContact3D(_other: Collider3D): void {}
  onEndContact3D(_other: Collider3D): void {}
  /** Requests a world-space raycast. The result arrives on a later tick. */
  raycast(
    origin: [number, number, number],
    direction: [number, number, number],
    distance: number,
    callback: (hit: RaycastHit3D | null) => void,
  ): void {
    const request = this.scene.requestId();
    this.raycastCallbacks.set(request, callback);
    this.scene.raycastWith(request, origin, direction, distance);
  }
  /** Requests a box shape cast. The result arrives as a raycast callback. */
  shapeCast(
    halfExtents: [number, number, number],
    origin: [number, number, number],
    direction: [number, number, number],
    distance: number,
    callback: (hit: RaycastHit3D | null) => void,
  ): void {
    const request = this.scene.requestId();
    this.raycastCallbacks.set(request, callback);
    this.scene.raycastWith(request, origin, direction, distance, halfExtents);
  }

  /** Requests a box overlap query. Each hit arrives through `onOverlapHit`. */
  overlap(
    halfExtents: [number, number, number],
    position: [number, number, number],
  ): void {
    const request = this.scene.requestId();
    this.overlapRequests.set(request, true);
    this.scene.overlapWith(request, halfExtents, position);
  }

  /** Called for each box overlap hit. `last` marks the final hit or an empty result. */
  onOverlapHit(_node: NodeHandle | null, _last: boolean): void {}

  /** @internal Consumes one overlap callback request. */
  _takeOverlap(request: number): boolean {
    return this.overlapRequests.get(request) ?? false;
  }
  private readonly overlapRequests = new Map<number, boolean>();

  private readonly raycastCallbacks = new Map<number, (hit: RaycastHit3D | null) => void>();
  /** @internal Consumes one raycast callback. */
  _takeRaycast(request: number): ((hit: RaycastHit3D | null) => void) | null {
    const callback = this.raycastCallbacks.get(request) ?? null;
    this.raycastCallbacks.delete(request);
    return callback;
  }
  onAnimationEvent(_code: number): void {}
  onBestScore(_score: number): void {}
}


type PrefabFactory = (references: number[]) => Component;
type PrefabBuilder = (references: number[]) => Component[];

class PrefabTemplate {
  constructor(
    readonly nodes: string[],
    readonly build: PrefabBuilder,
    readonly names: string[],
    readonly textContents: (string | null)[],
  ) {}
}

class DynamicComponent {
  constructor(readonly instance: Component, readonly owner: number, readonly name: string) {}
}

class ScriptScene {
  private readonly dynamicParents = new Map<number, number>();
  private readonly dynamicNames = new Map<number, string>();
  private readonly dynamicComponents = new Map<number, DynamicComponent>();
  private readonly clones = new Map<number, number[]>();
  private readonly cloneComponents = new Map<number, Component[]>();
  private readonly templates = new Map<number, PrefabTemplate>();
  private readonly componentFactories: PrefabFactory[] = [];
  private readonly componentFactoryNames: string[] = [];
  private readonly eventTargets = new Map<number, EventTarget>();
  private readonly removedBindings = new Set<number>();
  private nodes: string[] = [];
  private parents: number[] = [];
  private instances: Component[] = [];
  private owners: number[] = [];
  private names: string[] = [];
  private prefabs = new Map<string, Prefab>();
  private readonly textContents = new Map<number, string>();

  install(
    nodes: string[],
    parents: number[],
    instances: Component[],
    owners: number[],
    names: string[],
    prefabPaths: string[],
    textContents: (string | null)[] = [],
  ): void {
    this.nodes = nodes;
    this.parents = parents;
    this.instances = instances;
    this.owners = owners;
    this.names = names;
    this.textContents.clear();
    for (let index = 0; index < textContents.length; index++) {
      const text = textContents[index];
      if (text !== null && text !== undefined) this.textContents.set(index, text);
    }
    this.removedBindings.clear();
    this.prefabs.clear();
    for (let i = 0; i < prefabPaths.length; i++) {
      const path = prefabPaths[i];
      this.prefabs.set(path, new Prefab(path, i));
    }
    for (let i = 0; i < instances.length; i++) {
      instances[i].node.bindScene(owners[i], instances[i].scene);
    }
  }

  prefab(path: string): Prefab | null { return this.prefabs.get(path) ?? null; }

  parent(index: number): number {
    if (index < 0) return -1;
    if (index < this.parents.length) return this.parents[index];
    return this.dynamicParents.get(index) ?? -1;
  }

  nodeEvents(index: number): EventTarget {
    const existing = this.eventTargets.get(index);
    if (existing !== undefined) return existing;
    const events = new EventTarget();
    this.eventTargets.set(index, events);
    return events;
  }

  clearNodeEvents(index: number): void {
    const events = this.eventTargets.get(index);
    if (events !== undefined) events.clear();
    this.eventTargets.delete(index);
  }

  clearEventTargets(): void {
    for (const events of this.eventTargets.values()) events.clear();
    this.eventTargets.clear();
  }

  /** Discards scene-local state before a scene's bindings load. */
  resetScene(): void {
    this.clearEventTargets();
    this.dynamicParents.clear();
    this.dynamicNames.clear();
    this.dynamicComponents.clear();
    this.clones.clear();
    this.cloneComponents.clear();
    this.removedBindings.clear();
    this.positionX.clear();
    this.positionY.clear();
    this.scaleX.clear();
    this.scaleY.clear();
    this.angles.clear();
    this.spatialPosition.clear();
    this.spatialRotation.clear();
    this.spatialScale.clear();
    this.worldPosition.clear();
    this.worldRotation.clear();
  }

  childPath(index: number, path: string, commands: SceneCommands): NodeRef | null {
    let result: NodeRef | null = null;
    for (const name of path.split("/")) {
      if (name === "" || name === "." || name === "..") return null;
      result = this.child(index, name, commands);
      if (result === null) return null;
      index = result.index;
    }
    return result;
  }

  dispatchEvent(target: NodeRef, commands: SceneCommands, event: ScriptEvent, propagate: boolean): void {
    const ancestors: number[] = [];
    if (propagate) {
      const seen = new Set<number>();
      seen.add(target.index);
      let parent = this.parent(target.index);
      while (parent >= 0) {
        if (seen.has(parent)) throw new Error("Invalid event ancestry");
        seen.add(parent);
        ancestors.push(parent);
        parent = this.parent(parent);
      }
    }

    event._begin(target);
    try {
      event.eventPhase = EventPhase.CAPTURING_PHASE;
      for (let i = ancestors.length - 1; i >= 0; i--) {
        this.dispatchNodePhase(new NodeRef(ancestors[i], commands), event, true);
        if (event._isPropagationStopped()) return;
      }

      event.eventPhase = EventPhase.AT_TARGET;
      if (propagate) this.dispatchNodePhase(target, event, true);
      this.dispatchNodePhase(target, event, false);
      if (!event.bubbles || event._isPropagationStopped()) return;

      event.eventPhase = EventPhase.BUBBLING_PHASE;
      for (const index of ancestors) {
        this.dispatchNodePhase(new NodeRef(index, commands), event, false);
        if (event._isPropagationStopped()) return;
      }
    } finally {
      event._finish();
    }
  }

  private dispatchNodePhase(target: NodeRef, event: ScriptEvent, capture: boolean): void {
    const events = this.eventTargets.get(target.index);
    if (events !== undefined) {
      event.currentTarget = target;
      events.dispatch(event, capture);
    }
  }

  name(index: number): string {
    if (index < 0) return "";
    if (index < this.nodes.length) return this.nodes[index];
    return this.dynamicNames.get(index) ?? "";
  }

  components(index: number): Component[] {
    const result: Component[] = [];
    for (let i = 0; i < this.instances.length; i++) {
      if (this.owners[i] === index && !this.removedBindings.has(i)) result.push(this.instances[i]);
    }
    for (const entry of this.dynamicComponents.values()) {
      if (entry.owner === index) result.push(entry.instance);
    }
    return result;
  }

  componentName(instance: Component): string {
    const index = this.instances.indexOf(instance);
    if (index >= 0) return this.names[index];
    for (const entry of this.dynamicComponents.values()) {
      if (entry.instance === instance) return entry.name;
    }
    return "";
  }

  addFactory(name: string, factory: PrefabFactory): void {
    if (this.componentFactoryNames.includes(name)) throw new Error("Duplicate component factory");
    this.componentFactoryNames.push(name);
    this.componentFactories.push(factory);
  }

  factoryId(name: string): number { return this.componentFactoryNames.indexOf(name); }

  /** Returns the runtime binding for a component attached to the given node. */
  bindingFor(owner: number, component: Component): number {
    for (const [binding, entry] of this.dynamicComponents) {
      if (entry.owner === owner && entry.instance === component) return binding;
    }
    const index = this.instances.indexOf(component);
    if (index >= 0 && this.owners[index] === owner && !this.removedBindings.has(index)) return index;
    return -1;
  }

  /** Reports whether a binding came from the scene or a prefab attachment list. */
  authoredBinding(binding: number): boolean {
    return binding >= 0 && binding < this.instances.length;
  }

  markRemoved(binding: number): void { this.removedBindings.add(binding); }

  makeComponent(binding: number, owner: number, factory: number): Component {
    if (!Number.isInteger(factory) || factory < 0 || factory >= this.componentFactories.length || this.dynamicComponents.has(binding)) {
      throw new Error("Unknown component factory");
    }
    const instance = this.componentFactories[factory]([]);
    instance.node.bindScene(owner, instance.scene);
    this.dynamicComponents.set(binding, new DynamicComponent(instance, owner, this.componentFactoryNames[factory]));
    return instance;
  }

  registerTemplate(
    asset: number,
    names: string[],
    build: PrefabBuilder,
    componentNames: string[],
    textContents: (string | null)[] = [],
  ): void {
    this.templates.set(asset, new PrefabTemplate(names, build, componentNames, textContents));
  }

  registerDynamicNode(reference: number, root: number, asset: number, parent: number): void {
    let references = this.clones.get(root);
    if (references === undefined) {
      references = [];
      this.clones.set(root, references);
    }
    const template = this.templates.get(asset);
    if (this.dynamicNames.has(reference) || (template !== undefined && references.length >= template.nodes.length)) {
      throw new Error("Prefab nodes do not match the compiled template");
    }
    const name = template === undefined ? (asset < 0 ? "Node" : "") : template.nodes[references.length];
    references.push(reference);
    this.dynamicNames.set(reference, name);
    this.dynamicParents.set(reference, parent);
    const text = template?.textContents[references.length - 1];
    if (text !== null && text !== undefined) this.textContents.set(reference, text);
  }

  registerDynamicComponent(binding: number, root: number, asset: number, ordinal: number, ownerIndex: number): Component {
    const references = this.clones.get(root);
    const template = this.templates.get(asset);
    if (template === undefined || references === undefined || references.length !== template.nodes.length ||
        ordinal >= template.names.length || ownerIndex >= references.length ||
        this.dynamicComponents.has(binding)) {
      throw new Error("Prefab scripts require a matching compiled template");
    }
    let built = this.cloneComponents.get(root);
    if (built === undefined) {
      built = template.build(references);
      if (built.length !== template.names.length) {
        throw new Error("Prefab builder returned the wrong component count");
      }
      this.cloneComponents.set(root, built);
    }
    const instance = built[ordinal];
    const owner = references[ownerIndex];
    instance.node.bindScene(owner, instance.scene);
    this.dynamicComponents.set(binding, new DynamicComponent(instance, owner, template.names[ordinal]));
    return instance;
  }

  dynamicComponent(binding: number): Component | null {
    return this.dynamicComponents.get(binding)?.instance ?? null;
  }

  releaseDynamicComponent(binding: number): void {
    this.dynamicComponents.delete(binding);
  }

  releaseDynamicNode(reference: number): void {
    this.textContents.delete(reference);
    this.dynamicParents.delete(reference);
    this.dynamicNames.delete(reference);
    this.clones.delete(reference);
    this.cloneComponents.delete(reference);
    this.clearNodeEvents(reference);
  }

  updateDynamicParent(reference: number, parent: number): void {
    this.dynamicParents.set(reference, parent);
  }

  private readonly positionX = new Map<number, number>();
  private readonly positionY = new Map<number, number>();
  private readonly scaleX = new Map<number, number>();
  private readonly scaleY = new Map<number, number>();
  private readonly angles = new Map<number, number>();
  private readonly spatialPosition = new Map<number, [number, number, number]>();
  private readonly spatialRotation = new Map<number, [number, number, number]>();
  private readonly spatialScale = new Map<number, [number, number, number]>();
  private readonly worldPosition = new Map<number, [number, number, number]>();
  private readonly worldRotation = new Map<number, [number, number, number]>();

  setWorld(reference: number, field: "position" | "rotation", value: [number, number, number]): void {
    if (field === "position") this.worldPosition.set(reference, value);
    else this.worldRotation.set(reference, value);
  }

  nodeWorld(reference: number, field: "position" | "rotation"): [number, number, number] {
    const store = field === "position" ? this.worldPosition : this.worldRotation;
    return store.get(reference) ?? [0, 0, 0];
  }

  setSpatial(reference: number, field: "position" | "rotation" | "scale", value: [number, number, number]): void {
    if (field === "position") this.spatialPosition.set(reference, value);
    else if (field === "rotation") this.spatialRotation.set(reference, value);
    else this.spatialScale.set(reference, value);
  }

  setNodeTransform(reference: number, x: number, y: number): void {
    this.positionX.set(reference, x);
    this.positionY.set(reference, y);
  }

  setNodeScaleRotation(reference: number, x: number, y: number, angle: number): void {
    this.scaleX.set(reference, x);
    this.scaleY.set(reference, y);
    this.angles.set(reference, angle);
  }

  nodeX(reference: number): number { return this.positionX.get(reference) ?? 0; }
  nodeY(reference: number): number { return this.positionY.get(reference) ?? 0; }
  nodeScaleX(reference: number): number { return this.scaleX.get(reference) ?? 1; }
  nodeScaleY(reference: number): number { return this.scaleY.get(reference) ?? 1; }
  nodeAngle(reference: number): number { return this.angles.get(reference) ?? 0; }
  nodeZ(reference: number): number { return this.spatialPosition.get(reference)?.[2] ?? 0; }
  nodeSpatial(reference: number, field: "position" | "rotation" | "scale"): [number, number, number] {
    const store = field === "position"
      ? this.spatialPosition
      : field === "rotation"
        ? this.spatialRotation
        : this.spatialScale;
    return store.get(reference) ?? [0, 0, 0];
  }
  nodeText(reference: number): string {
    const text = this.textContents.get(reference);
    if (text === undefined) throw new Error("Node does not have a Text visual");
    return text;
  }
  setNodeText(reference: number, value: string): void {
    this.assertTextNode(reference);
    this.textContents.set(reference, value);
  }
  assertTextNode(reference: number): void {
    if (!this.textContents.has(reference)) throw new Error("Node does not have a Text visual");
  }
  moveNode(reference: number, x: number, y: number): void {
    this.positionX.set(reference, x);
    this.positionY.set(reference, y);
  }

  scaleNode(reference: number, x: number, y: number): void {
    this.scaleX.set(reference, x);
    this.scaleY.set(reference, y);
  }

  rotateNode(reference: number, angle: number): void {
    this.angles.set(reference, angle);
  }

  children(index: number): number[] {
    if (index < 0) return [];
    const result: number[] = [];
    for (let i = 0; i < this.nodes.length; i++) {
      if (this.parents[i] === index) result.push(i);
    }
    for (const [reference, parent] of this.dynamicParents) {
      if (parent === index) result.push(reference);
    }
    return result;
  }

  child(index: number, name: string, commands: SceneCommands): NodeRef | null {
    if (index < 0) return null;
    for (let i = 0; i < this.nodes.length; i++) {
      if (this.parents[i] === index && this.nodes[i] === name) return new NodeRef(i, commands);
    }
    for (const [reference, parent] of this.dynamicParents) {
      if (parent === index && this.dynamicNames.get(reference) === name) return new NodeRef(reference, commands);
    }
    return null;
  }
}

const scriptScene = new ScriptScene();

type ComponentQuery = string | ComponentConstructor<Component>;

function matchesComponent(component: Component, query: ComponentQuery): boolean {
  return typeof query === "string"
    ? scriptScene.componentName(component) === query
    : component instanceof query;
}

function firstComponent(index: number, query: ComponentQuery): Component | null {
  for (const component of scriptScene.components(index)) {
    if (matchesComponent(component, query)) return component;
  }
  return null;
}

function matchingComponents(index: number, query: ComponentQuery): Component[] {
  const matches: Component[] = [];
  for (const component of scriptScene.components(index)) {
    if (matchesComponent(component, query)) matches.push(component);
  }
  return matches;
}

/** @internal Selects the binding that owns subscriptions and queued edits for this callback. */
/** @internal Dispatches one player hit result under the current callback owner. */
export function dispatchPointerEvent(
  owner: Component,
  reference: number,
  phase: PointerPhase,
  x: number,
  y: number,
  pointerId: number,
  nodeX = 0,
  nodeY = 0,
): void {
  const target = new NodeRef(reference, owner.scene);
  scriptScene.dispatchEvent(target, owner.scene, new ScriptEvent(phase, 0, true, x, y, pointerId, 0, 0, nodeX, nodeY), true);
  if (pointerId === 0) {
    const mouse = phase === Input.EventType.TOUCH_START ? Input.EventType.MOUSE_DOWN
      : phase === Input.EventType.TOUCH_MOVE ? Input.EventType.MOUSE_MOVE
      : phase === Input.EventType.TOUCH_END ? Input.EventType.MOUSE_UP : Input.EventType.MOUSE_UP;
    scriptScene.dispatchEvent(target, owner.scene, new ScriptEvent(mouse, 0, true, x, y, 0, 0, 0, nodeX, nodeY), true);
  }
}

/** @internal Dispatches one wheel event to a hit node and its ancestors. */
export function dispatchWheelEvent(
  owner: Component,
  reference: number,
  deltaX: number,
  deltaY: number,
): void {
  const target = new NodeRef(reference, owner.scene);
  scriptScene.dispatchEvent(
    target,
    owner.scene,
    new ScriptEvent(InputEventType.MOUSE_WHEEL, 0, true, 0, 0, 0, deltaX, deltaY),
    true,
  );
}

export function setScriptCallbackOwner(component: Component | null): void {
  callbackOwner = component;
  callbackSceneCommands = component === null ? null : component.scene;
  input._select(component === null ? null : component.input);
}

function timerDelay(delay: number): number {
  if (Number.isNaN(delay)) return 0;
  return Math.min(2_147_483_647, Math.max(0, delay)) / 1000;
}

function allocateScriptTimer(owner: Component): number {
  let id = nextScriptTimerId;
  while (scriptTimerOwners.has(id) || id === 0) {
    id = id === Number.MAX_SAFE_INTEGER ? 1 : id + 1;
  }
  nextScriptTimerId = id === Number.MAX_SAFE_INTEGER ? 1 : id + 1;
  scriptTimerOwners.set(id, owner);
  return id;
}

function requireTimerOwner(): Component {
  if (callbackOwner === null) {
    throw new Error("Global timers can only be scheduled during a component callback");
  }
  return callbackOwner;
}

/** Schedules a one-shot callback in simulation milliseconds. */
export function auroraSetTimeout<Arguments extends unknown[]>(
  callback: (...args: Arguments) => void,
  delay = 0,
  ...args: Arguments
): number {
  return requireTimerOwner()._scheduleScriptTimer(() => callback(...args), timerDelay(delay), null);
}

/** Schedules a repeating callback in simulation milliseconds. */
export function auroraSetInterval<Arguments extends unknown[]>(
  callback: (...args: Arguments) => void,
  delay = 0,
  ...args: Arguments
): number {
  const interval = timerDelay(delay);
  return requireTimerOwner()._scheduleScriptTimer(
    () => callback(...args),
    interval,
    interval,
  );
}

/** Cancels a one-shot or repeating simulation timer. */
export function auroraClearTimer(id: number | undefined): void {
  if (id !== undefined) scriptTimerOwners.get(id)?._clearScriptTimer(id);
}

/** Schedules a callback for the next simulation frame. */
export function auroraRequestAnimationFrame(callback: (timestamp: number) => void): number {
  const owner = requireTimerOwner();
  return owner._scheduleScriptTimer(() => callback(owner._scriptTimeMilliseconds()), 0, null);
}

export function bindScriptScene(
  nodes: string[],
  parents: number[],
  instances: Component[],
  owners: number[],
  names: string[],
  prefabPaths: string[],
  textContents: (string | null)[] = [],
): void {
  scriptScene.clearEventTargets();
  scriptScene.install(nodes, parents, instances, owners, names, prefabPaths, textContents);
}

export function registerComponentFactory(name: string, factory: PrefabFactory): void {
  scriptScene.addFactory(name, factory);
}

// Function values are compared by identity because scriptc does not support
// Function as a Map key type.
const componentClasses: unknown[] = [];
const componentClassNames: string[] = [];

/** @internal Associates a component constructor with its registered factory name. */
export function registerComponentClass(type: unknown, name: string): void {
  for (let index = 0; index < componentClasses.length; index++) {
    if (componentClasses[index] === type) {
      componentClassNames[index] = name;
      return;
    }
  }
  componentClasses.push(type);
  componentClassNames.push(name);
}

function componentName(type: string | ComponentConstructor<Component>): string {
  if (typeof type === "string") return type;
  for (let index = 0; index < componentClasses.length; index++) {
    if (componentClasses[index] === type) return componentClassNames[index];
  }
  throw new Error("Component class is not registered");
}

/** @internal Caches one authored node transform before callbacks run. */
export function setScriptNodeTransform(reference: number, x: number, y: number): void {
  scriptScene.setNodeTransform(reference, x, y);
}

/** @internal Caches one authored spatial transform stream before callbacks run. */
export function setScriptNodeTransform3D(
  reference: number,
  field: "position" | "rotation" | "scale",
  value: [number, number, number],
): void {
  scriptScene.setSpatial(reference, field, value);
}

/** @internal Caches one authored node scale and rotation. */
export function setScriptNodeScaleRotation(
  reference: number,
  x: number,
  y: number,
  angle: number,
): void {
  scriptScene.setNodeScaleRotation(reference, x, y, angle);
}

/** @internal Resets scene-local handles before a scene's bindings load. */
export function resetScriptScene(): void {
  scriptScene.resetScene();
  assetManager._reset();
}

export function registerRuntimeComponent(binding: number, owner: number, factory: number): Component {
  return scriptScene.makeComponent(binding, owner, factory);
}

export function registerScriptPrefab(
  asset: number,
  names: string[],
  build: PrefabBuilder,
  componentNames: string[],
  textContents: (string | null)[] = [],
): void {
  scriptScene.registerTemplate(asset, names, build, componentNames, textContents);
}

export function registerPrefabNode(reference: number, root: number, asset: number, parent: number): void {
  scriptScene.registerDynamicNode(reference, root, asset, parent);
}

export function registerPrefabComponent(binding: number, root: number, asset: number, ordinal: number, owner: number): Component {
  return scriptScene.registerDynamicComponent(binding, root, asset, ordinal, owner);
}

export function prefabComponent(binding: number): Component | null { return scriptScene.dynamicComponent(binding); }
export function releasePrefabComponent(binding: number): void { scriptScene.releaseDynamicComponent(binding); }
export function releasePrefabNode(reference: number): void { scriptScene.releaseDynamicNode(reference); }
export function updatePrefabParent(reference: number, parent: number): void { scriptScene.updateDynamicParent(reference, parent); }

export type EasingName =
  | "linear"
  | "quadIn"
  | "quadOut"
  | "quadInOut"
  | "cubicIn"
  | "cubicOut"
  | "cubicInOut";

function easing(name: EasingName): (value: number) => number {
  switch (name) {
    case "quadIn": return (value) => value * value;
    case "quadOut": return (value) => value * (2 - value);
    case "quadInOut": return (value) => value < 0.5 ? 2 * value * value : -1 + (4 - 2 * value) * value;
    case "cubicIn": return (value) => value * value * value;
    case "cubicOut": return (value) => { const inverse = value - 1; return inverse * inverse * inverse + 1; };
    case "cubicInOut": return (value) => value < 0.5
      ? 4 * value * value * value
      : 1 + 4 * (value - 1) * (value - 1) * (value - 1);
    default: return (value) => value;
  }
}

/** Channels a tween can drive. Omitting a channel leaves it unchanged. */
export interface TweenValues {
  x?: number;
  y?: number;
  position?: { x?: number; y?: number };
  scaleX?: number;
  scaleY?: number;
  scale?: { x?: number; y?: number };
  angle?: number;
}

function channelX(values: TweenValues): number | undefined {
  if (values.x !== undefined) return values.x;
  return values.position?.x;
}

function channelY(values: TweenValues): number | undefined {
  if (values.y !== undefined) return values.y;
  return values.position?.y;
}

function channelScaleX(values: TweenValues): number | undefined {
  if (values.scaleX !== undefined) return values.scaleX;
  return values.scale?.x;
}

function channelScaleY(values: TweenValues): number | undefined {
  if (values.scaleY !== undefined) return values.scaleY;
  return values.scale?.y;
}

class TweenStep {
  readonly kind: "to" | "by" | "delay" | "call";
  readonly duration: number;
  readonly values: TweenValues;
  readonly ease: (value: number) => number;
  readonly callback: (() => void) | null;
  startX = 0;
  startY = 0;
  startScaleX = 1;
  startScaleY = 1;
  startAngle = 0;
  begun = false;

  constructor(
    kind: "to" | "by" | "delay" | "call",
    duration = 0,
    values: TweenValues = {},
    ease: (value: number) => number = (value) => value,
    callback: (() => void) | null = null,
  ) {
    this.kind = kind;
    this.duration = Math.max(0, duration);
    this.values = values;
    this.ease = ease;
    this.callback = callback;
  }
}

/** A chainable node tween advanced once per simulation tick. */
export class Tween {
  private readonly target: NodeRef;
  private steps: TweenStep[] = [];
  private index = 0;
  private stepElapsed = 0;
  private remainingRepeats = 0;
  private forever = false;
  private running = false;

  constructor(target: NodeRef) {
    this.target = target;
  }

  to(duration: number, values: TweenValues, options?: { easing?: EasingName }): this {
    this.steps.push(new TweenStep("to", duration, values, easing(options?.easing ?? "linear")));
    return this;
  }

  by(duration: number, values: TweenValues, options?: { easing?: EasingName }): this {
    this.steps.push(new TweenStep("by", duration, values, easing(options?.easing ?? "linear")));
    return this;
  }

  delay(seconds: number): this {
    this.steps.push(new TweenStep("delay", seconds));
    return this;
  }

  call(callback: () => void): this {
    this.steps.push(new TweenStep("call", 0, {}, (value) => value, callback));
    return this;
  }

  /** Repeats the whole sequence this many additional times. */
  repeat(times = 1): this {
    this.remainingRepeats = Math.max(0, Math.floor(times));
    return this;
  }

  repeatForever(): this {
    this.forever = true;
    return this;
  }

  start(): this {
    if (this.running || this.steps.length === 0) return this;
    this.index = 0;
    this.stepElapsed = 0;
    this.running = true;
    activeTweens.push(this);
    return this;
  }

  stop(): this {
    this.running = false;
    return this;
  }

  private restart(): boolean {
    if (this.forever) {
      this.index = 0;
      return true;
    }
    if (this.remainingRepeats > 0) {
      this.remainingRepeats--;
      this.index = 0;
      return true;
    }
    this.running = false;
    return false;
  }

  private apply(step: TweenStep, value: number): void {
    if (!step.begun) {
      step.begun = true;
      step.startX = this.target.x;
      step.startY = this.target.y;
      step.startScaleX = this.target.scaleX;
      step.startScaleY = this.target.scaleY;
      step.startAngle = this.target.angle;
    }
    const t = step.ease(Math.min(1, Math.max(0, value)));
    const toX = channelX(step.values);
    const toY = channelY(step.values);
    if (toX !== undefined || toY !== undefined) {
      const x = toX === undefined
        ? this.target.x
        : step.kind === "by" ? step.startX + toX * t : step.startX + (toX - step.startX) * t;
      const y = toY === undefined
        ? this.target.y
        : step.kind === "by" ? step.startY + toY * t : step.startY + (toY - step.startY) * t;
      this.target.setPosition(x, y);
    }
    const toScaleX = channelScaleX(step.values);
    const toScaleY = channelScaleY(step.values);
    if (toScaleX !== undefined || toScaleY !== undefined) {
      const scaleX = toScaleX === undefined
        ? this.target.scaleX
        : step.kind === "by"
          ? step.startScaleX + toScaleX * t
          : step.startScaleX + (toScaleX - step.startScaleX) * t;
      const scaleY = toScaleY === undefined
        ? this.target.scaleY
        : step.kind === "by"
          ? step.startScaleY + toScaleY * t
          : step.startScaleY + (toScaleY - step.startScaleY) * t;
      this.target.setScale(scaleX, scaleY);
    }
    const toAngle = step.values.angle;
    if (toAngle !== undefined) {
      const angle = step.kind === "by"
        ? step.startAngle + toAngle * t
        : step.startAngle + (toAngle - step.startAngle) * t;
      this.target.setAngle(angle);
    }
  }

  /** @internal Advances this tween. Returns false when it has finished. */
  _advance(dt: number): boolean {
    if (!this.running) return false;
    let budget = Math.max(0, dt);
    let guard = 0;
    while (this.running && guard++ < 10000) {
      if (this.index >= this.steps.length && !this.restart()) break;
      const step = this.steps[this.index];
      if (step.kind === "call") {
        step.callback?.();
        this.index++;
        continue;
      }
      if (step.duration <= 0) {
        if (step.kind !== "delay") this.apply(step, 1);
        this.index++;
        continue;
      }
      if (budget <= 0) break;
      const use = Math.min(budget, step.duration - this.stepElapsed);
      this.stepElapsed += use;
      budget -= use;
      if (step.kind !== "delay") {
        this.apply(step, this.stepElapsed / step.duration);
      }
      if (this.stepElapsed >= step.duration) {
        if (step.kind !== "delay") this.apply(step, 1);
        this.index++;
        this.stepElapsed = 0;
      }
    }
    return this.running;
  }
}

interface Advanceable {
  _advance(dt: number): boolean;
}

const activeTweens: Advanceable[] = [];

interface ValueStep {
  kind: "to" | "by" | "delay" | "call";
  duration: number;
  value: number;
  ease: (value: number) => number;
  callback: (() => void) | null;
  start: number;
  begun: boolean;
}

/** A chainable tween for any numeric script property. */
export class ValueTween {
  private readonly steps: ValueStep[] = [];
  private index = 0;
  private elapsed = 0;
  private repeats = 0;
  private forever = false;
  private running = false;

  constructor(
    private readonly get: () => number,
    private readonly set: (value: number) => void,
  ) {}

  to(value: number, duration: number, options?: { easing?: EasingName }): this {
    this.steps.push({
      kind: "to",
      duration: Math.max(0, duration),
      value,
      ease: easing(options?.easing ?? "linear"),
      callback: null,
      start: 0,
      begun: false,
    });
    return this;
  }

  by(delta: number, duration: number, options?: { easing?: EasingName }): this {
    this.steps.push({
      kind: "by",
      duration: Math.max(0, duration),
      value: delta,
      ease: easing(options?.easing ?? "linear"),
      callback: null,
      start: 0,
      begun: false,
    });
    return this;
  }

  delay(seconds: number): this {
    this.steps.push({
      kind: "delay",
      duration: Math.max(0, seconds),
      value: 0,
      ease: (value) => value,
      callback: null,
      start: 0,
      begun: false,
    });
    return this;
  }

  call(callback: () => void): this {
    this.steps.push({
      kind: "call",
      duration: 0,
      value: 0,
      ease: (value) => value,
      callback,
      start: 0,
      begun: false,
    });
    return this;
  }

  repeat(times = 1): this {
    this.repeats = Math.max(0, Math.floor(times));
    return this;
  }

  repeatForever(): this {
    this.forever = true;
    return this;
  }

  start(): this {
    if (!this.running && this.steps.length > 0) {
      this.index = 0;
      this.elapsed = 0;
      this.running = true;
      activeTweens.push(this);
    }
    return this;
  }

  stop(): this {
    this.running = false;
    return this;
  }

  /** @internal Advances this value tween once per simulation tick. */
  _advance(dt: number): boolean {
    if (!this.running) return false;
    let budget = Math.max(0, dt);
    let guard = 0;
    while (this.running && guard++ < 10000) {
      if (this.index >= this.steps.length) {
        if (this.forever) {
          this.resetSteps();
          this.index = 0;
        } else if (this.repeats > 0) {
          this.repeats--;
          this.resetSteps();
          this.index = 0;
        } else {
          this.running = false;
          break;
        }
      }
      const step = this.steps[this.index];
      if (step.kind === "call") {
        step.callback?.();
        this.index++;
        continue;
      }
      if (step.duration <= 0) {
        if (step.kind !== "delay") this.apply(step, 1);
        this.index++;
        continue;
      }
      if (budget <= 0) break;
      const use = Math.min(budget, step.duration - this.elapsed);
      this.elapsed += use;
      budget -= use;
      if (step.kind !== "delay") this.apply(step, this.elapsed / step.duration);
      if (this.elapsed >= step.duration) {
        this.index++;
        this.elapsed = 0;
      }
    }
    return this.running;
  }

  private resetSteps(): void {
    for (const step of this.steps) {
      step.begun = false;
    }
  }

  private apply(step: ValueStep, value: number): void {
    if (!step.begun) {
      step.begun = true;
      step.start = this.get();
    }
    const t = step.ease(Math.min(1, Math.max(0, value)));
    this.set(
      step.kind === "by" ? step.start + step.value * t : step.start + (step.value - step.start) * t,
    );
  }
}

/** Creates a tween for any numeric property with a getter and setter. */
export function tweenValue(
  get: () => number,
  set: (value: number) => void,
): ValueTween {
  return new ValueTween(get, set);
}

/** High-level pointer gesture delivered by a GestureDetector. */
export class GestureEvent {
  constructor(
    readonly type: string,
    readonly x: number,
    readonly y: number,
    readonly deltaX = 0,
    readonly deltaY = 0,
    readonly direction = "",
  ) {}
}

interface GestureOptions {
  /** Node-local pixel movement that starts a pan or cancels a tap. */
  tapDistance?: number;
  /** Held seconds before a long press fires. */
  longPressSeconds?: number;
  /** Minimum node-local travel for a swipe on release. */
  swipeDistance?: number;
}

/** Recognises tap, double tap, long press, pan and swipe on one node. */
export class GestureDetector {
  private readonly listeners = new Map<string, Array<(event: GestureEvent) => void>>();
  private pointer = -1;
  private startX = 0;
  private startY = 0;
  private lastX = 0;
  private lastY = 0;
  private held = 0;
  private moved = false;
  private longPressed = false;
  private lastTap = -Infinity;
  private clock = 0;
  private readonly tapDistance: number;
  private readonly longPressSeconds: number;
  private readonly swipeDistance: number;

  constructor(
    private readonly node: NodeRef,
    options: GestureOptions = {},
  ) {
    this.tapDistance = options.tapDistance ?? 12;
    this.longPressSeconds = options.longPressSeconds ?? 0.5;
    this.swipeDistance = options.swipeDistance ?? 24;
    node.on(InputEventType.TOUCH_START, this.onStart);
    node.on(InputEventType.TOUCH_MOVE, this.onMove);
    node.on(InputEventType.TOUCH_END, this.onEnd);
    node.on(InputEventType.TOUCH_CANCEL, this.onCancel);
    activeGestures.push(this);
  }

  on(type: string, callback: (event: GestureEvent) => void): void {
    const list = this.listeners.get(type) ?? [];
    list.push(callback);
    this.listeners.set(type, list);
  }

  private emit(event: GestureEvent): void {
    for (const callback of this.listeners.get(event.type) ?? []) callback(event);
  }

  private readonly onStart = (event: ScriptEvent): void => {
    if (this.pointer >= 0) return;
    this.pointer = event.pointerId;
    this.startX = this.lastX = event.nodeX;
    this.startY = this.lastY = event.nodeY;
    this.held = 0;
    this.moved = false;
    this.longPressed = false;
  };

  private readonly onMove = (event: ScriptEvent): void => {
    if (event.pointerId !== this.pointer) return;
    this.lastX = event.nodeX;
    this.lastY = event.nodeY;
    if (Math.hypot(event.nodeX - this.startX, event.nodeY - this.startY) > this.tapDistance) {
      this.moved = true;
    }
    if (this.moved) {
      this.emit(new GestureEvent("pan", event.nodeX, event.nodeY, event.nodeX - this.startX, event.nodeY - this.startY));
    }
  };

  private readonly onEnd = (event: ScriptEvent): void => {
    if (event.pointerId !== this.pointer) return;
    if (!this.moved && !this.longPressed) {
      const double = this.clock - this.lastTap < 0.3;
      this.emit(new GestureEvent(double ? "doubleTap" : "tap", this.lastX, this.lastY));
      this.lastTap = this.clock;
    } else if (this.moved) {
      const dx = this.lastX - this.startX;
      const dy = this.lastY - this.startY;
      if (Math.hypot(dx, dy) >= this.swipeDistance) {
        const direction = Math.abs(dx) > Math.abs(dy)
          ? dx > 0 ? "right" : "left"
          : dy > 0 ? "down" : "up";
        this.emit(new GestureEvent("swipe", this.lastX, this.lastY, dx, dy, direction));
      }
    }
    this.pointer = -1;
  };

  private readonly onCancel = (event: ScriptEvent): void => {
    if (event.pointerId === this.pointer) this.pointer = -1;
  };

  /** @internal Advances gesture timers once per simulation tick. */
  _advance(dt: number): void {
    this.clock += dt;
    if (this.pointer < 0 || this.moved || this.longPressed) return;
    this.held += dt;
    if (this.held >= this.longPressSeconds) {
      this.longPressed = true;
      this.emit(new GestureEvent("longPress", this.lastX, this.lastY));
    }
  }

  destroy(): void {
    this.node.off(InputEventType.TOUCH_START, this.onStart);
    this.node.off(InputEventType.TOUCH_MOVE, this.onMove);
    this.node.off(InputEventType.TOUCH_END, this.onEnd);
    this.node.off(InputEventType.TOUCH_CANCEL, this.onCancel);
    const index = activeGestures.indexOf(this);
    if (index >= 0) activeGestures.splice(index, 1);
  }
}

const activeGestures: GestureDetector[] = [];

/** @internal Advances every active gesture detector once per simulation tick. */
export function advanceGestures(dt: number): void {
  for (const detector of activeGestures.slice()) detector._advance(dt);
}

/** @internal Drops gesture listeners before a new scene loads. */
export function resetGestures(): void {
  for (const detector of activeGestures.slice()) detector.destroy();
  activeGestures.length = 0;
}

/** Creates a node tween for a script or component node. */
export function tween(target: NodeRef | Node): Tween {
  if (target instanceof NodeRef) return new Tween(target);
  const reference = target.nodeRef();
  if (reference === null) throw new Error("Tween target is not bound to a scene");
  return new Tween(reference);
}

/** @internal Advances every active tween once per simulation tick. */
export function advanceTweens(dt: number): void {
  for (const active of activeTweens.slice()) {
    if (!active._advance(dt)) {
      const index = activeTweens.indexOf(active);
      if (index >= 0) activeTweens.splice(index, 1);
    }
  }
}

export function executionOrder(_order: number): ClassDecorator {
  return (_target): void => {};
}

// The editor reads these annotations from source, and the native build erases them.
type PropertyToken =
  | NumberConstructor
  | typeof Integer
  | BooleanConstructor
  | StringConstructor
  | typeof Text
  | typeof Vector2
  | typeof Vector3
  | typeof Size
  | typeof Rect
  | typeof Color
  | typeof Bitmask
  | typeof Image
  | typeof TextRef
  | typeof ImageRef
  | typeof NodeRef
  | typeof Prefab
  | typeof AudioClip
  | typeof SpriteFrame;
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
export function ccclass(target: Function): void;
export function ccclass(name?: string): ClassDecorator;
export function ccclass(name?: string | Function): ClassDecorator | void {
  if (typeof name !== "function") return (_target): void => {};
}

export function property(target: object, key: string | symbol): void;
export function property(options?: PropertyToken | PropertyOptions): PropertyDecorator;
export function property(_options: object = {}, key?: string | symbol): PropertyDecorator | void {
  if (key === undefined) return (_target, _key): void => {};
}
