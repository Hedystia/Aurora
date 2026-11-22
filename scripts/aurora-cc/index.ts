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

export class Component {
  readonly node = new Node();

  onStart(): void {}
  onUpdate(_dt: number): void {}
  onFixedUpdate(_dt: number): void {}
}
