import { describe, expect, test } from "vitest";

import {
  Component,
  NodeRef,
  Ref,
  TextRef,
  ImageRef,
  ScriptEvent,
  bindScriptScene,
  dispatchPointerEvent,
  advanceTweens,
  auroraClearTimer,
  auroraRequestAnimationFrame,
  auroraSetInterval,
  auroraSetTimeout,
  dispatchWheelEvent,
  registerComponentClass,
  registerComponentFactory,
  registerScriptResources,
  setScriptCallbackOwner,
  setScriptNodeScaleRotation,
  setScriptNodeTransform,
  property,
  tween,
} from "../aurora/index.ts";

function bind(
  components: Component[],
  owners: number[],
  names: string[],
  textContents: (string | null)[] = [],
): void {
  bindScriptScene(
    ["Root", "Child", "Other"],
    [-1, 0, 0],
    components,
    owners,
    names,
    [],
    textContents,
  );
}

describe("node events", () => {
  test("dispatch runs capture, target and bubble phases in order", () => {
    const root = new Component();
    const child = new Component();
    bind([root, child], [0, 1], ["RootScript", "ChildScript"]);
    const phases: string[] = [];

    root.node.on("hit", (event) => phases.push(`root-capture-${event.eventPhase}`), true);
    root.node.on("hit", (event) => phases.push(`root-bubble-${event.eventPhase}`));
    child.node.on("hit", (event) => phases.push(`child-${event.eventPhase}`));

    new NodeRef(1, root.scene).dispatchEvent(new ScriptEvent("hit", 1, true));

    expect(phases).toEqual([
      "root-capture-1",
      "child-2",
      "root-bubble-3",
    ]);
  });

  test("pointer delivery carries coordinates and the pointer identity", () => {
    const owner = new Component();
    bind([owner], [1], ["Owner"]);
    const seen: string[] = [];

    new NodeRef(1, owner.scene).on("touch-move", (event) => {
      seen.push(`${event.type}:${event.x},${event.y}:${event.pointerId}`);
    });

    dispatchPointerEvent(owner, 1, "touch-move", 0.25, 0.75, 4);

    expect(seen).toEqual(["touch-move:0.25,0.75:4"]);
  });

  test("node events carry the pointer position in node-local pixels", () => {
    const owner = new Component();
    bind([owner], [1], ["Owner"]);
    const seen: string[] = [];

    new NodeRef(1, owner.scene).on("touch-start", (event) => {
      seen.push(`touch:${event.x},${event.y}:${event.nodeX},${event.nodeY}`);
    });
    new NodeRef(1, owner.scene).on("mouse-down", (event) => {
      seen.push(`mouse:${event.x},${event.y}:${event.nodeX},${event.nodeY}`);
    });

    dispatchPointerEvent(owner, 1, "touch-start", 0.4, 0.6, 0, 12.5, -3);

    expect(seen).toEqual([
      "touch:0.4,0.6:12.5,-3",
      "mouse:0.4,0.6:12.5,-3",
    ]);
  });

  test("the mouse also emits mouse-specific events and wheel deltas", () => {
    const owner = new Component();
    bind([owner], [1], ["Owner"]);
    const seen: string[] = [];

    owner.input.on("mouse-move", (event) => seen.push(`move:${event.pointerId}`));
    new NodeRef(1, owner.scene).on("mouse-wheel", (event) => {
      seen.push(`wheel:${event.scrollX},${event.scrollY}`);
    });
    owner.node.on("mouse-wheel", (event) => seen.push(`node-wheel:${event.scrollY}`));
    owner._dispatchTouch("touch-move", 0.1, 0.2, 0);
    dispatchWheelEvent(owner, 1, 2, -3);

    expect(seen).toEqual(["move:0", "wheel:2,-3", "node-wheel:-3"]);
  });

  test("stopping propagation skips the remaining phases", () => {
    const root = new Component();
    const child = new Component();
    bind([root, child], [0, 1], ["RootScript", "ChildScript"]);
    const phases: string[] = [];

    root.node.on("hit", () => phases.push("capture"), true);
    root.node.on("hit", () => phases.push("bubble"));
    child.node.on("hit", (event) => {
      phases.push("child");
      event.stopPropagation();
    });

    new NodeRef(1, root.scene).dispatchEvent(new ScriptEvent("hit", 0, true));

    expect(phases).toEqual(["capture", "child"]);
  });

  test("nodes expose children and queue removal from the parent", () => {
    const root = new Component();
    bind([root], [0], ["Root"]);

    expect(root.node.children.map((child) => child.name)).toEqual(["Child", "Other"]);
    expect(root.node.childCount).toBe(2);

    expect(() => root.node.children[0].removeFromParent()).toThrow();
  });
});

