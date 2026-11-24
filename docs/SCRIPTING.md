# Scripting in Aurora

Create a TypeScript component from Assets, then drag it onto a scene node or choose Add Component in the Inspector. New scripts import their API from `aurora`. Aurora uses directly imported decorators, following the style used in [Cocos 4's component source](https://github.com/cocos/cocos4/blob/v4.0.0/cocos/scene-graph/component.ts). See the [Cocos Creator authoring guide](https://docs.cocos.com/creator/3.8/manual/en/scripting/decorator.html) for the original workflow.

```ts
import { Component, NodeRef, ccclass, property } from "aurora";

@ccclass("Follower")
export default class Follower extends Component {
  @property({ type: Number, tooltip: "Horizontal speed in pixels per second." })
  speed = 60;

  @property(NodeRef)
  target: NodeRef | null = null;

  private distance = 0;

  update(dt: number): void {
    this.distance += this.speed * dt;
    this.target?.setPosition(this.distance, 0);
  }
}
```

- `@ccclass` uses the class name by default. A literal name gives the component a distinct Inspector label. Named and default exports are supported.
- `@property` infers numbers, booleans, and strings from scalar declarations or initializers. It also supports `Integer`, multiline `Text`, `Vector2`, `Vector3`, `Size`, `Rect`, `Color`, `Bitmask`, and `Image` markers; enum choices use `@property({ enum: ["idle", "run"] })`, and `readonly: true` displays a value without allowing edits. Numeric properties accept `min`, `max`, `step`, `slider`, and `group` options; sliders require both bounds. `Size` dimensions must be nonnegative; vector and rectangle coordinates may be negative. `Bitmask` stores an unsigned 32-bit mask and displays its hexadecimal value on hover. `Text` is a multiline string editor. To reference an authored Text node instead, use `@property({ type: Ref.Text }) label: TextRef | null = null`; this uses the scene-node picker and exposes readable/writable `label.content`. `Ref.Node` selects any scene node, while `Ref.Image` selects sprite, atlas, tile-map, and nine-slice nodes and supports `setSpriteFrame`. `Ref` also includes `Prefab`, `AudioClip`, and `SpriteFrame` asset references. Use the reference value type (`NodeRef`, `TextRef`, or `ImageRef`) rather than `typeof Ref.*`, which describes the marker constructor. `Image` and `SpriteFrame` fields pick project PNG assets and register selections for the node's runtime frame bank. Audio properties list the scene's registered sounds. Arrays use `@property([Number])`, `@property([Boolean])`, `@property([String])`, `@property([NodeRef])`, `@property([Prefab])`, `@property([AudioClip])`, or `@property([SpriteFrame])`, or the same element type written as `number[]`, `boolean[]`, `string[]`, `NodeRef[]`, `Prefab[]`, `AudioClip[]`, and `SpriteFrame[]`. Drop a matching scene node or prefab asset into the Inspector; array fields expose add and remove controls and accept node drops; asset arrays add and remove entries from the same registered lists as scalar asset fields. `NodeRef` exposes its parent and children, and can update local transforms, visibility, and Text content. Runtime reparenting remains limited to script-created nodes. `Prefab` identifies an asset by path.

For typed scene references, the decorator marker and the field's value type are separate: write `type: Ref.Text` with `text: TextRef | null`, and `type: Ref.Node` with `node: NodeRef | null`. Do not use `typeof Ref.Text` or `typeof Ref.Node` for the field annotation; those are constructor types and do not have the instance API such as `content`.
- Inspector overrides are applied before startup. Aurora runs `onLoad()`, `onEnable()`, and `start()` as a component becomes active. It delivers `onDisable()` when an enabled component or its node becomes inactive, and `onDestroy()` on scene replacement or graceful stop. Active components receive `fixedUpdate(dt)`, `update(dt)`, scheduled callbacks, resource callbacks, and `lateUpdate(dt)`. A phase finishes on every attachment before the next phase starts. Aurora's current tick interval is 1/60 second. The [Cocos 4 scheduler](https://github.com/cocos/cocos4/blob/v4.0.0/cocos/scene-graph/component-scheduler.ts) is the reference for separating update phases.
- The editor extracts the annotations from TypeScript syntax without executing game code. Play bundles the script for Bun or Node. Native builds remove Aurora's annotations from staged sources and compile with scriptc in static-only mode.
- The project editor's standalone TypeScript declarations are generated from the SDK with `tools/generate-sdk-types.sh`; installing or reopening a project refreshes those declarations.
- Existing projects may keep the old package alias, `registerComponent` metadata, and `onStart` / `onUpdate` / `onFixedUpdate`. Their reference fields still receive indices and paths. Add `aurora` to the paths in a custom tsconfig to adopt the new imports, and enable `experimentalDecorators`.


## Scheduling, events, and scene APIs

- `schedule(callback, interval, repeat, delay)` advances on simulation time. A finite repeat value counts additional calls after the first. `scheduleOnce`, `unschedule`, and `unscheduleAllCallbacks` manage the component's own timers.
- Global `setTimeout`, `setInterval`, `clearTimeout`, and `clearInterval` calls, plus `requestAnimationFrame` and `cancelAnimationFrame`, are routed to the component whose callback is running. Delays use milliseconds, callbacks run at fixed-tick boundaries, and timers pause with simulation time and are cleared when their component is destroyed. Animation-frame timestamps are elapsed simulation milliseconds; callbacks do not run while the game is paused.
- Subscribe from a component callback with `input.on(InputEventType.KEY_DOWN, callback)` or use `this.input` directly. Keyboard presses and releases include letters, digits, arrows, Space, Enter, Escape, and Backspace. The event exposes `keyCode` and the SDK exports matching `KeyCode` constants. `InputEventType` also exposes `KEY_UP`, `TOUCH_START`, `TOUCH_MOVE`, `TOUCH_END`, `TOUCH_CANCEL`, `MOUSE_DOWN`, `MOUSE_MOVE`, `MOUSE_UP`, and `MOUSE_WHEEL`.
- Pointer contacts are tracked per identity. A press hit-tests the topmost visible sprite and captures it; movement, release, and cancellation continue to target that node even if the pointer leaves it. The mouse uses the same path as one finger, and losing window focus cancels the contact. Every phase reaches global input subscriptions, and node listeners receive the phase through capture, target, and bubbling. Event x and y are normalized from the top-left of the game frame, and `pointerId` is zero for the mouse and one-based per touch. Node listeners also receive `nodeX` and `nodeY`, the same point in the target node's local pixels, which is what a touch on the sprite itself should use. Mouse contacts also emit the mouse-specific event names, including on node listeners, so `MOUSE_DOWN` and `TOUCH_START` both observe a mouse press. `MOUSE_WHEEL` events expose `scrollX` and `scrollY`; the wheel hit-tests the cursor and bubbles through the node hierarchy. A coordinate-free touch becomes a centre-frame tap.
- `this.events` supports local subscriptions and numeric payloads. `node.dispatchEvent(new ScriptEvent("hit", 1, true))` traverses the node hierarchy through capture, target, and bubbling. Use `node.on(EventType, handler, true)` for capture. `node.emit(type, value)` stays on the target.

```ts
import { Component, InputEventType } from "aurora";

start(): void {
  this.node.on(InputEventType.TOUCH_MOVE, (event) => {
    this.node.setPosition(event.x * 640 - 320, event.y * 384 - 192);
  });
}
```
- `getComponent("ComponentName")` and `getComponent(ComponentClass)` find a component on a node, and `getComponents` returns all matches. Relative lookup uses `getChildByPath("Rig/Hand")`. `@executionOrder(-10)` sets priority for authored attachments, with ties retaining authoring order.
- `this.scene.loadPrefab("prefabs/Projectile.prefab")` returns a registered prefab or `null`. `this.scene.instantiate(prefab, parent?)` and `this.scene.createNode(parent?)` return a `NodeRef` that can be moved, reparented, or destroyed. In editor-generated and exported scripts, prefab clones construct their compiled attachments with independent state and prefab-local Inspector references. Dynamic bindings follow prefab attachment order. Destruction tears down the clone's component bindings.


## Tweens

`tween(node)` accepts a `NodeRef` or a bound component `Node` and returns a chainable tween. `to` and `by` take any combination of `x`, `y`, `position: { x, y }`, `scaleX`, `scaleY`, `scale: { x, y }`, and `angle`, so one step can move, scale, and rotate together. Chains support `delay`, `call`, `repeat`, `repeatForever`, `start`, and `stop`, with `linear`, `quad*`, and `cubic*` easing. Angles are in degrees and match `NodeRef.angle`. Tweens advance once per fixed simulation tick, after `update` and before scheduled callbacks. Arbitrary property tweens such as opacity or color are not implemented yet.

```ts
tween(this.node)
  .to(0.4, { x: 0, y: 120 }, { easing: "quadOut" })
  .call(() => console.log("landed"))
  .start();
```

`NodeRef` and a bound `Node` expose cached local `x`, `y`, `scaleX`, `scaleY`, and `angle`, with `setPosition`, `setScale`, and `setAngle` writing through the scene command queue. The player streams authored transforms at scene start and picks up prefab-created nodes on a later tick. Script writes keep the cache current; movement produced by physics, animation, or the player without a script command can leave the cached start value stale.

`NodeRef.setText(value)` and `Node.setText(value)` replace the content of an authored Text visual at runtime. Rich Text replacement keeps the first run's style and replaces all remaining runs with the new content. The UTF-8 content limit is 1512 bytes per edit.

## Resources

`this.resources` resolves scene-registered prefabs, audio clips, and sprite frames by project path. `requireAudioClip` and `requireSpriteFrame` return a non-null handle or throw, which is what generated asset properties use. Under `assets/resources/` or `resources/`, an unambiguous extensionless name also works. Use `getPrefab`, `getAudioClip`, or `getSpriteFrame` for an immediate lookup. Their `load` counterparts deliver one callback per active component resource phase. Missing or ambiguous resources reach the callback as an error and a null asset. `preload*` warms the same component-local handle cache and reports the resolved handle through the same callback shape. `cancelLoads()` drops queued callbacks that have not run, and `pendingCount` reports how many remain.

Each component owns the handles it resolves. `releaseAudioClip` and `releaseSpriteFrame` drop only that component's handle, so another component continues to use its own. `releaseAll()` clears every handle owned by the component, and teardown does the same automatically. The player keeps the scene-owned asset data; there is no general on-demand importer yet.

```ts
this.resources.loadAudioClip("audio/confirm.wav", (error, clip) => {
  if (error === null && clip !== null) this.audio.playClip(clip);
});
```

A directory-scoped view is available with `const bundle = this.resources.bundle("assets/resources/ui")`. Its load and preload methods accept paths relative to that directory. `releaseAudioClip`, `releaseSpriteFrame`, `releaseAll`, and `cancelLoads` discard script handles or queued callbacks, and `release` closes the view. A new scene requires a new bundle view. Existing handles refer to the player-owned scene banks.

Audio uses the scene's sound bank. `scene.setSpriteFrame(node, frame)` selects a frame from the bank authored for that node. The script receives handles to these resources, and the player owns loading and decoding asset data. The resource naming and callback pattern draws on [Cocos 4's bundle loader](https://github.com/cocos/cocos4/blob/v4.0.0/cocos/asset/asset-manager/bundle.ts).

## Native scene export

```bash
aurora-editor --project ./MyGame \
  --include-scene ./MyGame/scenes/Level2.scene \
  --export ./MyGameLinux
aurora-player --bundle ./MyGameLinux
```

The destination must be a new directory. Repeat `--include-scene` for additional scenes inside the project. The project scene starts first. Scene filenames determine the names used by `this.director.loadScene("Level2")`. The player starts each scene's native script child with its own bindings and resources. Single-scene exports and existing bundle layouts still load.

Aurora stages project-local TypeScript, lowers its decorators, and compiles with `scriptc --tier static-only`. Play a bundle with a matching Linux Aurora player. Third-party package resolution and other platform targets remain separate distribution work.

Call `node.addComponent("Mover")` or pass the component class to request a component by its registered name. Editor and export entries register each compiled component class with its factory, so `node.addComponent(Mover)` resolves the same factory. The new component becomes available through `getComponent` after the current callback completes. `node.removeComponent(component)` removes authored attachments, dynamically added components, and prefab-created components after their disable and destroy callbacks. A removed authored attachment disappears from `getComponent` and `getComponents` immediately, even though the native script keeps its launch list. Editor and export builds register uniquely named components found in project assets.

Every scene starts with a reset of script-side dynamic nodes, removed-binding markers, and node listeners, so explicit CLI scripts can reuse their instances across scenes. Native bundles still start a scene-local process per scene.

`@property` covers numbers, booleans, strings, node references, prefab references, audio clip and sprite frame references, component references, and arrays of numbers, booleans, strings, node references, prefabs, audio clips, and sprite frames. Declare a component reference with the component class, as in `@property(Health) target: Health | null`. The Inspector lists components in the same document, and the generated entry assigns the live instance, so prefab-internal references resolve within each clone. Audio clip and sprite frame properties select from the scene's sound and frame banks; the generated entry resolves the registered handle before Start. An asset property whose path is not registered stays at the field default.

Remaining Cocos scripting differences include gesture recognizers, general on-demand asset import with reference-counted memory release, arbitrary property tweens, and `addComponent` returning the new instance synchronously. Aurora resolves the new component through `getComponent` after the callback.
