import type { Prefab, SceneCommands } from "./index.js";
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
export type AudioClipCallback = (error: Error | null, clip: AudioClip | null) => void;
export type SpriteFrameCallback = (error: Error | null, frame: SpriteFrame | null) => void;
export type PrefabCallback = (error: Error | null, prefab: Prefab | null) => void;
export declare function registerScriptResources(audio: string[], frames: string[], nodes: number[], slots: number[]): void;
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
    loadPrefab(path: string, callback: PrefabCallback): void;
    private defer;
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
    releaseAudioClip(path: string): void;
    releaseSpriteFrame(path: string): void;
    /** Drops every handle owned by the component behind this bundle view. */
    releaseAll(): void;
    /** Cancels queued load callbacks for the component behind this view. */
    cancelLoads(): void;
    release(): void;
}
