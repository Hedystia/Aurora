---
name: wgpu-render
description: Work on Aurora's runtime wgpu renderer, including surface lifetime, sprites, and text.
---

# Purpose

Work on Aurora's runtime wgpu renderer, including surface lifetime, sprites, and text.

## Invariants

Keep device and surface ownership in engine_core. Treat zero-size surfaces as temporarily undrawable. Use the workspace wgpu line. In M1 batch by render state and preserve declared sprite order.

## Build and run

`cargo run --locked --bin aurora-player -- --screenshot /tmp/aurora-player.png`

Commands for later milestones apply once their implementation lands. Check AGENTS.md for current status.

## Verification

- [ ] Verify texture formats and resize recovery. Capture a presented frame on X11 and Wayland. For text work, verify glyph cache and blending across scale changes.

## Definition of done

Player presents and captures the expected pixels on supported Linux surfaces.
