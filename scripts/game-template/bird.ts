import { Component, Collider2D, registerComponent } from "../aurora-cc/index.ts";

export class Bird extends Component {
  private soundPending = false;

  private flap(): void {
    this.body.setLinearVelocity(0, 5.8);
    this.soundPending = true;
  }

  onKeyDown(key: number): void {
    if (key === 1 || key === 2) this.flap();
  }

  onTouchStart(): void {
    this.flap();
  }

  onFixedUpdate(_dt: number): void {
    if (this.soundPending) {
      this.audio.playOneShot(0);
      this.soundPending = false;
    }
  }

  onBeginContact(_other: Collider2D): void {
    this.audio.playOneShot(1);
  }
}

export const metadata =
registerComponent(["Bird", {
  "props": {}
}]);
