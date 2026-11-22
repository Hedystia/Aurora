# Script protocol 3

Linux player and script use one full-duplex Unix stream socket. The script receives its end as file descriptor 0. Standard output and standard error carry diagnostics to the player's standard error. Native scripts also inherit a 528-byte memfd on descriptor 3. Its size is sealed before the process starts.

Each transaction is one request and one response with the same protocol, sequence, and node. The sequence starts at zero and increments after every valid reply. Protocol 3 accepts commands for the bound node only. The player rejects unknown variants, the wrong protocol or sequence, mismatched handles and positions that cannot be stored as finite f32 values.

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
| 40 | u64 | Kind-specific detail |

| Code | Kind | Detail |
| --- | --- | --- |
| 0 | Start | 0 |
| 1 | Update | 0 |
| 2 | FixedUpdate | 0 |
| 3 | Ack | 0 |
| 4 | SetPosition | 0 |
| 5 | CollisionEnter | Other node handle |
| 6 | CollisionExit | Other node handle |
| 7 | KeyDown | 1 Space, 2 Up |
| 8 | TouchStart | 0 |
| 9 | SetLinearVelocity | 0 |
| 10 | PlayOneShot | Zero-based clip index |
| 11 | AnimationEvent | Authored marker code |
| 12 | Pause | 0 |
| 13 | Resume | 0 |
| 14 | ChangeScene | Registered scene index |

Startup sends Start once. Each bird tick sends queued input, FixedUpdate, ordered Rapier contact changes, then Update. The SDK supplies 1/60 second to both tick callbacks. Each transaction accepts one command and applies it before the next event. Scripts receive contact handles as opaque low/high words. SetLinearVelocity (meters per second) and PlayOneShot are valid for the bird scene. For authored scenes, input is delivered even while paused. On an unpaused tick FixedUpdate precedes ascending animation playback time, then Update. AnimationEvent conveys each crossed marker in that order. Pause and Resume take effect immediately. ChangeScene loads the destination at the next tick boundary and sends Start again with the new scene's bound node. Existing script processes retain their component instance across a scene change. The envelope and ring sizes remain the same as in protocol 2.

## Shared memory

The native socket record is the 16-byte envelope header. The corresponding 32-byte payload is published into shared memory before the header is sent. The receiver removes one payload after reading each complete header. Ring 0 (offset 0) carries player requests. Ring 1 (offset 264) carries script replies.

Each ring has a native-endian atomic u32 write cursor at offset 0 and read cursor at offset 4. Eight 32-byte slots follow at offset 8. Counters wrap modulo 2³² and select slots modulo 8. The producer acquires the read cursor before reusing a slot and releases the write cursor after copying. The consumer acquires the write cursor before copying and releases the read cursor after copying. One process owns each cursor for its session. A full ring or a notification without a valid payload ends the session without overwriting data.

Development scripts preserve the same envelope and transaction ordering, sending all 48 bytes on the socket via Bun or Node. This lets the development runtime use ordinary file-descriptor I/O. A bad reply, disconnect, or 250 ms reply deadline ends the session. The windowed player logs the error and continues without the child. Headless and screenshot runs return the error for automated verification.
