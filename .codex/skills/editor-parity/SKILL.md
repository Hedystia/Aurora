---
name: editor-parity
description: Verify Aurora's editor layout and scene-authoring workflows, especially at M5.
---

# Purpose

Verify Aurora's editor layout and scene-authoring workflows, especially at M5.

## Invariants

Match the requested workflow in Aurora's own visual language. Preserve the six default dock areas and keep dock actions consistent with project state.

## Build and run

`cargo run --locked --bin aurora-editor -- --screenshot /tmp/aurora-editor.png`

Commands for later milestones apply once their implementation lands. Check AGENTS.md for current status.

## Verification

- [ ] Check tree search, rename, duplicate and drag; asset import and script creation; attach both ways; reference rejection and acceptance; timeline keys; and Play, Stop, and reload.

## Definition of done

A user completes the scene-authoring checklist in the native editor and screenshot evidence covers the default layout.
