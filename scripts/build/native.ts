import { Component } from "../aurora-cc/index.ts";
import { run } from "./wire.ts";

declare function nativeReceive(bytes: Uint8Array): number;
declare function nativeReply(bytes: Uint8Array): number;

export function launch(component: Component): void {
  run(component, (bytes) => nativeReceive(bytes), (bytes) => nativeReply(bytes));
}
