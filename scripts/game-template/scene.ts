import { Component, ccclass } from "aurora";

@ccclass('SceneDemo')
export class SceneDemo extends Component {
  onAnimationEvent(code: number): void {
    if (code === 7) this.director.changeScene(1);
    else if (code === 8) this.director.pause();
  }

  onKeyDown(_key: number): void {
    this.director.resume();
  }
}
