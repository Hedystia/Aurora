import { Component, ccclass, property } from "aurora";

@ccclass('Hello')
export class Hello extends Component {
  @property({ type: Number, tooltip: 'Local pixels per second.' })
  speed = 30;

  onLoad(): void {
    this.node.setPosition(8, 0);
  }

  start(): void {
    this.node.setPosition(this.node.x * 2, this.node.y);
  }

  update(dt: number): void {
    this.node.setPosition(this.node.x + this.speed * dt, this.node.y);
  }
}
