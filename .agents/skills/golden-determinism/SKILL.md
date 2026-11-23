---
name: golden-determinism
description: Work on the fixed-step determinism test and committed demo baseline from M6.
---

# Purpose

Work on the fixed-step determinism test and committed demo baseline from M6.

## Invariants

Hash canonical entity state after exactly 600 fixed steps. Use stable ID ordering and a fixed input trace. Baseline changes require the explicit bless command.

## Build and run

`tools/golden_test.sh; tools/golden_test.sh --bless`

Commands for later milestones apply once their implementation lands. Check AGENTS.md for current status.

## Verification

- [ ] Run the normal check twice and compare state hashes. Review the state changes before blessing an intentional update.

## Definition of done

The committed baseline passes cleanly and repeatably from the headless runner.
