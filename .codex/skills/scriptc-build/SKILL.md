---
name: scriptc-build
description: Work on Aurora's TypeScript SDK, codegen, and native script build from M2.
---

# Purpose

Work on Aurora's TypeScript SDK, codegen, and native script build from M2.

## Invariants

Use scriptc for shipping. Resolve Bun before Node for development. Lower decorators to explicit registration metadata before static compilation and use an explicit C ABI at native boundaries.

## Build and run

`tools/setup.sh --print-runtime; tools/setup.sh --print-scriptc; cargo run --locked --bin aurora-codegen`

Commands for later milestones apply once their implementation lands. Check AGENTS.md for current status.

## Verification

- [ ] Check metadata extraction against fixtures, compile the native sample in static-only mode, and verify the fallback runtime separately. Exercise WASI where the task requests it.

## Definition of done

Generated bindings and inspector metadata match the SDK, and the native sample runs without a JS engine.
