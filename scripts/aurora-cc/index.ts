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

export class Component {
  readonly node = new Node();
  readonly audio = new AudioSource();
  readonly body = new RigidBody2D();
  readonly director = new Director();

  onStart(): void {}
  onUpdate(_dt: number): void {}
  onFixedUpdate(_dt: number): void {}
  onKeyDown(_key: number): void {}
  onTouchStart(): void {}
  onBeginContact(_other: Collider2D): void {}
  onEndContact(_other: Collider2D): void {}
  onAnimationEvent(_code: number): void {}
}
