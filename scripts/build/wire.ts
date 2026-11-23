import { frameBytes, protocol, kindStart, kindUpdate, kindFixedUpdate, kindAck, kindSetPosition, kindCollisionEnter, kindCollisionExit, kindKeyDown, kindKeyUp, kindTouchStart, kindSetLinearVelocity, kindPlayOneShot, kindAnimationEvent, kindPause, kindResume, kindChangeScene, kindFlushCommands, kindMoveNode, kindShowNode, kindSelectFrame, kindQueueSound, kindRecordScore, kindRestoreScore, kindLoad, kindLateUpdate, kindEnable, kindDisable, kindDestroy, kindSchedule, kindPollEnabled, kindProcessResources, kindPointerTouch, kindPointerMove, kindPointerEnd, kindPointerCancel, kindPointerNodeMove, kindPointerNodeEnd, kindPointerNodeCancel, kindSceneStart, kindMouseWheel, kindPointerNodeWheel, kindNodeTransform, kindNodeScaleRotation, kindTween, kindScaleNode, kindRotateNode, bindingEnabled } from "../../.aurora/scripts/metadata/protocol.mts";
import { Component, Collider2D, InputEventType, NodeHandle, advanceTweens, setScriptCallbackOwner, setScriptNodeTransform, setScriptNodeScaleRotation, dispatchPointerEvent, dispatchWheelEvent, resetScriptScene } from "../aurora/index.ts";
import { kindQueueCreateNode, kindQueueInstantiatePrefab, kindQueueReparentNode, kindQueueDestroyNode, kindQueueAddComponent, kindQueueRemoveComponent, kindRegisterComponent } from "../../.aurora/scripts/metadata/protocol.mts";

import { kindRegisterPrefabNode, kindRegisterPrefabComponent, kindReleasePrefabNode, kindReleasePrefabComponent, kindUpdatePrefabParent } from "../../.aurora/scripts/metadata/protocol.mts";
import { registerRuntimeComponent, prefabComponent, registerPrefabNode, registerPrefabComponent, releasePrefabNode, releasePrefabComponent, updatePrefabParent } from "../aurora/index.ts";

// The record layout is bincode v2 fixed-width little-endian (docs/PROTOCOL.md).
const fixedDt = 1 / 60;

// Last normalized position for each pointer identity. Node transactions carry
// local coordinates, so the normalized values are reused for the event fields.
const pointerX = new Map<number, number>();
const pointerY = new Map<number, number>();

/// An immediate reply held back until a callback's queued edits drain.
interface Immediate {
  kind: number;
  detail: number;
  x: number;
  y: number;
}

let pendingImmediate: Immediate | null = null;

