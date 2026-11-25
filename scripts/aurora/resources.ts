import type { LoadedPrefab, Prefab, SceneCommands } from "./index.ts";

export class AudioClip {
  readonly path: string;
  readonly index: number;

  constructor(path: string, index: number) {
    this.path = path;
    this.index = index;
  }
}

export class SpriteFrame {
  readonly path: string;
  private readonly nodes: number[];
  private readonly slots: number[];

  constructor(path: string, nodes: number[], slots: number[]) {
    this.path = path;
    this.nodes = nodes;
    this.slots = slots;
  }

  frameFor(node: number): number {
    const index = this.nodes.indexOf(node);
    return index === -1 ? -1 : this.slots[index];
  }
}

/** An audio clip imported from disk on demand rather than a scene bank. */
export class LoadedAudioClip extends AudioClip {
  readonly handle: number;

  constructor(path: string, handle: number) {
    super(path, -1);
    this.handle = handle;
  }
}

/** A sprite frame imported from disk on demand rather than a scene bank. */
export class LoadedSpriteFrame extends SpriteFrame {
  readonly handle: number;

  constructor(path: string, handle: number) {
    super(path, [], []);
    this.handle = handle;
  }

  frameFor(_node: number): number {
    return this.handle;
  }
}

export type AudioClipCallback = (error: Error | null, clip: AudioClip | null) => void;
export type SpriteFrameCallback = (error: Error | null, frame: SpriteFrame | null) => void;
export type PrefabCallback = (error: Error | null, prefab: Prefab | null) => void;
export type BundleProgressCallback = (completed: number, total: number, path: string) => void;

export interface BundleAssets {
  audio: AudioClip[];
  sprites: SpriteFrame[];
  prefabs: Prefab[];
  errors: Error[];
}

export type BundleCallback = (error: Error | null, assets: BundleAssets | null) => void;

function logicalPath(path: string): string {
  let relative = path;
  if (relative.startsWith("./")) relative = relative.slice(2);
  if (relative.startsWith("assets/resources/")) relative = relative.slice(17);
  else if (relative.startsWith("resources/")) relative = relative.slice(10);
  const dot = relative.lastIndexOf(".");
  return dot > relative.lastIndexOf("/") ? relative.slice(0, dot) : relative;
}

function registeredPath(path: string, paths: string[]): string | null {
  const exact = path.startsWith("./") ? path.slice(2) : path;
  const key = logicalPath(exact);
  let found: string | null = null;
  let ambiguous = false;
  for (const value of paths) {
    if (value === exact) return value;
    if (logicalPath(value) !== key || found === value) continue;
    if (found !== null) ambiguous = true;
    found = value;
  }
  return ambiguous ? null : found;
}

class Catalog {
  audio: string[] = [];
  frames: string[] = [];
  frameNodes: number[] = [];
  frameSlots: number[] = [];
  prefabs: string[] = [];
  generation = 0;
}

// Registration is scene-wide, while each component owns the handles it caches.
// Releasing a handle therefore never invalidates another component's view.
const catalog = new Catalog();

export function registerScriptResources(
  audio: string[],
  frames: string[],
  nodes: number[],
  slots: number[],
  prefabs: string[] = [],
): void {
  if (frames.length !== nodes.length || frames.length !== slots.length) {
    throw new Error("Invalid Aurora script resource catalog");
  }
  catalog.generation++;
  catalog.audio = audio;
  catalog.frames = frames;
  catalog.frameNodes = nodes;
  catalog.frameSlots = slots;
  catalog.prefabs = prefabs;
}

export class Resources {
  private readonly scene: SceneCommands;
  private pending: (() => void)[] = [];
  private generation = catalog.generation;
  private readonly clips = new Map<string, AudioClip>();
  private readonly sprites = new Map<string, SpriteFrame>();

  constructor(scene: SceneCommands) {
    this.scene = scene;
  }

  bundle(root = ""): AssetBundle {
    return new AssetBundle(this, root);
  }

