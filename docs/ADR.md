# Aurora architecture decisions

## M0 workspace and review notes

- The player delegates its event loop and GPU surface to engine_core. The editor owns its eframe renderer and dock tree. Separate player and script child processes will establish the runtime boundaries when those features land.
- The editor uses an original navy palette and preserves the requested panel roles and locations. The M0 lower center has one placeholder dock for Console and Timeline. Later milestones add the separate views and editing workflows.
- Stable rustfmt handles formatting. Crate-level import grouping and rearrangement are maintained in source because the requested rustfmt options require nightly.
- Dependency versions are centralized. Milestone dependencies enter the lockfile when a workspace member starts using them.
- Godot research used the shallow checkout at revision `67421f3`. `EditorNode` composes the dock manager and run bar. `SceneTreeDock` owns tree operations and the class creation dialog. `FileSystemDock` owns asset searching and drag/drop. `InspectorDock` tracks the inspected object and delegates field editing to the inspector. The requested animation dock and node library filenames are absent at this revision. The corresponding entry points are the animation player editor plugin, animation track editor and create dialog. Aurora's lifecycle specification follows parent-first tree entry, child-first readiness and parent-relative 2D transforms. [Godot source](https://github.com/godotengine/godot/tree/67421f3)
- Cocos research follows creating a named TS component in Assets, adding it by drag or Inspector, and populating typed reference slots. Prefab isolation and pooling, contact callbacks, audio clips, input and animation remain milestones after M0. [Official tutorial](https://www.youtube.com/watch?v=zQBOxCUclx8) · [Cocos 2D workflow](https://docs.cocos.com/creator/3.8/manual/en/getting-started/first-game-2d/)

## M1 runtime

- Scene owns hecs entities and exposes monotonic scene-local node handles. Traversal uses insertion order. Active reparenting emits subtree exit and entry in tree order, retaining readiness and local pose. Lifecycle events are returned to the runtime for dispatch.
- Rendering uses a centered, aspect-preserving design viewport. Adjacent sprites with the same texture share a draw call, preserving order for alpha blending. GPU resources stay on the window path.
- A rational 60 Hz accumulator drives the window loop with at most 250 ms of catch-up per wake. Headless runs advance the same runtime one tick at a time and write ordered JSON. The M1 smoke check compares repeated output. The game golden baseline lands in M6.
- The PNG importer hashes source bytes with a versioned importer identifier. It publishes checksummed RGBA8 cache files by atomic rename and rebuilds damaged cache entries from source. File watching and reference remapping will land with editor hot reload.

## M2 scripting bridge

- The pinned scriptc release builds the native sample in static-only mode. Its C ABI adapter accepts byte spans and communicates through inherited file descriptors. The game logic and byte codec are shared by the native and development entry points.
- Development uses the same envelopes fully on the socket so both Bun and Node can use their built-in descriptor I/O. Native builds carry hot payloads in memfd rings.
- The current SDK binds one component to the demo sprite. It passes position snapshots and applies checked commands in frame order. The sample registers inspector metadata as a strict JSON literal in TypeScript, and codegen extracts that declaration at build time.
- The player launches the child in a dedicated process group, bounds reply wait time, and reaps the group on session teardown. Automated headless runs surface protocol failures as command failures.
- Both backend choices share the same child lifecycle and envelope validation. A backend enum selects the launch and transport configuration, so the runtime uses one session implementation.

## M3 physics, input, and sound

- Rapier owns body and collider state. Nodes keep stable body associations. Contact pairs are normalized by node ID, then diffed once per tick to report exits before entries. This covers removal and keeps callback order independent of Rapier iteration order.
- The bird scene uses meters in physics and 32 pixels per meter in rendering. Window presses are queued for the next fixed tick. Headless and screenshot runs feed the same input queue with a tick-indexed schedule.
- Kira decodes the two embedded WAV clips before opening the render loop. The default device services separate effects and music tracks. Headless verification runs the same mixer and effects commands against Kira's silent backend, which also provides a fallback when the default device is unavailable.

## M4 authored scene runtime

- JSON scene and prefab documents store stable document indices and source PNG paths. Resolved entity handles and asset hashes belong to the runtime. File writes stage in the destination directory before atomic replacement. A scene transition resolves its resources before replacing the active tree.
- Pool checkout reuses the existing hecs allocations and issues fresh node handles. Pool return detaches the subtree, invalidates its handles, and removes it from processing and rendering. Scene-owned handles advance across transitions.
- Authored clips use scalar transform tracks and sample-indexed markers. Clip playback advances at 60 Hz, supports held, linear and cubic segments, and applies once, loop or ping-pong wrapping. Director pause freezes scene ticks and leaves input available for resuming.

## M5 editor integration

- The editor edits the versioned scene document. Tree operations remap indexed bindings, and both the editor viewport and player resolve the same scene resources.
- The preview owns a private frame directory and runs the normal player simulation in a separate process. A shared GPU pass renders windowed and offscreen frames. Atomic RGBA publication prevents partial frames from reaching egui. Preview commands travel downstream through the child input pipe.
- Script builds run off the UI thread and reuse static SDK metadata. A successful rebuild replaces the running preview. The dock tree is serialized per document directory.

## M6 authored game

- All sample game rules live in TypeScript. The generic authored runtime resolves sensor, sprite-frame and audio banks and applies bounded script command batches. The scene's contact areas use transformed world-space AABBs. The M3 Rapier sample remains a separate path.
- The TypeScript NodePool recycles a fixed set of authored prefab instances. Returning a slot hides its subtree and removes its colliders from authored sensor queries. This keeps authored indices and entity state deterministic through wraparound and restart.
- Profile storage is opt-in for standalone runs. Best-score writes are locked and atomically replaced. The golden run uses no profile and hashes only the player's serialized state, after checking that the trace both scores and ends the run.
