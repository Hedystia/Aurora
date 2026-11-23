# Aurora scripting and roadmap handoff

Reviewed on 2026-11-23.

## Context for the next agent

Aurora is an independent game engine and editor. Its runtime and editor are written in Rust. Games use TypeScript through the `aurora` module. The scripting migration aims to provide a workflow similar to Cocos Creator and Cocos 4 while shipping a native game executable compiled with scriptc. Bun or Node is used for development.

The working tree contains many uncommitted changes and new files. Preserve them. Run `git status --short` before editing and inspect the existing changes. `docs/ROADMAP.md` and `docs/SCRIPTING.md` describe earlier work, but some statements may lag behind the code.

The user asked that the final test suite be deferred until implementation is finished. The recent broad scripting, protocol 9, export, and GUI changes have not been verified end to end. Features described below as implemented are observations from the source, not a guarantee that they compile or work.

## Official references

- [Cocos 4 repository](https://github.com/cocos/cocos4), including [Component](https://github.com/cocos/cocos4/blob/v4.0.0/cocos/scene-graph/component.ts), [Node](https://github.com/cocos/cocos4/blob/v4.0.0/cocos/scene-graph/node.ts), the [component scheduler](https://github.com/cocos/cocos4/blob/v4.0.0/cocos/scene-graph/component-scheduler.ts), and [asset bundles](https://github.com/cocos/cocos4/blob/v4.0.0/cocos/asset/asset-manager/bundle.ts).
- [Cocos Creator 3.8 scripting index](https://docs.cocos.com/creator/3.8/manual/en/scripting/index.html), [decorators](https://docs.cocos.com/creator/3.8/manual/en/scripting/decorator.html), and [lifecycle callbacks](https://docs.cocos.com/creator/3.8/manual/en/scripting/life-cycle-callbacks.html).
- [scriptc on GitHub](https://github.com/vercel-labs/scriptc), [CLI](https://scriptc.dev/docs/cli), [FFI](https://scriptc.dev/docs/ffi), and [native objects](https://scriptc.dev/docs/native-objects).
- In this repository, read `AGENTS.md`, `docs/ADR.md`, `docs/PROTOCOL.md`, `docs/VERSIONS.md`, `docs/SCRIPTING.md`, `docs/SCRIPTC_LIMITATIONS.md`, and relevant skills under `.codex/skills/`.

Consult the official sources before claiming parity. Do not copy Cocos code or depend on its editor.

## Current scripting map

- `scripts/aurora/index.ts`, `events.ts`, `scheduler.ts`, and `resources.ts` contain the TypeScript API.
- `crates/codegen/src/metadata/` extracts decorators using Tree-sitter and prepares code for scriptc. `crates/codegen/src/sdk.d.ts` contains declarations installed into projects.
- `crates/editor/src/preview/` generates instances, properties, and factories for Play. `crates/editor/src/export/` prepares scenes and native programs. `crates/editor/src/script_assets/` builds the resource catalog.
- `crates/script_api/`, `scripts/build/wire.ts`, and `scripts/build/transport.c` define the interprocess protocol. The current source indicates version 13. Before changing messages, read and update `docs/PROTOCOL.md` and confirm that all endpoints use the same ordering and version.
- `crates/engine_core/src/app/runtime/`, `app/script_lifecycle/`, and `director/script_scene/` manage callbacks, entities, and prefabs. The player owns handles and validates commands.
- Legacy `.aurora-scene` scenes, `.aurora-prefab` prefabs, the `aurora-cc` alias, and `registerComponent` registrations have compatibility paths. Do not delete or rename user data without a safe migration.

## Remaining scripting work

1. **Integrate and verify recent changes.** Review protocol 9, pointer delivery, component creation and removal, dynamic prefabs, lifecycle ordering, and multi-scene export. These changes were made in parallel and need an integrated review. Check errors and child-process cleanup when stopping or switching scenes.
2. **Runtime components.** The source adds `addComponent` by name and removal of dynamic components. Removal of components originally authored in a scene is a known limitation. Review identity, node ownership, callback ordering, destruction, and multiple instances. Compare with Cocos 4 before expanding the interface.
3. **Input and events.** Recent code covers pointer position and sprite hit testing. Complete and verify movement, end and cancellation events, multiple touches, mouse input, key-up and key-down, coordinates, and capture and propagation in both the player window and Game view. Avoid claiming complete Cocos input semantics.
4. **Resources.** `AssetBundle` provides views over already registered catalogs and releases script-side handles. The player still needs general on-demand import and loading, real preloading, reference management, memory release, errors, and cancellation. Keep the editor, Bun/Node, and scriptc APIs consistent.
5. **Cocos API.** Complete the relevant node and component creation and destruction, lookup, timers, events, scene loading and switching, resources, and decorators against the references. Review signatures and behavior, serializable types, arrays, components, and asset references. The current API covers a subset.
6. **Native export.** Verify arbitrary projects, prefab components and resources, module references, multiple scenes, partial failures, and portability. The exporter uses `--tier static-only`. Confirm which TypeScript constructs scriptc supports and record actual limits in `docs/SCRIPTC_LIMITATIONS.md`.
7. **Compatibility and migration.** Confirm older projects still open and preserve their data and customized `tsconfig.json`. Older native scripts need rebuilding after a protocol change. Writes and retries must keep projects consistent.
8. **Declarations and editor workflow.** Keep `sdk.d.ts`, implementation classes, metadata extraction, and the Inspector synchronized. Verify that generated components can be created, attached, and configured through the UI.

## Remaining roadmap

- **2D and physics.** Integrate Rapier into authored scenes with rigid bodies, box/circle/polygon colliders, contacts, and Inspector controls. Authored scenes currently use overlap sensors, while the physics example has a separate path.
- **2D rendering and editor.** Add runtime text and fonts, Rich Text, particles, tile maps, atlases, nine-slice sprites, groups, and animation curve editing. Complete end-to-end editor interaction checks and the visual migration requested by the user.
- **Jumpy Bird.** The project and a golden replay exist. Build and verify the complete workflow in the editor against the original tutorial checklist.
- **Authored 3D.** The current example demonstrates cube physics and rendering. Add spatial nodes and transforms, cameras, lights, glTF, materials, editing tools, and script-visible 3D physics.
- **Distribution.** Verify hosted CI, packaging, clean X11 and Wayland startup, live audio, and different drivers. Verify Windows and macOS configuration paths. Add a repository license before publishing.

## Verification after implementation

When the user permits the final test run, use the commands in `AGENTS.md`. At minimum, check formatting, Clippy, workspace tests, TypeScript, scriptc native builds, Bun/Node development execution, Play/Stop, export and scene switching, prefabs, collisions, input, and migration of older projects.

`tools/golden_test.sh` checks Jumpy Bird over 600 frames. The last previously verified hash was `64e4400479526792aa932d5df8a8a49a83f4f69e716d62a5b960091c6e04b165`. Regenerate the baseline only after reviewing an intentional behavior change and following the documented `--bless` procedure.

## Progress after the 2026-11-23 scripting pass

Completed and verified end to end:

- Protocol 10 adds key release, identity-tracked pointer press/move/release/cancel, node-targeted pointer transactions with capture and bubbling, and a `SceneStart` reset. Native and development launchers produce identical snapshots for `hello` and the two-scene `scene` sample.
- Pointer capture keeps a drag on the node hit at press. The mouse uses pointer identity zero, touches use one-based identities, and window focus loss cancels the mouse contact. The Game view forwards press, drag, release, key press and key release over its input pipe.
- Authored, prefab-created, and runtime-added components share one removal path. Removing an authored attachment hides it from lookup immediately and runs disable/destroy teardown. A later scene start restores the binding.
- A callback that changes scene and also queues node edits now drains the queued edits before the immediate command. This was the cause of the multi-scene export abort.
- Multi-scene native export was verified with prefab-backed scenes, per-scene native programs, and a scripted transition (`main` to `second`). Exports reject assets outside the project and existing destinations without leaving a partial directory.
- Resource handles are component-local, `releaseAll` exists on `Resources` and `AssetBundle`, and teardown drops the owning component's handles. Unit tests cover event propagation, pointer delivery, authored removal, and handle isolation.

Progress after the follow-up protocol 13 pass:

- Protocol 13 adds queued ScaleNode and RotateNode edits. `NodeRef` and `Node` expose writable `scaleX`, `scaleY`, and `angle` (degrees), and `tween` drives position, scale, and rotation together. A headless probe reached scale `[4,5]` and a 90-degree angle with byte-identical native and development snapshots.
- The demo launcher now dispatches the tween tick and drains queued edits through FlushCommands; previously only an immediate command was observed, so queued edits were dropped outside authored scenes.
- `@property(SomeComponent)` is parsed as a component reference. The Inspector lists components from the open document, the scene format stores the node index and component name, and generated entries construct all instances first so fields receive live instances. Prefab builders create a clone's components together and the SDK caches them per clone.
- Keyboard coverage includes Tab, Shift, Control, Alt, Page Up/Down, Home, End, Insert, Delete, F1–F12, and common punctuation. Shift, Control, and Alt accept either physical side.
- `@property(AudioClip)` and `@property(SpriteFrame)` select scene-registered sound and frame-bank assets in the Inspector. Arrays of prefabs, audio clips, and sprite frames use the same pickers with add/remove controls. The generated entry resolves them through `requireAudioClip`/`requireSpriteFrame` (or `new Prefab`), and unregistered paths stay at the field default.
- The Game view forwards real egui multi-touch contacts with stable ids through the preview pipe, and window or view focus loss cancels every contact. The standalone player's focus loss also cancels all fingers, not just the mouse.

Still open:

- General on-demand asset import, true preloading with progress, reference-counted player memory release, and a Cocos-compatible `assetManager`. Queued resource callbacks can now be cancelled with `cancelLoads`, but current resources are still views over the scene-registered banks.
- Gesture recognizers. Pointer events carry `nodeX`/`nodeY` in node-local pixels alongside the normalized frame coordinates; mouse-specific events, wheel input, and the Game view wheel and multi-touch paths are implemented. The player and SDK now cover Tab, Shift, Control, Alt, navigation and editing keys, function keys F1–F12, and common punctuation.
- Full Cocos node API, tweening arbitrary properties, and additional component decorators beyond `ccclass`, `property`, and `executionOrder`. Nodes expose `children`, `childCount`, `removeFromParent`, cached local transforms, writable scale and angle, and position/scale/rotation tweens on the fixed tick.
- Prefab prefab-list changes across an explicit-script scene switch (native bundles avoid this by starting a scene-local process).
- Interactive multi-touch, window-focus, and Game view drag checks in a live desktop session. Headless pointer verification covers press, move, end, capture, and node propagation.
