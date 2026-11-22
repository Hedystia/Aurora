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
export class Component {
  readonly node: Node;
  onStart(): void;
  onUpdate(dt: number): void;
  onFixedUpdate(dt: number): void;
}
