---
name: audio-kira
description: Work on Kira sound playback, buses, and clip loading from M3.
---

# Purpose

Work on Kira sound playback, buses, and clip loading from M3.

## Invariants

Route one-shots to the SFX bus and music to its own bus. Decode resources away from the render thread. Resolve clip references before playback.

## Build and run

`cargo test --locked -p aurora-engine-core`

Commands for later milestones apply once their implementation lands. Check AGENTS.md for current status.

## Verification

- [ ] Check valid and missing clips, index bounds, bus routing, and unavailable devices.

## Definition of done

AudioSource playback resolves a typed clip and produces the expected one-shot.