  releaseAudioClip(path: string): void {
    const key = registeredPath(path, catalog.audio);
    if (key !== null) this.clips.delete(key);
  }

  releaseSpriteFrame(path: string): void {
    const key = registeredPath(path, catalog.frames);
    if (key !== null) this.sprites.delete(key);
  }

  /** Drops every script-side handle. The player keeps scene-owned asset data. */
  releaseAll(): void {
    this.clips.clear();
    this.sprites.clear();
    this.pending = [];
  }

  /** Cancels queued load callbacks that have not run yet. */
  cancelLoads(): void {
    this.pending = [];
  }

  /** Number of load or preload callbacks waiting for the next phase. */
  get pendingCount(): number { return this.pending.length; }

  getAudioClip(path: string): AudioClip | null {
    const key = registeredPath(path, catalog.audio);
    if (key === null) return null;
    const cached = this.clips.get(key);
    if (cached !== undefined) return cached;
    const clip = new AudioClip(key, catalog.audio.indexOf(key));
    this.clips.set(key, clip);
    return clip;
  }

  /** Returns a registered audio clip, or throws when the scene cannot resolve it. */
  requireAudioClip(path: string): AudioClip {
    const clip = this.getAudioClip(path);
    if (clip === null) throw new Error(`Authored audio clip is not registered: ${path}`);
    return clip;
  }

  /** Returns a registered sprite frame, or throws when the scene cannot resolve it. */
  requireSpriteFrame(path: string): SpriteFrame {
    const frame = this.getSpriteFrame(path);
    if (frame === null) throw new Error(`Authored sprite frame is not registered: ${path}`);
    return frame;
  }

  getSpriteFrame(path: string): SpriteFrame | null {
    const key = registeredPath(path, catalog.frames);
    if (key === null) return null;
    const cached = this.sprites.get(key);
    if (cached !== undefined) return cached;
    const nodes: number[] = [];
    const slots: number[] = [];
    for (let i = 0; i < catalog.frames.length; i++) {
      if (catalog.frames[i] === key) {
        nodes.push(catalog.frameNodes[i]);
        slots.push(catalog.frameSlots[i]);
      }
    }
    const sprite = new SpriteFrame(key, nodes, slots);
    this.sprites.set(key, sprite);
    return sprite;
  }

  getPrefab(path: string): Prefab | null {
    const exact = path.startsWith("./") ? path.slice(2) : path;
    const registered = this.scene.loadPrefab(exact);
    if (registered !== null) return registered;
    const key = logicalPath(exact);
    const candidates = [
      this.scene.loadPrefab(`assets/resources/${key}.prefab`),
      this.scene.loadPrefab(`assets/resources/${key}.aurora-prefab`),
      this.scene.loadPrefab(`resources/${key}.prefab`),
      this.scene.loadPrefab(`resources/${key}.aurora-prefab`),
    ];
    let found: Prefab | null = null;
    for (const candidate of candidates) {
      if (candidate === null) continue;
      if (found !== null && found.assetIndex !== candidate.assetIndex) return null;
      found = candidate;
    }
    return found;
  }

  loadAudioClip(path: string, callback: AudioClipCallback): void {
    this._defer(() => {
      const asset = this.getAudioClip(path);
      callback(asset === null ? new Error(`Audio clip is missing or ambiguous: ${path}`) : null, asset);
    });
  }

  loadSpriteFrame(path: string, callback: SpriteFrameCallback): void {
    this._defer(() => {
      const asset = this.getSpriteFrame(path);
      callback(asset === null ? new Error(`Sprite frame is missing or ambiguous: ${path}`) : null, asset);
    });
  }

  /** Imports a PNG under the project on demand and delivers a refcounted handle. */
  loadSpriteFile(path: string, callback: SpriteFrameCallback): void {
    this.scene.loadSprite(path, callback);
  }

  /** Imports a WAV or MP3 under the project on demand. */
  loadAudioFile(path: string, callback: AudioClipCallback): void {
    this.scene.loadAudio(path, callback);
  }

