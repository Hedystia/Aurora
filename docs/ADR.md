# Aurora architecture decisions

## M0 workspace and review notes

- The player delegates its event loop and GPU surface to engine_core. The editor owns its eframe renderer and dock tree. Separate player and script child processes will establish the runtime boundaries when those features land.
- The editor uses an original navy palette and preserves the requested panel roles and locations. The M0 lower center has one placeholder dock for Console and Timeline. Later milestones add the separate views and editing workflows.
- Stable rustfmt handles formatting. Crate-level import grouping and rearrangement are maintained in source because the requested rustfmt options require nightly.
- Dependency versions are centralized. Milestone dependencies enter the lockfile when a workspace member starts using them.
- Godot research used the shallow checkout at revision `67421f3`. `EditorNode` composes the dock manager and run bar. `SceneTreeDock` owns tree operations and the class creation dialog. `FileSystemDock` owns asset searching and drag/drop. `InspectorDock` tracks the inspected object and delegates field editing to the inspector. The requested animation dock and node library filenames are absent at this revision. The corresponding entry points are the animation player editor plugin, animation track editor and create dialog. Aurora's lifecycle specification follows parent-first tree entry, child-first readiness and parent-relative 2D transforms. [Godot source](https://github.com/godotengine/godot/tree/67421f3)
- Cocos research follows creating a named TS component in Assets, adding it by drag or Inspector, and populating typed reference slots. Prefab isolation and pooling, contact callbacks, audio clips, input and animation remain milestones after M0. [Official tutorial](https://www.youtube.com/watch?v=zQBOxCUclx8) · [Cocos 2D workflow](https://docs.cocos.com/creator/3.8/manual/en/getting-started/first-game-2d/)
