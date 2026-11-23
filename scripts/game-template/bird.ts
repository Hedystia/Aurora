import { Component, Collider2D, ccclass } from "aurora";

@ccclass('Bird')
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

  fixedUpdate(_dt: number): void {
    if (this.soundPending) {
      this.audio.playOneShot(0);
      this.soundPending = false;
    }
  }

  onBeginContact(_other: Collider2D): void {
    this.audio.playOneShot(1);
  }
}
