---
name: ecs-scene
description: Work on the hecs world and Aurora's Node tree from M1.
---

# Purpose

Work on the hecs world and Aurora's Node tree from M1.

## Invariants

The player owns entity handles. Reject cycles, keep tree and ECS mutations consistent, and define parent-before-child entry and child-before-parent ready ordering.

## Build and run

`cargo test --locked -p aurora-engine-core`

Commands for later milestones apply once their implementation lands. Check AGENTS.md for current status.

## Verification

- [ ] Exercise add, remove, reparent, stale IDs, and transform composition. Check process and fixed update ordering.

## Definition of done

Lifecycle and ownership tests cover nested and invalid operations.
