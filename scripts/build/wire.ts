import { frameBytes, protocol, kindStart, kindUpdate, kindFixedUpdate, kindAck, kindSetPosition } from "../../.aurora/scripts/metadata/protocol.mts";
import { Component } from "../aurora-cc/index.ts";

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
    const kind = view.getUint32(12, true);
    if (kind === kindStart) component.onStart();
    else if (kind === kindUpdate) component.onUpdate(fixedDt);
    else if (kind === kindFixedUpdate) component.onFixedUpdate(fixedDt);
    else throw new Error("Invalid Aurora script event");
    view.setUint32(12, node.changed ? kindSetPosition : kindAck, true);
    view.setFloat64(24, node.x, true);
    view.setFloat64(32, node.y, true);
    if (reply(bytes) !== 1) throw new Error("Aurora script reply failed");
  }
}
