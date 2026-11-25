# Script protocol 17

The player and script use one full-duplex Unix stream socket. The script receives its end as file descriptor 0. Standard output and standard error carry diagnostics to the player's standard error. Native scripts also inherit a 720-byte shared-memory region on descriptor 3. Linux allocates it with a sealed `memfd`; macOS creates an unlinked POSIX shared-memory object with `shm_open`. The native script maps the descriptor and exchanges fixed-size payload slots while the socket carries the 16-byte headers.

Each transaction is one request and one response with the same protocol, sequence, node, and component binding. The sequence starts at zero and increments after every valid reply. The player accepts immediate commands for the bound node and queued commands for validated authored indices. The player rejects unknown variants, the wrong protocol or sequence, mismatched node handles or bindings, and positions that cannot be stored as finite f32 values.

## Envelope

bincode 2 uses fixed-width little-endian encoding. `aurora-codegen` emits the protocol version and encoded envelope size from script_api.

| Offset | Type | Field |
| --- | --- | --- |
| 0 | u32 | Protocol version |
| 4 | u64 | Sequence |
| 12 | u32 | Kind |
| 16 | u64 | Opaque node handle |
| 24 | f64 | Local X or linear velocity X |
| 32 | f64 | Local Y or linear velocity Y |
| 40 | f64 | Local Z position, rotation or velocity component. Zero when unused. |
| 48 | u64 | Kind-specific detail |
| 56 | u32 | Component binding index. In replies, bit 31 reports the enabled state. |

Pointer kinds use the position slots differently: `X` and `Y` carry normalized
coordinates measured from the top left, not a node transform. `PointerTouch`
and the node-targeted move, end and cancel kinds put the hit node reference in
the low 32 bits of `Detail` and the pointer identity in the high 32 bits. The
global `PointerMove`, `PointerEnd` and `PointerCancel` kinds put only the
pointer identity in `Detail`. Pointer identity is zero for the mouse and
one-based for each touch contact.

