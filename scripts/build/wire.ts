import { frameBytes, protocol, kindStart, kindUpdate, kindFixedUpdate, kindAck, kindSetPosition, kindCollisionEnter, kindCollisionExit, kindKeyDown, kindTouchStart, kindSetLinearVelocity, kindPlayOneShot } from "../../.aurora/scripts/metadata/protocol.mts";
import { Component, Collider2D, NodeHandle } from "../aurora-cc/index.ts";

// The record layout is bincode v2 fixed-width little-endian (docs/PROTOCOL.md).
const fixedDt = 1 / 60;

export function run(
  component: Component,
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
    const node = component.node;
    node.x = view.getFloat64(24, true);
    node.y = view.getFloat64(32, true);
    node.changed = false;
    const body = component.body;
    body.changed = false;
    const audio = component.audio;
    audio.pending = -1;
    const kind = view.getUint32(12, true);
    if (kind === kindStart) component.onStart();
    else if (kind === kindUpdate) component.onUpdate(fixedDt);
    else if (kind === kindFixedUpdate) component.onFixedUpdate(fixedDt);
    else if (kind === kindKeyDown) component.onKeyDown(view.getUint32(40, true));
    else if (kind === kindTouchStart) component.onTouchStart();
    else if (kind === kindCollisionEnter || kind === kindCollisionExit) {
      const other = new Collider2D(new NodeHandle(view.getUint32(40, true), view.getUint32(44, true)));
      if (kind === kindCollisionEnter) component.onBeginContact(other);
      else component.onEndContact(other);
    } else throw new Error("Invalid Aurora script event");
    const commands = Number(node.changed) + Number(body.changed) + Number(audio.pending >= 0);
    if (commands > 1) throw new Error("Only one distinct command is allowed per event");
    let response = kindAck;
    if (node.changed) response = kindSetPosition;
    else if (body.changed) response = kindSetLinearVelocity;
    else if (audio.pending >= 0) response = kindPlayOneShot;
    view.setUint32(12, response, true);
    view.setUint32(40, audio.pending >= 0 ? audio.pending : 0, true);
    view.setUint32(44, 0, true);
    view.setFloat64(24, body.changed ? body.vx : node.x, true);
    view.setFloat64(32, body.changed ? body.vy : node.y, true);
    if (reply(bytes) !== 1) throw new Error("Aurora script reply failed");
  }
}
