export interface Property {
  type: "number" | "boolean" | "string" | "node" | "prefab";
  tooltip?: string;
}
export interface ComponentMetadata {
  props: Record<string, Property>;
}
export function registerComponent(entry: [string, ComponentMetadata]): [string, ComponentMetadata];
export class Node {
  x: number;
  y: number;
  setPosition(x: number, y: number): void;
}
export class NodeHandle {
  readonly low: number;
  readonly high: number;
}
export class RigidBody2D {
  setLinearVelocity(x: number, y: number): void;
}
export class Collider2D {
  readonly node: NodeHandle;
}
export class AudioSource {
  playOneShot(index: number): void;
}
export class Component {
  readonly node: Node;
  readonly audio: AudioSource;
  readonly body: RigidBody2D;
  onStart(): void;
  onUpdate(dt: number): void;
  onFixedUpdate(dt: number): void;
  onKeyDown(key: number): void;
  onTouchStart(): void;
  onBeginContact(other: Collider2D): void;
  onEndContact(other: Collider2D): void;
}