| Code | Kind | Detail |
| --- | --- | --- |
| 0 | Start | 0 |
| 1 | Update | 0 |
| 2 | FixedUpdate | 0 |
| 3 | Ack | 0 |
| 4 | SetPosition | 0 |
| 5 | CollisionEnter | Other node handle |
| 6 | CollisionExit | Other node handle |
| 7 | KeyDown | 1 Space, 2 Up, 8 Backspace, 13 Enter, 27 Escape, 37 Left, 39 Right, 40 Down, 48–57 digits, 65–90 letters. |
| 8 | TouchStart | Pointer identity. X and Y carry normalized coordinates. |
| 9 | SetLinearVelocity | 0 |
| 10 | PlayOneShot | Zero-based clip index |
| 11 | AnimationEvent | Authored marker code |
| 12 | Pause | 0 |
| 13 | Resume | 0 |
| 14 | ChangeScene | Registered scene index |
| 15 | FlushCommands | 0 |
| 16 | MoveNode | Authored node index |
| 17 | ShowNode | Authored node index |
| 18 | SelectFrame | Authored node index |
| 19 | QueueSound | Scene sound index |
| 20 | RecordScore | Unsigned score |
| 21 | RestoreScore | Unsigned best score |
| 22 | Load | 0 |
| 23 | LateUpdate | 0 |
| 24 | QueueCreateNode | New script node handle. X is parent or -1 for a root. |
| 25 | QueueInstantiatePrefab | New script node handle. X is parent or -1, Y is the scene prefab index. |
| 26 | QueueReparentNode | Script node handle. X is parent or -1. |
| 27 | QueueDestroyNode | Script node handle. |
| 28 | Enable | 0 |
| 29 | Disable | 0 |
| 30 | Destroy | 0 |
| 31 | Schedule | 0 |
| 32 | PollEnabled | 0 |
| 33 | RegisterPrefabNode | Low u32 is node reference, high u32 is clone root reference. X is prefab index or -1, Y is parent reference or -1. |
| 34 | RegisterPrefabComponent | Low u32 is clone root reference, high u32 is prefab index. X is attachment ordinal, Y is prefab-local owner index. |
| 35 | ReleasePrefabNode | Node reference. |
| 36 | ReleasePrefabComponent | 0. The binding identifies the removed script component. |
| 37 | UpdatePrefabParent | Node reference. X is the new parent reference or -1. |
| 38 | ProcessResources | 0. Delivers one pending resource callback. |
| 39 | QueueAddComponent | Node reference. X is the registered component factory index. |
| 40 | QueueRemoveComponent | Binding index, authored or dynamic. X is its owner node reference. |
| 41 | RegisterComponent | Binding identifies the new component. X is node reference, Y is factory index. |
| 42 | PointerTouch | Low u32 is the hit node reference, high u32 the pointer identity. X and Y carry node-local pixels. |
| 43 | KeyUp | 1 Space, 2 Up, 8 Backspace, 13 Enter, 27 Escape, 37 Left, 39 Right, 40 Down, 48–57 digits, 65–90 letters. |
| 44 | PointerMove | Pointer identity. X and Y carry normalized coordinates. |
| 45 | PointerEnd | Pointer identity. X and Y carry normalized coordinates. |
| 46 | PointerCancel | Pointer identity. |
| 47 | PointerNodeMove | Low u32 is the hit node reference, high u32 the pointer identity. X and Y carry node-local pixels. |
| 48 | PointerNodeEnd | Low u32 is the hit node reference, high u32 the pointer identity. X and Y carry node-local pixels. |
| 49 | PointerNodeCancel | Low u32 is the hit node reference, high u32 the pointer identity. |
| 50 | SceneStart | 0. Resets script scene state before the scene's bindings load. |
| 51 | MouseWheel | X is the horizontal delta and Y the vertical delta. Delivered to every running binding. |
| 52 | PointerNodeWheel | Low u32 is the hit node reference. X is the horizontal delta and Y the vertical delta. |
| 53 | NodeTransform | Detail is the node reference. X and Y carry the local position. |
| 54 | NodeScaleRotation | Low detail u32 is the node reference; high detail u32 is the rotation's IEEE-754 bits. X and Y carry the local scale. |
| 55 | Tween | 0. Advances script tweens once for the current simulation tick. |
| 56 | ScaleNode | Queued local scale for the authored node in detail. X is scale X and Y is scale Y. |
| 57 | RotateNode | Queued local rotation for the authored node in detail. X is the angle in radians. |
| 58 | SetText | Queued text replacement. Detail packs an operation in the high 32 bits and the node reference in the low 32 bits. |
| 59 | SetLoading | Immediate command. Detail is 1 to show the blocking overlay and 0 to hide it. |
| 60 | ConfigSet | Queued config write. Detail packs an operation in the high 32 bits and an entry index in the low 32 bits. |
| 61 | ConfigValue | Host delivery of one config entry. Detail packs the operation and entry index. |
| 62 | SecretValue | Host delivery of one secret entry. Detail packs the operation and entry index. |
| 63 | NodeTransform3D | Detail is the node reference. X, Y and Z carry the local position. |
| 64 | NodeRotation3D | Detail is the node reference. X, Y and Z carry local Euler rotation in degrees. |
| 65 | NodeScale3D | Detail is the node reference. X, Y and Z carry the local scale. |
| 66 | SetLinearVelocity3D | X, Y and Z carry the bound spatial body's velocity in meters per second. |
| 67 | CollisionEnter3D | Other spatial body handle in detail. |
| 68 | CollisionExit3D | Other spatial body handle in detail. |
| 69 | LoadSprite | Chunked on-demand PNG path. Detail packs the operation and request index. |
| 70 | AssetLoaded | Delivers one requested handle. Detail is the request index, X the handle and Y one on failure. |
| 71 | ReleaseAsset | Releases one script-owned asset handle. |

