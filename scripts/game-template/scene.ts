import { Component, registerComponent } from "../aurora-cc/index.ts";

export class SceneDemo extends Component {
  onAnimationEvent(code: number): void {
    if (code === 7) this.director.changeScene(1);
    else if (code === 8) this.director.pause();
  }

  onKeyDown(_key: number): void {
    this.director.resume();
  }
}

export const metadata =
registerComponent(["SceneDemo", {
  "props": {}
}]);
