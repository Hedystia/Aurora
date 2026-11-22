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
export class Director {
  pause(): void;
  resume(): void;
  changeScene(index: number): void;
}
export class SceneCommands {
  move(node: number, x: number, y: number): void;
  show(node: number, visible: boolean): void;
  frame(node: number, frame: number): void;
  sound(clip: number): void;
  bestScore(score: number): void;
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
  onStart(): void;
  onUpdate(dt: number): void;
  onFixedUpdate(dt: number): void;
  onKeyDown(key: number): void;
  onTouchStart(): void;
  onBeginContact(other: Collider2D): void;
  onEndContact(other: Collider2D): void;
  onAnimationEvent(code: number): void;
  onBestScore(score: number): void;
}
