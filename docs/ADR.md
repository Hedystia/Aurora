# Aurora architecture decisions

## M0 workspace and review notes

- The player delegates its event loop and GPU surface to engine_core. The editor owns its eframe renderer and dock tree. Separate player and script child processes will establish the runtime boundaries when those features land.
- The editor uses an original Godot-inspired slate-blue palette and preserves distinct Scene, Game, hierarchy, Inspector, asset, Console and Timeline roles. The dock tree remains project-persisted and user-editable. The default dock arrangement follows Godot's workspace: hierarchy on the left, viewport and bottom dock in the centre, Inspector above FileSystem on the right. Dock enum names are unchanged so existing saved layouts still deserialize; the `Godot Layout` action reapplies the new arrangement.
- Stable rustfmt handles formatting. Crate-level import grouping and rearrangement are maintained in source because the requested rustfmt options require nightly.
- Dependency versions are centralized. Milestone dependencies enter the lockfile when a workspace member starts using them.
- A later Godot checkout at `8f2e46b` confirmed `editor/docks/filesystem_dock.cpp` provides tree-only, vertical and horizontal split modes plus a recursive search box and per-item right-click menus. Aurora uses the searchable single-file-pane variant: no folder-tree sidebar, folder navigation through the `res://` breadcrumb and folder tiles, and the existing right-click actions. Godot's C++/custom-GUI source is not portable to egui, so the dock is reimplemented rather than copied. Godot research used the shallow checkout at revision `67421f3`. `EditorNode` composes the dock manager and run bar. `SceneTreeDock` owns tree operations and the class creation dialog. `FileSystemDock` owns asset searching and drag/drop. `InspectorDock` tracks the inspected object and delegates field editing to the inspector. The requested animation dock and node library filenames are absent at this revision. The corresponding entry points are the animation player editor plugin, animation track editor and create dialog. Aurora's lifecycle specification follows parent-first tree entry, child-first readiness and parent-relative 2D transforms. [Godot source](https://github.com/godotengine/godot/tree/67421f3)
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
- Global timer calls are lowered to component-owned SDK functions and advanced by the existing Schedule phase. Development and native builds share that lowering, so timers remain deterministic and pause with simulation instead of depending on a blocked host event loop.

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

## M7 3D groundwork

- Rapier remains the default 3D solver. The optional Jolt implementation is selected at runtime in a build that enables `physics-jolt`. Both expose the same cuboid pose and velocity shape and advance once per existing fixed tick.
- The 3D preview and windowed player share their existing GPU device, surface management and frame readback. The new opaque pass owns a perspective camera, an instanced unit cuboid and a depth attachment sized to the active target.
- The spatial snapshot adds an optional 3D section. The 2D state schema stays compatible with the earlier samples, but protocol 16 widens the script envelope from 52 to 60 bytes with a real Z slot instead of packing a coordinate into `detail`. Scripts from older protocols must be rebuilt. Authored 3D bodies are queued by the host and written when the lazily built Rapier world has created the body, so first-tick script callbacks do not race body creation. Spatial contact transitions are derived from Rapier contact pairs and mapped back to scene nodes through collider ownership. Protocol 18 keeps the 60-byte envelope and extends chunked on-demand import to PNG, audio and prefab files with one shared handle space and one release command; the host confines paths to the scene's project directory, bounds reads, allocates handles above the scene bank index range, and frees entries when the last script reference is released. Dynamic audio plays through a lazily created mixer so scenes without authored sounds still work; dynamic prefabs are registered in the scene's prefab catalog and instantiated through the existing runtime path. Spatial constraints are scene-level documents with additive serde defaults; the authored Rapier world rebuilds all impulse joints once per step so endpoint removal cannot leave orphan constraints. Raycast results use the envelope's node slot to carry the remaining normal components because the result has no scene node. Spatial physics bodies are authored with additive `SpatialNode` fields, so version 1 documents keep loading: the default collider is the original cuboid, the default body set is None/Fixed/Dynamic, and the legacy friction and restitution values remain the defaults. Kinematic bodies are driven from the scene pose, integrate queued velocity into that pose, and are never overwritten by the solver; triggers are sensors whose overlap transitions are unioned with solid contacts and reported to scripts through the Z slot of the existing contact kinds. The editor's Unity-style 3D Scene view stays in the egui painter rather than exposing the private renderer: it reuses the perspective projection for grid, gizmos and sprite quads, while Play and Game view remain the shaded WYSIWYG surface.

## Export bundle integrity

- Every new editor export includes `export.json` with the engine version, exporting host target, sorted file SHA-256 values, and a digest of those records. The player verifies the complete file set and hashes before loading a bundle that has this manifest. This detects content changes relative to the manifest; it does not authenticate a publisher or a modified manifest.
- The editor copies its sibling player into each local export and creates a host launcher. This makes the exported folder directly runnable on its build host without requiring a separately installed Aurora player; it does not provide cross-compilation or a downloadable target template.
- Older exports without the integrity manifest remain supported. The manifest does not imply the bundle contains a portable target runtime or that its scripts can run on a different platform.

## Scene component bindings

- The editor generates one ordered component list from the scene's inline attachments. The player sends a binding index alongside the node handle in protocol 5, so several components can own distinct state while accessing the same node. One child still handles the full scene, and queued edits finish before the next component receives that event.

## Project preferences and TypeScript SDK

- Project manager preferences are per user, while the project catalog retains its existing Linux data location. New projects use the configured parent folder and claim a fresh child directory before writing.
- Per-project startup state lives in `.aurora/project-session.json`, separate from the bare `DockState` in `layout.json` so older layouts keep loading. Opening a folder prefers the cached last scene, then the configured default scene, then `main.scene`; paths are relative, validated, and rejected when they escape the project. Scenes opened from inside the current project keep its root so FileSystem does not jump to the scene's folder. Unsaved navigation or deletion is confirmed with Cancel/Save/Discard rather than rejected as an error.
- The editor and CLI share the same embedded declarations and protocol generator. The editor refreshes its generated TypeScript config and stamps the declarations with its version. Existing root configs belong to the project. A cross-version stamp requires confirmation, while matching versions refresh changed generated contents on open.

## TypeScript authoring decorators

- The editor and native staging share a TypeScript syntax parser. Authored decorators declare Inspector fields and component names, and native staging erases these annotations before scriptc. Legacy script metadata remains readable.
- Protocol 6 adds ordered load and late-update phases. The scene player completes one phase across all bindings before starting the next. Each attachment retains its own component and command queue.

## Script lifecycle, authored bindings, and native export

- Protocol 7 reports component enable state in reply binding flags and adds lifecycle phases and validated scene mutation commands. Dynamic handles occupy a separate range. The player alone resolves assets from the scene's ordered prefab catalog.
- The editor builds a single ordered binding list for preview and native export. It expands authored prefab bindings in player order and applies stable execution priority. A native scene export stages source and resources, uses static-only scriptc, and publishes a new directory after compilation succeeds.

## Scene-local native programs and runtime prefab bindings

- Native bundles retain each scene's resource and component binding order and start a scene-local native process when switching. A manifest names scenes while member paths derive from validated indices.
- Protocol 8 synchronizes prefab descendants and factory bindings after a script command batch. The player owns descendant handles and runs lifecycle teardown before destroying a subtree.
- Resource callbacks resolve the authored scene's catalog and run in a separate simulation phase. Scripts receive typed handles, while decoding and rendering remain in the player.

## Scene resources and runtime components

- Protocol 9 adds scene-registered resource callbacks, component factories by registered name, runtime removal of dynamic components, and pointer press hit testing. Resource decoding and ownership remain in the player; scripts receive typed handles.

## Node transforms and tweens

- Protocol 12 sends NodeTransform and NodeScaleRotation for each scene node once per scene, after SceneStart. The script caches parent-relative position, scale and angle, so `NodeRef` getters and tween start values do not need a synchronous query. Newly created or prefab-cloned nodes enter the stream on a later tick.
- Node-targeted pointer transactions carry the hit point in the node's local pixels. The script recovers the normalized frame position from the global pointer notification that precedes the node transaction for the same identity, so `event.x`/`event.y` stay frame-normalized while `event.nodeX`/`event.nodeY` are node-local. The player computes the local point while it still owns the captured node's world matrix.
- Tweens advance on the player's fixed tick through the Tween kind rather than script-side timers. A tween emits MoveNode edits through the existing command queue, so authored and script-created nodes share the same movement path.
- Node transforms are local, not world transforms. `prepare`/`perform` style matrix control and tweening scale or rotation still need new commands.

## Scale, rotation and component references

- Protocol 13 sends local scale and rotation changes as queued ScaleNode and RotateNode edits instead of an immediate command. A fixed reply carries one command, so position, scale, and rotation changes from the same callback would otherwise conflict. Draining queued edits through FlushCommands keeps the existing ordering and callback settlement semantics.
- Protocol 14 sends runtime text changes as a bounded Start/Chunk/End sequence through the existing queued-edit path. The player assembles and validates UTF-8 before changing an authored Text visual, so partially received content cannot replace the current display.
- Protocol 15 reuses the same chunk encoding for project config writes and host-delivered config/secret values. Config is host-persisted and script-writable; secrets live outside the project with owner-only permissions and are read-only to scripts. The loading overlay is a synthesized sprite in the frame's node list, so the player and editor Play share it without a shader or frame-variant change.
- `angle` is exposed in degrees because Cocos uses degrees for node angles. The protocol and player keep radians, and the SDK converts at the boundary.
- Asset property arrays use the same catalog keys as scalars and preserve authored order. Prefab arrays join the existing prefab dependency catalog, so dynamic factories and exports see every referenced prefab. Unregistered audio or frame entries are dropped from the generated array instead of aborting scene load.
- Audio clip and sprite frame properties resolve through the scene's registered resource banks rather than raw file paths. The player exposes those banks to scripts, so listing them in the Inspector prevents authoring handles the scene cannot resolve. The generated entry calls `requireAudioClip`/`requireSpriteFrame`; every instance and prefab clone resolves the same catalog key. An unregistered path is skipped rather than crashing scene load.
- Component references are stored as a local node index plus the `@ccclass` name. The generated entry constructs every instance before assigning properties, then points the field at the live instance. Prefab builders create all of a clone's components first and the script caches the array per clone, so an internal reference resolves to the same clone rather than to another instance.

## Mouse events and wheel input

- Protocol 11 sends wheel movement as deltas in the position slots. The player hit-tests before dispatching, so a handler that moves the scene does not change the wheel target. Every running binding receives MouseWheel, then a running binding on the hit node receives PointerNodeWheel for capture, target, and bubbling delivery.
- Mouse pointer contacts also emit mouse-specific node and input event names. Touch phases remain available for code that treats the mouse as one finger. Wheel events carry deltas instead of pointer coordinates, so node wheel events do not expose a position.
- Generated editor and export entries register each compiled component class with its factory name. This lets scripts pass a class constructor to `addComponent`; the instance still arrives after the callback because the player allocates the binding.

## Pointer phases and scene reset

- Protocol 10 tracks pointer contacts by identity. A press hit-tests and captures the topmost visible sprite; movement, release, and cancellation keep the captured reference so a drag remains coherent outside the sprite. Mouse contacts use identity zero and touch contacts use one-based identities. The player sends a global phase to every running binding and a node-targeted transaction for capture, target, and bubbling delivery.
- Pointer coordinates occupy the envelope's position slots, so pointer transactions do not overwrite the cached node transform in the script runtime. Pointer identity shares the detail word with the node reference.
- `SceneStart` starts every scene. It resets node listeners, dynamic prefab state, and removed-binding markers before score restoration and load callbacks, which lets an explicit CLI script reuse authored instances across scene changes.
- Authored attachments share the dynamic teardown path for removal. The script marks an authored binding removed immediately so lookup stops returning it, while the player completes disable and destroy callbacks before dropping the binding. Bindings removed this way return on the next scene start.
- Resource handles are component-local. Releasing a clip or frame on one component no longer discards another component's handle, and teardown releases the owning component's handles only.
- The editor forwards real multi-touch contacts from egui to the preview pipe with editor-local ids, and the preview reader maps them to one-based touch identities. Focus loss emits one cancel-all event, which the runtime expands into a cancellation for every tracked pointer. The standalone player uses the same cancel-all path on `WindowEvent::Focused(false)`, so mouse and finger contacts cannot remain captured.

## Authored 2D body ownership

Scenes and prefab templates store an optional body and collider on each node. The director owns one Rapier world per active scene. After animation sampling it synchronizes external pose edits, advances the solver at 60 Hz, and writes dynamic poses back in parent-first order. Handles stay in the player. Enter and exit events use the existing collision callback path and the union of solid, trigger and legacy overlap pairs. An absent physics component has no serialized representation, which keeps old scenes and golden snapshots compatible.

## Authored 2D visuals

Text runs, atlas cells, tile grids and nine-slice panels resolve to content-addressed RGBA textures at their authored pixel dimensions. They use the existing sprite pass for transforms, alpha compositing, draw order and picking. Scene and prefab documents retain their editable source and asset reference. Fixed-tick particle emitters instead generate local quads at draw time. Their particles share one texture and leave entity handles and simulation order intact.

## Mixed 2D and 3D authoring

A scene or prefab can place an optional `spatial` component on any node. Metre-space transforms compose through spatial ancestors. Pixel-space transforms and sprite ordering keep their existing interpretation. The first visible camera and directional light in traversal order drive the depth-tested mesh pass, then the existing sprite pass overlays the scene. The Director owns the authored Rapier world and replaces it together with a successfully loaded scene. A 3D project starter uses the same scene format as a 2D project.

The editor converts static glTF 2.0 triangle primitives to an ordinary Aurora prefab, including hierarchy, local TRS, geometry and material base colors. The import publishes a new prefab without replacing an existing file. Its meshes are self-contained for Play and export.

## Engine releases and export packages

Engine/editor downloads and project export packages are distinct versioned deliverables. The engine's wgpu/winit backend remains the cross-platform renderer/window layer; Sokol bindings would not remove Android/Apple SDK or linker requirements and would not solve script portability. Target packages should be built on hosted platform runners, checksum-verified, and installed independently by the editor. Script IPC now uses Unix descriptor passing on Linux and macOS; Windows still needs a separate transport and packaging path.

## Local container build toolchain

Engine releases, builder images, and game packages are separate deliverables. `crates/cli` adds the `aurora` binary, which resolves one versioned OCI builder image, pulls it, runs the existing editor export inside it, verifies the package, and copies it to the host. The CLI reuses `engine_core`'s `ExportManifest` so host verification and player verification share one integrity algorithm rather than maintaining a second one.

The host performs no compilation. The image carries the engine source at one version, Rust, the pinned scriptc and Bun releases, and the system libraries needed by the player. Runtime network access is disabled, so builds cannot fetch dependencies; anything missing must be baked into the image. Only the project (read-only) and a private staging directory (read-write) are mounted. The container runs without Linux capabilities or new privileges, and a non-root image user writes the package. The host copies the verified package into the user-owned output directory so uid mappings do not affect ownership. Bind mounts are not SELinux-relabeled; Podman instead disables label confinement for this unprivileged build container rather than mutating the user's files.

The default lifecycle is ephemeral: `--rm` for the container and image removal after both success and failure. `--keep-image` is an explicit opt-in. A failed pull falls back only to the exact same reference already present locally, and the resolved image ID is recorded in `build.json`; this supports offline builds without silently accepting a different image. Build metadata is written before the export manifest is regenerated, so it is covered by the player's integrity check.

Target support is data, not a runtime surprise: `target` owns the triple, manifest target, features, and a supported, host-only, or blocked status, and the CLI refuses a blocked target before pulling. Windows has a MinGW cross-toolchain image as groundwork, but script IPC is Unix-only and export stamps the compiling host target. Android has an experimental SDK/NDK image, but no Activity/SurfaceView runtime or Gradle packaging, so no APK can exist yet. These images are explicitly unpublishable until the blockers are resolved and the vendor terms reviewed.

macOS is host-only, not container-backed. `aurora build --target macos` runs the editor export on an Apple-branded Mac, verifies the export, and writes `Game.app/Contents/{MacOS,Resources,Info.plist,PkgInfo}` with `build.json` marked `builder_kind: "host"`. Codesigning and notarization are optional explicit subprocesses so CI can inject a certificate and keychain profile without committing secrets. No macOS builder image exists: Apple's SLA permits up to two development VMs only on Apple-branded hardware already running macOS, so Linux images such as `dockurr/macos` breach the terms even when they boot. The supported alternatives are the user's own Mac, hosted macOS runners, Tart on Apple silicon, or EC2 Mac under Apple's CI lease. Scripted macOS packages use the same fd-3 shared-memory transport as Linux. Linux allocates a sealed `memfd`; macOS creates an unlinked POSIX shared-memory object with `shm_open`, maps it, and passes the descriptor over a `UnixStream`. The child's C endpoint is unchanged. `tools/setup.sh` now resolves Bun and scriptc Darwin builds so `compile-native.sh` can link scripts with the host `clang`. Windows still lacks Unix descriptor passing. The player finds `Contents/Resources/game` relative to its own executable when launched without an explicit mode, so the bundle needs no wrapper script.

Cleanup when the CLI process itself is killed remains best effort: `--rm` and the `aurora-build-<pid>-<n>` container name allow recovery, but no supervisor guarantees removal. Image publishing is represented by the disabled package CI: it pushes only the Linux image, and only on a release tag. No image has been published; the GHCR references remain versioned naming conventions, and the disabled workflows stay disabled until publishing is authorized.
