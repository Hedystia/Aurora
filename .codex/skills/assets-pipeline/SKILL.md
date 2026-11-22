---
name: assets-pipeline
description: Modify Aurora asset importing, caching, and file watches from M1.
---

# Purpose

Modify Aurora asset importing, caching, and file watches from M1.

## Invariants

Hash the source bytes and importer settings. Publish cache results only after import succeeds. Coalesce watcher events and preserve stable reference identity across reimports.

## Build and run

`cargo test --locked -p engine_core`

Commands for later milestones apply once their implementation lands. Check AGENTS.md for current status.

## Verification

- [ ] Check repeat imports, content changes, invalid data, and rename or delete events. Verify the player can load the imported resource.

## Definition of done

The same inputs resolve consistently and changed assets refresh through normal editor entry points.
