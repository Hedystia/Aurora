import { Hello } from "../game-template/hello.ts";
import { run } from "./wire.ts";

declare function nativeReceive(bytes: Uint8Array): number;
declare function nativeReply(bytes: Uint8Array): number;

run(new Hello(), (bytes) => nativeReceive(bytes), (bytes) => nativeReply(bytes));