export function run(
  components: Component[],
  receive: (bytes: Uint8Array) => number,
  reply: (bytes: Uint8Array) => number,
): void {
  const bytes = new Uint8Array(frameBytes);
  const view = new DataView(bytes.buffer);
  while (true) {
    const status = receive(bytes);
    if (status === 0) return;
    if (status !== 1 || view.getUint32(0, true) !== protocol) {
      throw new Error("Invalid Aurora script frame");
    }
    const binding = view.getUint32(48, true);
    const kind = view.getUint32(12, true);
    if (kind === kindNodeTransform) {
      setScriptNodeTransform(view.getUint32(40, true), view.getFloat64(24, true), view.getFloat64(32, true));
      view.setUint32(12, kindAck, true);
      view.setUint32(48, binding + bindingEnabled, true);
      if (reply(bytes) !== 1) throw new Error("Aurora transform reply failed");
      continue;
    }
    if (kind === kindNodeScaleRotation) {
      const packed = view.getUint32(44, true);
      const angle = new DataView(new ArrayBuffer(4));
      angle.setUint32(0, packed, true);
      setScriptNodeScaleRotation(
        view.getUint32(40, true),
        view.getFloat64(24, true),
        view.getFloat64(32, true),
        angle.getFloat32(0, true),
      );
      view.setUint32(12, kindAck, true);
      view.setUint32(48, binding + bindingEnabled, true);
      if (reply(bytes) !== 1) throw new Error("Aurora transform reply failed");
      continue;
    }
    if (kind === kindSceneStart) {
      resetScriptScene();
      view.setUint32(12, kindAck, true);
      view.setUint32(48, binding + bindingEnabled, true);
      if (reply(bytes) !== 1) throw new Error("Aurora scene reply failed");
      continue;
    }
    if (kind === kindRegisterComponent) {
      const enabled = registerRuntimeComponent(binding, view.getFloat64(24, true), view.getFloat64(32, true)).enabled;
      view.setUint32(12, kindAck, true);
      view.setUint32(48, enabled ? binding + bindingEnabled : binding, true);
      if (reply(bytes) !== 1) throw new Error("Aurora component reply failed");
      continue;
    }
    if (kind >= kindRegisterPrefabNode && kind <= kindUpdatePrefabParent) {
      const low = view.getUint32(40, true);
      const high = view.getUint32(44, true);
      const x = view.getFloat64(24, true);
      const y = view.getFloat64(32, true);
      let enabled = false;
      if (kind === kindRegisterPrefabNode) registerPrefabNode(low, high, x, y);
      else if (kind === kindRegisterPrefabComponent) {
        enabled = registerPrefabComponent(binding, low, high, x, y).enabled;
      } else if (kind === kindReleasePrefabNode) releasePrefabNode(low);
      else if (kind === kindReleasePrefabComponent) releasePrefabComponent(binding);
      else updatePrefabParent(low, x);

      view.setUint32(12, kindAck, true);
      view.setUint32(48, enabled ? binding + bindingEnabled : binding, true);
      if (reply(bytes) !== 1) throw new Error("Aurora prefab reply failed");
      continue;
    }
    const component = binding < components.length ? components[binding] : prefabComponent(binding);
    if (component === null) throw new Error("Invalid Aurora component binding");
    const node = component.node;
    // Pointer transactions carry normalized pointer coordinates in the position
    // slots, so they must not overwrite the cached node transform.
    const pointer = kind === kindTouchStart || kind === kindPointerMove || kind === kindPointerEnd
      || kind === kindPointerCancel || kind === kindPointerTouch || kind === kindPointerNodeMove
      || kind === kindPointerNodeEnd || kind === kindPointerNodeCancel
      || kind === kindMouseWheel || kind === kindPointerNodeWheel;
    if (!pointer) {
      node.x = view.getFloat64(24, true);
      node.y = view.getFloat64(32, true);
    }
    node.changed = false;
    const body = component.body;
    body.changed = false;
    const audio = component.audio;
    audio.pending = -1;
    const director = component.director;
    director.action = 0;
    const scene = component.scene;
    if (kind !== kindFlushCommands) scene.clear();
    setScriptCallbackOwner(component);
    if (kind === kindFlushCommands) { /* Drain the existing queue. */ }
    else if (kind === kindRestoreScore) component.onBestScore(view.getUint32(40, true));
    else if (kind === kindLoad) component.onLoad();
    else if (kind === kindEnable) {
      component._setLifecycleActive(true);
      if (component.enabled) component.onEnable();
    } else if (kind === kindDisable) {
      component._setLifecycleActive(false);
      component.onDisable();
    } else if (kind === kindDestroy) {
      component._setLifecycleActive(false);
      component.onDestroy();
      component._clearScriptServices();
    } else if (kind === kindPollEnabled) { /* Report enabled in the reply. */ }
    else if (component.enabledInHierarchy) {
      if (kind === kindStart) component.start();
      else if (kind === kindUpdate) component.update(fixedDt);
      else if (kind === kindFixedUpdate) component.fixedUpdate(fixedDt);
      else if (kind === kindLateUpdate) component.lateUpdate(fixedDt);
      else if (kind === kindSchedule) component._tickScheduler(fixedDt);
      else if (kind === kindProcessResources) component._tickResources();
      else if (kind === kindKeyDown) component._dispatchKeyDown(view.getUint32(40, true));
      else if (kind === kindKeyUp) component._dispatchKeyUp(view.getUint32(40, true));
      else if (kind === kindTouchStart) {
        const x = view.getFloat64(24, true);
        const y = view.getFloat64(32, true);
        const pointerId = view.getUint32(40, true);
        pointerX.set(pointerId, x);
        pointerY.set(pointerId, y);
        component._dispatchTouchStart(x, y, pointerId);
      }
      else if (kind === kindPointerMove || kind === kindPointerEnd || kind === kindPointerCancel) {
        const x = view.getFloat64(24, true);
        const y = view.getFloat64(32, true);
        const pointerId = view.getUint32(40, true);
        pointerX.set(pointerId, x);
        pointerY.set(pointerId, y);
        const phase = kind === kindPointerMove ? InputEventType.TOUCH_MOVE
          : kind === kindPointerEnd ? InputEventType.TOUCH_END : InputEventType.TOUCH_CANCEL;
        component._dispatchTouch(phase, x, y, pointerId);
      }
      else if (kind === kindPointerTouch || kind === kindPointerNodeMove
               || kind === kindPointerNodeEnd || kind === kindPointerNodeCancel) {
        const reference = view.getUint32(40, true);
        const pointerId = view.getUint32(44, true);
        const phase = kind === kindPointerTouch ? InputEventType.TOUCH_START
          : kind === kindPointerNodeMove ? InputEventType.TOUCH_MOVE
          : kind === kindPointerNodeEnd ? InputEventType.TOUCH_END : InputEventType.TOUCH_CANCEL;
        dispatchPointerEvent(
          component,
          reference,
          phase,
          pointerX.get(pointerId) ?? 0,
          pointerY.get(pointerId) ?? 0,
          pointerId,
          view.getFloat64(24, true),
          view.getFloat64(32, true),
        );
      }
      else if (kind === kindTween) advanceTweens(fixedDt);
      else if (kind === kindMouseWheel) {
        component._dispatchWheel(view.getFloat64(24, true), view.getFloat64(32, true));
      }
      else if (kind === kindPointerNodeWheel) {
        dispatchWheelEvent(component, view.getUint32(40, true), view.getFloat64(24, true), view.getFloat64(32, true));
      }
      else if (kind === kindAnimationEvent) component.onAnimationEvent(view.getUint32(40, true));
      else if (kind === kindCollisionEnter || kind === kindCollisionExit) {
        const other = new Collider2D(new NodeHandle(view.getUint32(40, true), view.getUint32(44, true)));
        if (kind === kindCollisionEnter) component.onBeginContact(other);
        else component.onEndContact(other);
      } else throw new Error("Invalid Aurora script event");
    }
    setScriptCallbackOwner(null);
    view.setUint32(48, component.enabled ? binding + bindingEnabled : binding, true);
    const commands = Number(node.changed) + Number(body.changed) + Number(audio.pending >= 0) + Number(director.action !== 0);
    if (commands > 1) throw new Error("Only one distinct command is allowed per event");
    let immediate: Immediate | null = null;
    if (node.changed) immediate = { kind: kindSetPosition, detail: 0, x: node.x, y: node.y };
    else if (body.changed) immediate = { kind: kindSetLinearVelocity, detail: 0, x: body.vx, y: body.vy };
    else if (audio.pending >= 0) immediate = { kind: kindPlayOneShot, detail: audio.pending, x: 0, y: 0 };
    else if (director.action === 1) immediate = { kind: kindPause, detail: 0, x: 0, y: 0 };
    else if (director.action === 2) immediate = { kind: kindResume, detail: 0, x: 0, y: 0 };
    else if (director.action === 3) immediate = { kind: kindChangeScene, detail: director.scene, x: 0, y: 0 };
    if (kind !== kindFlushCommands) pendingImmediate = null;
    if (scene.cursor < scene.edits.length) {
      // Queued edits drain first. An immediate command from the same callback
      // is delivered once the queue is empty so a scene change can also move
      // nodes in that callback.
      if (immediate !== null) pendingImmediate = immediate;
      const edit = scene.edits[scene.cursor++];
      let queued = kindMoveNode;
      if (edit.kind === 1) queued = kindShowNode;
      else if (edit.kind === 2) queued = kindSelectFrame;
      else if (edit.kind === 3) queued = kindQueueSound;
      else if (edit.kind === 4) queued = kindRecordScore;
      else if (edit.kind === 5) queued = kindQueueCreateNode;
      else if (edit.kind === 6) queued = kindQueueInstantiatePrefab;
      else if (edit.kind === 7) queued = kindQueueReparentNode;
      else if (edit.kind === 8) queued = kindQueueDestroyNode;
      else if (edit.kind === 9) queued = kindQueueAddComponent;
      else if (edit.kind === 10) queued = kindQueueRemoveComponent;
      else if (edit.kind === 11) queued = kindScaleNode;
      else if (edit.kind === 12) queued = kindRotateNode;
      view.setUint32(12, queued, true);
      view.setUint32(40, edit.target, true);
      view.setUint32(44, 0, true);
      view.setFloat64(24, edit.x, true);
      view.setFloat64(32, edit.y, true);
      if (reply(bytes) !== 1) throw new Error("Aurora script reply failed");
      continue;
    }
    const response = immediate ?? pendingImmediate;
    pendingImmediate = null;
    view.setUint32(12, response === null ? kindAck : response.kind, true);
    view.setUint32(40, response === null ? 0 : response.detail, true);
    view.setUint32(44, 0, true);
    view.setFloat64(24, response === null ? node.x : response.x, true);
    view.setFloat64(32, response === null ? node.y : response.y, true);
    if (reply(bytes) !== 1) throw new Error("Aurora script reply failed");
  }
}
