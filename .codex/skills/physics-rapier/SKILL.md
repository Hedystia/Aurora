---
name: physics-rapier
description: Modify Aurora's Rapier bodies, colliders, or contact events from M3.
---

# Purpose

Modify Aurora's Rapier bodies, colliders, or contact events from M3.

## Invariants

Step at 60 Hz using the simulation clock. Maintain stable node-to-body ownership and normalize contact ordering before sending events to scripts.

## Build and run

`cargo test --locked -p aurora-engine-core`

Commands for later milestones apply once their implementation lands. Check AGENTS.md for current status.

## Verification

- [ ] Check dynamic, static, and kinematic pairs, all requested collider shapes, and contact enter/exit on removal.

## Definition of done

Physics steps and contact callbacks are reproducible for the same inputs.
