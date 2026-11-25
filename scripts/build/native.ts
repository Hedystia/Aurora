import { Component } from "../aurora/index.ts";
import { run } from "./wire.ts";

declare function nativeReceive(bytes: Uint8Array): number;
declare function nativeReply(bytes: Uint8Array): number;

export function launch(component: Component): void {
  void launchMany([component]);
}

export async function launchMany(components: Component[]): Promise<void> {
  await run(components, (bytes) => nativeReceive(bytes), (bytes) => nativeReply(bytes));
}