  /** Imports a prefab under the project on demand. */
  loadPrefabFile(path: string, callback: PrefabCallback): void {
    this.scene.loadPrefabFile(path, callback);
  }

  /** Releases one on-demand asset handle directly. */
  releaseAssetHandle(handle: number): void {
    this.scene.releaseAsset(handle);
  }

  /** Releases one on-demand sprite handle. Scene bank handles are unaffected. */
  releaseSpriteFile(frame: LoadedSpriteFrame): void {
    this.scene.releaseAsset(frame.handle);
  }

  /** Releases one on-demand audio handle. */
  releaseAudioFile(clip: LoadedAudioClip): void {
    this.scene.releaseAsset(clip.handle);
  }

  /** Releases one on-demand prefab handle. Already-instantiated nodes are unaffected. */
  releasePrefabFile(prefab: LoadedPrefab): void {
    this.scene.releaseAsset(prefab.handle);
  }

  loadPrefab(path: string, callback: PrefabCallback): void {
    this._defer(() => {
      const asset = this.getPrefab(path);
      callback(asset === null ? new Error(`Prefab is missing or ambiguous: ${path}`) : null, asset);
    });
  }

  /** @internal Queues one callback for the next resource phase. */
  _defer(callback: () => void): void {
    if (this.generation !== catalog.generation) {
      this.pending = [];
      this.generation = catalog.generation;
    }
    if (this.pending.length >= 128) throw new Error("Too many pending resource loads");
    this.pending.push(callback);
  }

  /** @internal Runs one queued resource callback per simulation phase. */
  _advance(): void {
    if (this.generation !== catalog.generation) {
      this.pending = [];
      this.generation = catalog.generation;
      return;
    }
    const callback = this.pending.shift();
    if (callback !== undefined) callback();
  }

  /** @internal Drops resource callbacks and handles during component teardown. */
  _clear(): void {
    this.pending = [];
    this.clips.clear();
    this.sprites.clear();
  }
}


/** A directory-scoped view of the current scene's registered resources. */
export class AssetBundle {
  private readonly resources: Resources;
  private readonly root: string;
  private readonly generation: number;
  private closed = false;

  constructor(resources: Resources, root: string) {
    this.resources = resources;
    this.root = AssetBundle.path(root, true);
    this.generation = catalog.generation;
  }

  private static path(path: string, allowEmpty = false): string {
    const normalized = path.split("\\").join("/");
    if (normalized.startsWith("/")) throw new Error("Invalid resource path");
    const parts = normalized.split("/");
    const clean: string[] = [];
    for (const part of parts) {
      if (part === "" || part === ".") continue;
      if (part === ".." || part.includes(":")) throw new Error("Invalid resource path");
      clean.push(part);
    }
    const result = clean.join("/");
    if (!allowEmpty && result.length === 0) throw new Error("Resource path is empty");
    return result;
  }

  private asset(path: string): string {
    if (this.closed || this.generation !== catalog.generation) {
      throw new Error("Asset bundle is no longer active");
    }
    const relative = AssetBundle.path(path);
    return this.root.length === 0 ? relative : `${this.root}/${relative}`;
  }

  private paths(values: string[]): string[] {
    const found = new Set<string>();
    for (const path of values) {
      if (this.root.length === 0 || path === this.root || path.startsWith(`${this.root}/`)) {
        found.add(path);
      }
    }
    return Array.from(found);
  }

  getAudioClip(path: string): AudioClip | null {
    return this.resources.getAudioClip(this.asset(path));
  }

  getSpriteFrame(path: string): SpriteFrame | null {
    return this.resources.getSpriteFrame(this.asset(path));
  }

  requireAudioClip(path: string): AudioClip {
    return this.resources.requireAudioClip(this.asset(path));
  }

  requireSpriteFrame(path: string): SpriteFrame {
    return this.resources.requireSpriteFrame(this.asset(path));
  }

  getPrefab(path: string): Prefab | null {
    return this.resources.getPrefab(this.asset(path));
  }