describe("typed node reference markers", () => {
  test("Ref.Text exposes readable and writable authored text content", () => {
    class TextTarget extends Component {
      @property({ type: Ref.Node })
      nodeData: NodeRef | null = null;
      @property({
        type: Ref.Text,
      })
      label: TextRef | null = null;
      @property(Ref.Image)
      image: ImageRef | null = null;

      onStart(): void {
        const text = this.label;
        if (text !== null) {
          setTimeout(() => {
            text.content = "Updated";
          }, 5000);
        }
      }
    }

    const target = new TextTarget();
    expect(Ref.Node).toBe(NodeRef);
    expect(Ref.Text).toBe(TextRef);
    expect(Ref.Image).toBe(ImageRef);
    expect(target.label).toBeNull();
    bind([target], [0], ["TextTarget"], ["", "Authored text", null]);
    target.label = new TextRef(1, target.scene);
    expect(target.label.content).toBe("Authored text");
    target.label.content = "Updated text";
    expect(target.label.content).toBe("Updated text");
    expect(target.scene.edits.map((edit) => edit.kind)).toEqual([13, 14, 15]);
  });
});

describe("simulation timers", () => {
  test("timeout waits for fixed simulation time and forwards callback arguments", () => {
    const owner = new Component();
    bind([owner], [0], ["Timer"]);
    setScriptCallbackOwner(owner);
    const calls: string[] = [];

    auroraSetTimeout((value: string) => calls.push(value), 100, "fired");
    owner._tickScheduler(0.05);
    expect(calls).toEqual([]);
    owner._tickScheduler(0.05);
    expect(calls).toEqual(["fired"]);
    owner._tickScheduler(1);
    expect(calls).toEqual(["fired"]);
    setScriptCallbackOwner(null);
  });

  test("intervals can be canceled and animation frames report simulation time", () => {
    const owner = new Component();
    bind([owner], [0], ["Timer"]);
    setScriptCallbackOwner(owner);
    let intervalId = 0;
    let invocations = 0;
    intervalId = auroraSetInterval(() => {
      invocations++;
      if (invocations === 2) auroraClearTimer(intervalId);
    }, 100);
    let frameTime = -1;
    auroraRequestAnimationFrame((time) => { frameTime = time; });

    owner._tickScheduler(0.1);
    owner._tickScheduler(0.1);
    owner._tickScheduler(1);

    expect(invocations).toBe(2);
    expect(frameTime).toBe(100);
    setScriptCallbackOwner(null);
  });

  test("destroying a component clears its pending simulation timers", () => {
    const owner = new Component();
    bind([owner], [0], ["Timer"]);
    setScriptCallbackOwner(owner);
    let invoked = false;
    auroraSetTimeout(() => { invoked = true; }, 10);
    owner._clearScriptServices();
    owner._tickScheduler(1);
    expect(invoked).toBe(false);
    setScriptCallbackOwner(null);
  });

  test("timer handles are unique across components and can be canceled globally", () => {
    const first = new Component();
    const second = new Component();
    bind([first, second], [0, 1], ["First", "Second"]);
    setScriptCallbackOwner(first);
    let invoked = false;
    const handle = auroraSetTimeout(() => { invoked = true; }, 10);
    setScriptCallbackOwner(second);
    auroraClearTimer(handle);
    setScriptCallbackOwner(first);
    first._tickScheduler(1);
    expect(invoked).toBe(false);
    setScriptCallbackOwner(null);
  });
});

describe("component removal", () => {
  test("authored attachments disappear from lookup and queue removal", () => {
    const owner = new Component();
    const target = new Component();
    bind([owner, target], [1, 1], ["Owner", "Target"]);

    expect(owner.node.getComponent("Target")).toBe(target);

    owner.node.removeComponent(target);

    expect(owner.node.getComponent("Target")).toBeNull();
    expect(owner.scene.edits).toHaveLength(1);
    expect(owner.scene.edits[0].kind).toBe(10);
    expect(owner.scene.edits[0].target).toBe(1);
  });

  test("components on another node cannot be removed through this node", () => {
    const owner = new Component();
    const other = new Component();
    bind([owner, other], [1, 2], ["Owner", "Other"]);

    expect(() => owner.node.removeComponent(other)).toThrow();
  });

  test("classes registered for factories can be added by constructor", () => {
    class Mover extends Component {}
    registerComponentFactory("Mover", () => new Mover());
    registerComponentClass(Mover, "Mover");
    const owner = new Component();
    bind([owner], [1], ["Owner"]);

    owner.node.addComponent(Mover);

    expect(owner.scene.edits).toHaveLength(1);
    expect(owner.scene.edits[0].kind).toBe(9);
  });
});