For `SetText`, operation 0 starts a replacement, operations 1 through 12 carry
that many UTF-8 bytes, and operation 13 ends the replacement. For a chunk,
the operation value in `Detail` is the byte count. X and Y each carry six
little-endian payload bytes as exactly representable f64 integers; unused bytes
in the last chunk are zero. The queue allows at most 128 edits, limiting one
replacement to 1512 UTF-8 bytes. The player validates the assembled UTF-8
before updating an authored Text visual.

`ConfigSet`, `ConfigValue` and `SecretValue` use the same 12-byte chunk packing, but each chunk carries a `name\0value` byte stream rather than text-node content. Operations 0 and 13 start and end a stream; operations 1 through 12 carry that many bytes with the count in the high 32 bits of `Detail` and the entry index in the low 32 bits. The player sends config and secret entries to every running binding once per scene start and expects an `Ack`. Config writes are assembled, UTF-8 validated, and atomically persisted by the host; secrets are read-only to scripts. `SetLoading` toggles a fullscreen dim overlay that swallows gameplay input until a script clears it; it does not pause simulation.

`LoadSprite` reuses the 12-byte chunk packing for a project-relative PNG path. Operation 0 starts a request, operations 1 through 12 carry path bytes with the count in the high detail bits and the request index in the low bits, and operation 13 ends it. The host confines the path to the scene's project directory, imports the PNG through the existing content-addressed cache, and replies with `AssetLoaded` carrying the request index, the allocated handle in X, and a failure flag in Y. Handles begin at `0x80000000` so `SelectFrame` can distinguish them from scene frame-bank indices. `ReleaseAsset` drops one script reference; the host frees the dynamic frame when its reference count reaches zero.

The player resolves authored binding indices in the same order as the script entry. It reports Load when a node first becomes active, Enable when a loaded binding becomes active and enabled, and Start once per loaded binding. Active components receive input, FixedUpdate, animation markers and physics contacts according to the host, Update, Schedule, ProcessResources, and LateUpdate. The tick callbacks receive 1/60 second. The player synchronizes activation between callback phases and polls an independently disabled component for changes on its active node.

Scene replacement and graceful shutdown send Disable and Destroy in reverse binding order. Every scene begins with SceneStart, which clears script-side dynamic nodes, removed-binding markers and node event listeners before score restoration and Load callbacks. Native bundles start a scene-local script process on scene replacement. Explicit CLI script launchers can reuse their authored instances across scenes. On graceful shutdown the player bounds the total exchange time before terminating the child. Rebuild scripts from older protocol versions before using them with this player; protocol 16 scripts lack on-demand asset loading, protocol 15 scripts use the older 52-byte envelope and cannot exchange spatial transforms or 3D velocities, protocol 14 scripts do not understand the loading overlay or configuration delivery, protocol 13 scripts do not understand text replacement, protocol 12 scripts also lack scale and rotation edits, protocol 11 scripts also lack the transform stream and tween tick, protocol 10 scripts also lack the wheel kinds, and protocol 9 scripts also lack the pointer kinds and SceneStart.

Initial scene node indices occupy the lower half of the script command reference range. Script-created roots use monotonically increasing references in `0x80000000..0xC0000000`. The player allocates prefab descendant references from `0xC0000000` onwards. The player validates reference existence, prefab indices, and parent links before applying commands. The prefab index names an entry in the scene's ordered `script_prefabs` list. Runtime prefab control messages synchronize node references and new component bindings after a callback command batch. The node header identifies the actual player entity. Each control request requires an Ack. Node-control requests use binding zero, while component-control requests name the dynamic binding. Lifecycle teardown precedes the physical removal of a subtree.

## Shared memory

The native socket record is the 16-byte envelope header. The corresponding 44-byte payload is published into shared memory before the header is sent. The receiver removes one payload after reading each complete header. Ring 0 (offset 0) carries player requests. Ring 1 (offset 360) carries script replies.

