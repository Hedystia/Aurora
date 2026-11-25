import type { LoadedPrefab, Prefab, SceneCommands } from "./index.js";
export declare class AudioClip {
    readonly path: string;
    readonly index: number;
    constructor(path: string, index: number);
}
export declare class SpriteFrame {
    readonly path: string;
    private readonly nodes;
    private readonly slots;
    constructor(path: string, nodes: number[], slots: number[]);
    frameFor(node: number): number;
}
/** An audio clip imported from disk on demand rather than a scene bank. */
export declare class LoadedAudioClip extends AudioClip {
    readonly handle: number;
    constructor(path: string, handle: number);
}
/** A sprite frame imported from disk on demand rather than a scene bank. */
export declare class LoadedSpriteFrame extends SpriteFrame {
    readonly handle: number;
    constructor(path: string, handle: number);
    frameFor(_node: number): number;
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
export declare function registerScriptResources(audio: string[], frames: string[], nodes: number[], slots: number[], prefabs?: string[]): void;
export declare class Resources {
    private readonly scene;
    private pending;
    private generation;
    private readonly clips;
    private readonly sprites;
    constructor(scene: SceneCommands);
    bundle(root?: string): AssetBundle;
    releaseAudioClip(path: string): void;
    releaseSpriteFrame(path: string): void;
    /** Drops every script-side handle. The player keeps scene-owned asset data. */
    releaseAll(): void;
    /** Cancels queued load callbacks that have not run yet. */
    cancelLoads(): void;
    /** Number of load or preload callbacks waiting for the next phase. */
    get pendingCount(): number;
    getAudioClip(path: string): AudioClip | null;
    /** Returns a registered audio clip, or throws when the scene cannot resolve it. */
    requireAudioClip(path: string): AudioClip;
    /** Returns a registered sprite frame, or throws when the scene cannot resolve it. */
    requireSpriteFrame(path: string): SpriteFrame;
    getSpriteFrame(path: string): SpriteFrame | null;
    getPrefab(path: string): Prefab | null;
    loadAudioClip(path: string, callback: AudioClipCallback): void;
    loadSpriteFrame(path: string, callback: SpriteFrameCallback): void;
    /** Imports a PNG under the project on demand and delivers a refcounted handle. */
    loadSpriteFile(path: string, callback: SpriteFrameCallback): void;
    /** Imports a WAV or MP3 under the project on demand. */
    loadAudioFile(path: string, callback: AudioClipCallback): void;
    /** Imports a prefab under the project on demand. */
    loadPrefabFile(path: string, callback: PrefabCallback): void;
    /** Releases one on-demand asset handle directly. */
    releaseAssetHandle(handle: number): void;
    /** Releases one on-demand sprite handle. Scene bank handles are unaffected. */
    releaseSpriteFile(frame: LoadedSpriteFrame): void;
    /** Releases one on-demand audio handle. */
    releaseAudioFile(clip: LoadedAudioClip): void;
    /** Releases one on-demand prefab handle. Already-instantiated nodes are unaffected. */
    releasePrefabFile(prefab: LoadedPrefab): void;
    loadPrefab(path: string, callback: PrefabCallback): void;
    /** @internal Queues one callback for the next resource phase. */
    _defer(callback: () => void): void;
    /** @internal Runs one queued resource callback per simulation phase. */
    _advance(): void;
    /** @internal Drops resource callbacks and handles during component teardown. */
    _clear(): void;
}
/** A directory-scoped view of the current scene's registered resources. */
export declare class AssetBundle {
    private readonly resources;
    private readonly root;
    private readonly generation;
    private closed;
    constructor(resources: Resources, root: string);
    private static path;
    private asset;
    private paths;
    getAudioClip(path: string): AudioClip | null;
    getSpriteFrame(path: string): SpriteFrame | null;
    requireAudioClip(path: string): AudioClip;
    requireSpriteFrame(path: string): SpriteFrame;
    getPrefab(path: string): Prefab | null;
    loadAudioClip(path: string, callback: AudioClipCallback): void;
    loadSpriteFrame(path: string, callback: SpriteFrameCallback): void;
    loadPrefab(path: string, callback: PrefabCallback): void;
    preloadAudioClip(path: string, callback: AudioClipCallback): void;
    preloadSpriteFrame(path: string, callback: SpriteFrameCallback): void;
    preloadPrefab(path: string, callback: PrefabCallback): void;
    /**
     * Resolves every registered asset under this bundle root, one resource phase
     * at a time. Progress counts completed assets; the final callback receives
     * all resolved handles and any per-asset errors.
     */
    preloadAll(callback?: BundleCallback, progress?: BundleProgressCallback): void;
    /** Alias for preloadAll that also reports the resolved handles. */
    loadAll(callback?: BundleCallback, progress?: BundleProgressCallback): void;
    releaseAudioClip(path: string): void;
    releaseSpriteFrame(path: string): void;
    /** Drops every handle owned by the component behind this bundle view. */
    releaseAll(): void;
    /** Cancels queued load callbacks for the component behind this view. */
    cancelLoads(): void;
    release(): void;
}
