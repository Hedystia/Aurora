import { Worker } from "node:worker_threads";
import type { Component } from "../aurora/index.ts";
import { run } from "./wire.ts";

const FRAME = 60;
const name = process.env.AURORA_SCRIPT_PIPE;
if (!name) throw new Error("AURORA_SCRIPT_PIPE is not set");

// The development wire interface is synchronous, while node:net is event
// driven. A worker owns the pipe and exchanges whole frames through shared
// memory: state[0] is the input state (0 empty, 1 connecting, 2 frame, 3 EOF)
// and state[2] flags an outgoing frame.
const control = new SharedArrayBuffer(12);
const input = new SharedArrayBuffer(FRAME);
const output = new SharedArrayBuffer(FRAME);
const state = new Int32Array(control);

const workerSource = `
(async () => {
  const net = await import("node:net");
  const threads = await import("node:worker_threads");
  const state = new Int32Array(threads.workerData.control);
  const input = new Uint8Array(threads.workerData.input);
  const output = new Uint8Array(threads.workerData.output);
  const socket = net.connect(threads.workerData.name);
  const pending = [];
  let ended = false;

  function pump() {
    if (Atomics.load(state, 2) === 1) {
      socket.write(Buffer.from(output));
      Atomics.store(state, 2, 0);
      Atomics.notify(state, 2);
    }
    if (Atomics.load(state, 0) === 0) {
      if (pending.length >= 60) {
        for (let index = 0; index < 60; index++) input[index] = pending.shift();
        Atomics.store(state, 0, 2);
        Atomics.notify(state, 0);
      } else if (ended) {
        Atomics.store(state, 0, 3);
        Atomics.notify(state, 0);
      }
    }
  }

  socket.on("data", (chunk) => {
    for (let index = 0; index < chunk.length; index++) pending.push(chunk[index]);
    pump();
  });
  socket.on("end", () => { ended = true; pump(); });
  socket.on("error", () => { ended = true; pump(); });
  socket.on("connect", pump);
  threads.parentPort.on("message", pump);
})();
`;

const worker = new Worker(workerSource, {
  eval: true,
  workerData: { control, input, output, name },
});

function receive(bytes: Uint8Array): number {
  for (;;) {
    const current = Atomics.load(state, 0);
    if (current === 2) {
      bytes.set(new Uint8Array(input));
      Atomics.store(state, 0, 0);
      Atomics.notify(state, 0);
      worker.postMessage("pump");
      return 1;
    }
    if (current === 3) {
      void worker.terminate();
      return 0;
    }
    Atomics.wait(state, 0, current);
  }
}

function reply(bytes: Uint8Array): number {
  new Uint8Array(output).set(bytes);
  Atomics.store(state, 2, 1);
  worker.postMessage("pump");
  return 1;
}

export function launch(component: Component): void {
  void launchMany([component]);
}

export async function launchMany(components: Component[]): Promise<void> {
  await run(components, receive, reply);
}
