import { Component, registerComponent } from "../aurora-cc/index.ts";

export class Hello extends Component {
  speed = 30;

  onStart(): void {
    this.node.setPosition(16, 0);
  }

  onUpdate(dt: number): void {
    this.node.setPosition(this.node.x + this.speed * dt, this.node.y);
  }
}

export const metadata =
registerComponent(["Hello", {
  "props": {
    "speed": { "type": "number", "tooltip": "Local pixels per second." }
  }
}]);
