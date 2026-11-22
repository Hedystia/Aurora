---
name: ipc-shm
description: Implement or review Aurora's script IPC, shared memory, or lifecycle supervision from M2.
---

# Purpose

Implement or review Aurora's script IPC, shared memory, or lifecycle supervision from M2.

## Invariants

Use the socket for framed control messages and one producer and consumer per shared-memory direction. Specify acquire/release ordering, capacity, acknowledgments, saturation behavior, and protocol version before changing the wire format.

## Build and run

`cargo test --locked -p script_api -p engine_core`

Commands for later milestones apply once their implementation lands. Check AGENTS.md for current status.

## Verification

- [ ] Check wraparound, full/empty transitions, disconnect and child exit, malformed envelopes, and version rejection.

## Definition of done

Protocol docs and both process endpoints agree on layout, sequence rules, and backpressure.
