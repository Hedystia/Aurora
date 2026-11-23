import { Component, Collider2D, NodePool, ccclass, property, NodeRef } from "aurora";

const birdX = -145;
const speed = 115;
const groundWidth = 256;

@ccclass('Jumpy')
export class Jumpy extends Component {
  @property({ type: NodeRef, tooltip: 'Bird visual and contact sensor.' })
  birdNode = new NodeRef(5, this.scene);
  @property({ type: NodeRef, tooltip: 'Ready overlay.' })
  startScreen = new NodeRef(7, this.scene);
  @property({ type: NodeRef, tooltip: 'Retry overlay.' })
  endScreen = new NodeRef(8, this.scene);
  @property({ type: NodeRef, tooltip: 'First of the three consecutive ground segments.' })
  firstGround = new NodeRef(2, this.scene);
  @property({ type: NodeRef, tooltip: 'First of the three consecutive score digits.' })
  scoreDigits = new NodeRef(9, this.scene);
  @property({ type: NodeRef, tooltip: 'First of the three consecutive best-score digits.' })
  bestDigits = new NodeRef(12, this.scene);

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

  start(): void {
    this.startScreen.setActive(true);
    this.endScreen.setActive(false);
    this.digits(this.scoreDigits.index, this.score);
    this.digits(this.bestDigits.index, this.best);
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
      this.birdNode.setPosition(birdX, this.y);
      this.startScreen.setActive(false);
      this.endScreen.setActive(false);
      this.digits(this.scoreDigits.index, this.score);
    }
    this.velocity = 164;
    this.scene.sound(0);
  }

  update(dt: number): void {
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
      this.scene.move(this.firstGround.index + i, x, -184);
    }
    if (this.phase !== 1) return;
    this.velocity -= 370 * dt;
    this.y += this.velocity * dt;
    if (this.y > 164) {
      this.y = 164;
      this.velocity = 0;
    }
    this.birdNode.setPosition(birdX, this.y);

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
          this.digits(this.scoreDigits.index, this.score);
          this.scene.sound(1);
          if (this.score > this.best) {
            this.best = this.score;
            this.digits(this.bestDigits.index, this.best);
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
    this.endScreen.setActive(true);
    this.scene.sound(2);
  }

  private digits(start: number, score: number): void {
    this.scene.frame(start, Math.floor(score / 100) % 10);
    this.scene.frame(start + 1, Math.floor(score / 10) % 10);
    this.scene.frame(start + 2, score % 10);
  }
}
