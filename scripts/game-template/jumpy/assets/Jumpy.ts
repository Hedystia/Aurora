import { Component, Collider2D, NodePool, registerComponent } from "../../../aurora-cc/index.ts";

const birdX = -145;
const speed = 115;
const groundWidth = 256;

export class Jumpy extends Component {
  birdNode = 5;
  startScreen = 7;
  endScreen = 8;
  firstGround = 2;
  scoreDigits = 9;
  bestDigits = 12;

  private pool = new NodePool([18, 21, 24, 27]);
  private slots: number[] = [];
  private pipeX: number[] = [];
  private pipeY: number[] = [];
  private counted: boolean[] = [];
  private ground = [-320, -64, 192];
  private phase = 0;
  private y = 0;
  private velocity = 0;
  private countdown = 0;
  private pipeCount = 0;
  private score = 0;
  private best = 0;
  private diePending = false;

  onBestScore(score: number): void {
    this.best = score;
  }

  onStart(): void {
    this.scene.show(this.startScreen, true);
    this.scene.show(this.endScreen, false);
    this.digits(this.scoreDigits, this.score);
    this.digits(this.bestDigits, this.best);
  }

  onKeyDown(_key: number): void { this.press(); }
  onTouchStart(): void { this.press(); }

  private press(): void {
    if (this.phase !== 1) {
      for (const slot of this.slots) this.pool.put(this.scene, slot);
      this.slots = [];
      this.pipeX = [];
      this.pipeY = [];
      this.counted = [];
      this.phase = 1;
      this.y = 0;
      this.countdown = 0;
      this.pipeCount = 0;
      this.score = 0;
      this.diePending = false;
      this.scene.move(this.birdNode, birdX, this.y);
      this.scene.show(this.startScreen, false);
      this.scene.show(this.endScreen, false);
      this.digits(this.scoreDigits, this.score);
    }
    this.velocity = 164;
    this.scene.sound(0);
  }

  onUpdate(dt: number): void {
    if (this.phase === 2) {
      if (this.diePending) {
        this.scene.sound(3);
        this.diePending = false;
      }
      return;
    }
    for (let i = 0; i < this.ground.length; i++) {
      let x = this.ground[i] - speed * dt;
      if (x < -320 - groundWidth) x += this.ground.length * groundWidth;
      this.ground[i] = x;
      this.scene.move(this.firstGround + i, x, -184);
    }
    if (this.phase !== 1) return;
    this.velocity -= 370 * dt;
    this.y += this.velocity * dt;
    if (this.y > 164) {
      this.y = 164;
      this.velocity = 0;
    }
    this.scene.move(this.birdNode, birdX, this.y);

    if (this.countdown === 0) {
      const slot = this.pool.get();
      if (slot >= 0) {
        const y = 14 + (this.pipeCount % 3) * 8;
        this.pipeCount++;
        this.pool.reset(this.scene, slot, 355, y);
        this.slots.push(slot);
        this.pipeX.push(355);
        this.pipeY.push(y);
        this.counted.push(false);
      }
      this.countdown = 116;
    }
    this.countdown--;
    for (let i = this.slots.length - 1; i >= 0; i--) {
      const x = this.pipeX[i] - speed * dt;
      if (x < -355) {
        this.pool.put(this.scene, this.slots[i]);
        this.slots.splice(i, 1);
        this.pipeX.splice(i, 1);
        this.pipeY.splice(i, 1);
        this.counted.splice(i, 1);
      } else {
        if (!this.counted[i] && x < birdX - 42) {
          this.counted[i] = true;
          this.score++;
          this.digits(this.scoreDigits, this.score);
          this.scene.sound(1);
          if (this.score > this.best) {
            this.best = this.score;
            this.digits(this.bestDigits, this.best);
            this.scene.bestScore(this.best);
          }
        }
        this.pipeX[i] = x;
        this.scene.move(this.slots[i], x, this.pipeY[i]);
      }
    }
  }

  onBeginContact(_other: Collider2D): void {
    if (this.phase !== 1) return;
    this.phase = 2;
    this.diePending = true;
    this.scene.show(this.endScreen, true);
    this.scene.sound(2);
  }

  private digits(start: number, score: number): void {
    this.scene.frame(start, Math.floor(score / 100) % 10);
    this.scene.frame(start + 1, Math.floor(score / 10) % 10);
    this.scene.frame(start + 2, score % 10);
  }
}

export const metadata =
registerComponent(["Jumpy", { "props": {
  "birdNode": { "type": "node", "tooltip": "Bird visual and bound contact sensor." },
  "startScreen": { "type": "node", "tooltip": "Ready overlay." },
  "endScreen": { "type": "node", "tooltip": "Retry overlay." },
  "firstGround": { "type": "node", "tooltip": "First of the three consecutive ground segments." },
  "scoreDigits": { "type": "node", "tooltip": "First of the three consecutive score digits." },
  "bestDigits": { "type": "node", "tooltip": "First of the three consecutive best-score digits." }
} }]);
