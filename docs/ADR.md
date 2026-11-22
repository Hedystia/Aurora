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
