//! Scriptc-safe component and bound node API for the M2 runtime.

export interface Property {
  type: "number" | "boolean" | "string" | "node" | "prefab";
  tooltip?: string;
}

export interface ComponentMetadata {
  props: Record<string, Property>;
}

export function registerComponent(entry: [string, ComponentMetadata]): [string, ComponentMetadata] {
  return entry;
}

export class Node {
  x = 0;
  y = 0;
  changed = false;

  setPosition(x: number, y: number): void {
    this.x = x;
    this.y = y;
    this.changed = true;
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

export class Collider2D {
  readonly node: NodeHandle;
  constructor(node: NodeHandle) {
    this.node = node;
  }
}

export class AudioSource {
  pending = -1;

  playOneShot(index: number): void {
    if (!Number.isInteger(index) || index < 0 || index > 0xffffffff) {
      throw new Error("Invalid audio clip index");
    }
    this.pending = index;
  }
}

export class Director {
  action = 0;
  scene = 0;

  pause(): void { this.action = 1; }
  resume(): void { this.action = 2; }
  changeScene(index: number): void {
    if (!Number.isInteger(index) || index < 0 || index > 0xffffffff) {
      throw new Error("Invalid scene index");
    }
    this.scene = index;
    this.action = 3;
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

export class SceneCommands {
  edits: SceneEdit[] = [];
  cursor = 0;

  clear(): void {
    this.edits = [];
    this.cursor = 0;
  }

  private push(kind: number, target: number, x: number, y: number): void {
    if (!Number.isInteger(target) || target < 0 || target > 0xffffffff || this.edits.length >= 128) {
      throw new Error("Invalid scene command");
    }
    this.edits.push(new SceneEdit(kind, target, x, y));
  }

  move(node: number, x: number, y: number): void { this.push(0, node, x, y); }
  show(node: number, visible: boolean): void { this.push(1, node, visible ? 1 : 0, 0); }
  frame(node: number, frame: number): void { this.push(2, node, frame, 0); }
  sound(clip: number): void { this.push(3, clip, 0, 0); }
  bestScore(score: number): void { this.push(4, score, 0, 0); }
}

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

export class Component {
  readonly node = new Node();
  readonly audio = new AudioSource();
  readonly body = new RigidBody2D();
  readonly director = new Director();
  readonly scene = new SceneCommands();

  onStart(): void {}
  onUpdate(_dt: number): void {}
  onFixedUpdate(_dt: number): void {}
  onKeyDown(_key: number): void {}
  onTouchStart(): void {}
  onBeginContact(_other: Collider2D): void {}
  onEndContact(_other: Collider2D): void {}
  onAnimationEvent(_code: number): void {}
  onBestScore(_score: number): void {}
}