describe("runtime text edits", () => {
  test("queues UTF-8 text in bounded start, chunk and end commands", () => {
    const owner = new Component();
    bind([owner], [0], ["Owner"], ["", ""]);
    const value = "A😊é";
    setScriptCallbackOwner(null);

    new NodeRef(1, owner.scene).setText(value);

    const bytes = new TextEncoder().encode(value);
    const [start, chunk, end] = owner.scene.edits;
    const packed = (source: Uint8Array): number => {
      let result = 0;
      for (let index = 0; index < 6; index++) {
        result += (source[index] ?? 0) * 2 ** (index * 8);
      }
      return result;
    };
    expect(owner.scene.edits.map((edit) => edit.kind)).toEqual([13, 14, 15]);
    expect(start.target).toBe(1);
    expect(chunk.x).toBe(packed(bytes.subarray(0, 6)));
    expect(chunk.y).toBe(packed(bytes.subarray(6, 12)) + bytes.length * 2 ** 48);
    expect(end.target).toBe(1);
  });

  test("rejects oversized or non-atomic replacements before queueing", () => {
    const owner = new Component();
    bind([owner], [0], ["Owner"], ["", ""]);
    setScriptCallbackOwner(null);

    new NodeRef(1, owner.scene).setText("a".repeat(1512));
    expect(owner.scene.edits).toHaveLength(128);
    expect(() => new NodeRef(1, owner.scene).setText("a".repeat(1513))).toThrow();

    const next = new Component();
    bind([next], [0], ["Next"], ["", ""]);
    for (let index = 0; index < 126; index++) next.scene.move(1, 0, 0);
    expect(() => new NodeRef(1, next.scene).setText("text")).toThrow();
    expect(next.scene.edits).toHaveLength(126);
  });
});

describe("resources", () => {
  test("releasing one component's handles keeps another component's handles", () => {
    registerScriptResources(["audio/confirm.wav"], [], [], []);
    const first = new Component();
    const second = new Component();

    const firstClip = first.resources.getAudioClip("audio/confirm.wav");
    const secondClip = second.resources.getAudioClip("audio/confirm.wav");
    expect(firstClip).not.toBeNull();
    expect(secondClip).not.toBeNull();

    first.resources.releaseAudioClip("audio/confirm.wav");

    expect(first.resources.getAudioClip("audio/confirm.wav")).not.toBe(firstClip);
    expect(second.resources.getAudioClip("audio/confirm.wav")).toBe(secondClip);
  });

  test("require methods return registered handles and throw for missing assets", () => {
    registerScriptResources(["audio/confirm.wav"], ["art/hero.png"], [3], [0]);
    const owner = new Component();

    const clip = owner.resources.requireAudioClip("audio/confirm.wav");
    expect(clip.path).toBe("audio/confirm.wav");
    expect(owner.resources.requireAudioClip("./audio/confirm.wav")).toBe(clip);
    expect(owner.resources.requireSpriteFrame("art/hero.png").frameFor(3)).toBe(0);
    expect(() => owner.resources.requireAudioClip("audio/missing.wav")).toThrow();
    expect(() => owner.resources.bundle("art").requireSpriteFrame("missing.png")).toThrow();
  });

  test("queued loads can be cancelled before they run", () => {
    registerScriptResources(["audio/confirm.wav"], [], [], []);
    const owner = new Component();
    let called = false;

    owner.resources.loadAudioClip("audio/confirm.wav", () => { called = true; });
    expect(owner.resources.pendingCount).toBe(1);

    owner.resources.cancelLoads();
    expect(owner.resources.pendingCount).toBe(0);
    owner.resources._advance();
    expect(called).toBe(false);
  });
});

describe("tweens", () => {
  test("interpolates cached node positions and finishes", () => {
    const owner = new Component();
    bind([owner], [0], ["Owner"]);
    setScriptNodeTransform(0, 10, 20);
    const target = new NodeRef(0, owner.scene);
    let calls = 0;

    tween(target).to(1, { x: 110, y: 20 }).call(() => { calls++; }).start();
    advanceTweens(0.5);

    expect(target.x).toBeCloseTo(60);
    expect(target.y).toBeCloseTo(20);

    advanceTweens(0.5);
    expect(target.x).toBeCloseTo(110);
    expect(calls).toBe(1);
  });

  test("by offsets from the current position and delays consume time", () => {
    const owner = new Component();
    bind([owner], [0], ["Owner"]);
    setScriptNodeTransform(0, 5, 5);
    const target = new NodeRef(0, owner.scene);

    tween(target).delay(0.5).by(0.5, { x: 10 }).start();
    advanceTweens(0.25);
    expect(target.x).toBe(5);

    advanceTweens(0.75);
    expect(target.x).toBe(15);
  });

  test("interpolates scale and rotation from the cached transform", () => {
    const owner = new Component();
    bind([owner], [0], ["Owner"]);
    setScriptNodeScaleRotation(0, 1, 1, 0);
    const target = new NodeRef(0, owner.scene);

    tween(target).to(1, { scale: { x: 3, y: 2 }, angle: 90 }).start();
    advanceTweens(0.5);

    expect(target.scaleX).toBeCloseTo(2);
    expect(target.scaleY).toBeCloseTo(1.5);
    expect(target.angle).toBeCloseTo(45);

    advanceTweens(0.5);
    expect(target.scaleX).toBeCloseTo(3);
    expect(target.angle).toBeCloseTo(90);
  });

  test("node scale and angle setters cache and queue one edit each", () => {
    const owner = new Component();
    bind([owner], [0], ["Owner"]);
    const target = new NodeRef(0, owner.scene);

    target.setScale(2, 4);
    target.setAngle(30);

    expect(target.scaleX).toBe(2);
    expect(target.scaleY).toBe(4);
    expect(target.angle).toBeCloseTo(30);
    expect(owner.scene.edits.map((edit) => edit.kind)).toEqual([11, 12]);
  });
});