  loadAudioClip(path: string, callback: AudioClipCallback): void {
    const asset = this.asset(path);
    this.resources.loadAudioClip(asset, (error, clip) => {
      if (this.closed || this.generation !== catalog.generation) {
        callback(new Error("Asset bundle is no longer active"), null);
      } else {
        callback(error, clip);
      }
    });
  }

  loadSpriteFrame(path: string, callback: SpriteFrameCallback): void {
    const asset = this.asset(path);
    this.resources.loadSpriteFrame(asset, (error, frame) => {
      if (this.closed || this.generation !== catalog.generation) {
        callback(new Error("Asset bundle is no longer active"), null);
      } else {
        callback(error, frame);
      }
    });
  }

  loadPrefab(path: string, callback: PrefabCallback): void {
    const asset = this.asset(path);
    this.resources.loadPrefab(asset, (error, prefab) => {
      if (this.closed || this.generation !== catalog.generation) {
        callback(new Error("Asset bundle is no longer active"), null);
      } else {
        callback(error, prefab);
      }
    });
  }

  preloadAudioClip(path: string, callback: AudioClipCallback): void {
    this.loadAudioClip(path, callback);
  }

  preloadSpriteFrame(path: string, callback: SpriteFrameCallback): void {
    this.loadSpriteFrame(path, callback);
  }

  preloadPrefab(path: string, callback: PrefabCallback): void {
    this.loadPrefab(path, callback);
  }

  /**
   * Resolves every registered asset under this bundle root, one resource phase
   * at a time. Progress counts completed assets; the final callback receives
   * all resolved handles and any per-asset errors.
   */
  preloadAll(callback?: BundleCallback, progress?: BundleProgressCallback): void {
    const audio = this.paths(catalog.audio);
    const sprites = this.paths(catalog.frames);
    const prefabs = this.paths(catalog.prefabs);
    const total = audio.length + sprites.length + prefabs.length;
    const result: BundleAssets = { audio: [], sprites: [], prefabs: [], errors: [] };
    let completed = 0;
    let finished = false;
    const complete = (path: string, error: Error | null): void => {
      if (error !== null) result.errors.push(error);
      completed++;
      progress?.(completed, total, path);
      if (finished || completed < total) return;
      finished = true;
      if (this.closed || this.generation !== catalog.generation) {
        callback?.(new Error("Asset bundle is no longer active"), null);
        return;
      }
      callback?.(result.errors[0] ?? null, result);
    };
    if (total === 0) {
      this.resources._defer(() => callback?.(null, result));
      return;
    }
    for (const path of audio) {
      this.resources.loadAudioClip(path, (error, clip) => {
        if (clip !== null) result.audio.push(clip);
        complete(path, error);
      });
    }
    for (const path of sprites) {
      this.resources.loadSpriteFrame(path, (error, frame) => {
        if (frame !== null) result.sprites.push(frame);
        complete(path, error);
      });
    }
    for (const path of prefabs) {
      this.resources.loadPrefab(path, (error, prefab) => {
        if (prefab !== null) result.prefabs.push(prefab);
        complete(path, error);
      });
    }
  }

  /** Alias for preloadAll that also reports the resolved handles. */
  loadAll(callback?: BundleCallback, progress?: BundleProgressCallback): void {
    this.preloadAll(callback, progress);
  }

  releaseAudioClip(path: string): void {
    this.resources.releaseAudioClip(this.asset(path));
  }

  releaseSpriteFrame(path: string): void {
    this.resources.releaseSpriteFrame(this.asset(path));
  }

  /** Drops every handle owned by the component behind this bundle view. */
  releaseAll(): void {
    this.resources.releaseAll();
  }

  /** Cancels queued load callbacks for the component behind this view. */
  cancelLoads(): void {
    this.resources.cancelLoads();
  }

  release(): void {
    if (this.closed) return;
    for (const path of this.paths(catalog.audio)) this.resources.releaseAudioClip(path);
    for (const path of this.paths(catalog.frames)) this.resources.releaseSpriteFrame(path);
    this.closed = true;
  }
}
