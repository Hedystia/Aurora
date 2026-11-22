import { readSync, writeSync } from "node:fs";
import { Component } from "../aurora-cc/index.ts";
import { run } from "./wire.ts";

function receive(bytes: Uint8Array): number {
  let offset = 0;
  while (offset < bytes.length) {
    const count = readSync(0, bytes, offset, bytes.length - offset, null);
    if (count === 0) return offset === 0 ? 0 : -1;
    offset += count;
  }
  return 1;
}

function reply(bytes: Uint8Array): number {
  let offset = 0;
  while (offset < bytes.length) {
    const count = writeSync(0, bytes, offset, bytes.length - offset);
    if (count <= 0) return -1;
    offset += count;
  }
  return 1;
}

export function launch(component: Component): void {
  run(component, receive, reply);
}
