---
name: egui-editor
description: Modify Aurora's eframe shell, docking, theme, or game viewport.
---

# Purpose

Modify Aurora's eframe shell, docking, theme, or game viewport.

## Invariants

Keep labels and visual constants in their owning modules. Store the dock tree in editor state. Keep game-frame input downstream of the player process boundary.

## Build and run

`cargo run --locked --bin aurora-editor -- --screenshot /tmp/aurora-editor.png`

Commands for later milestones apply once their implementation lands. Check AGENTS.md for current status.

## Verification

- [ ] Capture default and reset layouts. Exercise moving and floating panes. For preview work, check child shutdown and resizing independently.

## Definition of done

The requested editor interaction works from the native app and screenshot evidence matches the layout.