Each ring has a native-endian atomic u32 write cursor at offset 0 and read cursor at offset 4. Eight 44-byte slots follow at offset 8. Counters wrap modulo 2³² and select slots modulo 8. The producer acquires the read cursor before reusing a slot and releases the write cursor after copying. The consumer acquires the write cursor before copying and releases the read cursor after copying. One process owns each cursor for its session. A full ring or a notification without a valid payload ends the session without overwriting data.

Development scripts preserve the same envelope and transaction ordering, sending all 52 bytes on the socket via Bun or Node. This lets the development runtime use ordinary file-descriptor I/O. A bad reply, disconnect, or 250 ms reply deadline ends the session. The windowed player logs the error and continues without the child. Headless and screenshot runs return the error for automated verification.

An authored event can return an immediate command or queue up to 128 scene edits. After each queued reply, the player applies one edit and sends FlushCommands to get the next one. Ack ends the batch. MoveNode reads X/Y in local pixels. ShowNode requires X equal to 0 or 1. SelectFrame reads an integral frame index from X. QueueSound and RecordScore read only Detail. RestoreScore precedes Load and Start for a scene with gameplay resources. Each callback and its queued edits finish before the next binding receives the event. Sensor contacts are sampled once per bound node after all LateUpdate callbacks and their commands. Every component on that node receives the same exits and entries, ordered by handle within each group.

Pointer input is tracked per identity. A press carries a marker byte followed by
little-endian normalized X and Y values from the editor, or a touch or mouse
press from the player window. The player maps the position through the active
camera fit policy, checks visible sprite quads in reverse render order, and
captures the topmost hit. Movement, release and cancellation reuse that capture
even when the pointer leaves the sprite. The mouse uses the same pointer path
as one finger. A lost window focus cancels every active contact, including all
fingers, so no node stays pressed after focus returns.

Each phase produces a global notification for every running binding and a
node-targeted transaction delivered to the captured reference. The player
prefers a running binding on the hit node so `this.node` mutations affect that
node, then falls back to the first running binding for listeners that other
components attached. Node listeners receive capture, target and bubbling
phases. Global input subscriptions receive the phase regardless of what was
hit. Mouse contacts also emit the corresponding mouse-specific event names.
Node event coordinates are supplied twice. `event.x` and `event.y` are normalized
to the full game frame with origin at the top left, because the script caches the
preceding global pointer notification. `event.nodeX` and `event.nodeY` are the
same point mapped into the hit node's local pixel space, so a sprite anchored at
its centre reads zero when the pointer sits on its pivot. A coordinate-free
legacy touch becomes a centre-frame tap so headless checks still exercise hit
testing.A mouse wheel hit-tests the cursor before any handler moves the scene. The player sends the deltas to every running binding as MouseWheel, then sends PointerNodeWheel to the precomputed hit node. Wheel transactions carry deltas in the position slots, so the script does not treat them as a transform. A node wheel event has no pointer position; its `scrollX` and `scrollY` carry the
deltas.

After SceneStart, the player streams NodeTransform and NodeScaleRotation for
every scene node the script has not seen. Script transforms are therefore
available before Load and Start for `NodeRef` position, scale and angle reads.
The stream is idempotent per reference and catches script-created or prefab
nodes on a later tick. Local transforms are parent-relative; they are not world
transforms.

The player sends Tween once per simulation tick after Update and before
Schedule, to a running binding. The script advances its tween list once and
drains the resulting MoveNode, ScaleNode, and RotateNode edits through the normal
command queue. Tweens are driven by the player's fixed tick, not wall-clock time.

Scale and rotation changes use queued edits rather than an immediate command, so
a callback can move, scale, and rotate the same node before returning. As with
other queued edits, the player sends FlushCommands after each reply until the
script answers Ack. The script reports `angle` in degrees and converts to
radians for RotateNode.
