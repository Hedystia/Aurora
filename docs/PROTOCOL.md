# Script protocol 1

Linux player and script use one full-duplex Unix stream socket. The script receives its end as file descriptor 0. Standard output and standard error carry diagnostics to the player's standard error. Native scripts also inherit a 400-byte memfd on descriptor 3. Its size is sealed before the process starts.

Each transaction is one request and one response with the same protocol, sequence, and node. The sequence starts at zero and increments after every valid reply. Protocol 1 accepts commands for the bound node only. The player rejects unknown variants, the wrong protocol or sequence, mismatched handles and positions that cannot be stored as finite f32 values.

## Envelope

bincode 2 uses fixed-width little-endian encoding. `aurora-codegen` emits the protocol version and encoded envelope size from script_api.

| Offset | Type | Field |
| --- | --- | --- |
| 0 | u32 | Protocol version |
| 4 | u64 | Sequence |
| 12 | u32 | Kind |
| 16 | u64 | Opaque node handle |
| 24 | f64 | Local X |
| 32 | f64 | Local Y |

Kind codes are 0 Start, 1 Update, 2 FixedUpdate, 3 Ack, and 4 SetPosition. Startup sends Start once. The player sends FixedUpdate then Update on each 60 Hz tick. The SDK supplies a constant 1/60 second to both callbacks at this milestone. Each command takes effect before the next event. Collision, input, and animation events will be added with their respective subsystems and a protocol version bump.

## Shared memory

The native socket record is the 16-byte envelope header. The corresponding 24-byte payload is published into shared memory before the header is sent. The receiver removes one payload after reading each complete header. Ring 0 (offset 0) carries player requests. Ring 1 (offset 200) carries script replies.

Each ring has a native-endian atomic u32 write cursor at offset 0 and read cursor at offset 4. Eight 24-byte slots follow at offset 8. Counters wrap modulo 2³² and select slots modulo 8. The producer acquires the read cursor before reusing a slot and releases the write cursor after copying. The consumer acquires the write cursor before copying and releases the read cursor after copying. One process owns each cursor for its session. A full ring or a notification without a valid payload ends the session without overwriting data.

Development scripts preserve the same envelope and transaction ordering, sending all 40 bytes on the socket via Bun or Node. This lets the development runtime use ordinary file-descriptor I/O. A bad reply, disconnect, or 250 ms reply deadline ends the session. The windowed player logs the error and continues without the child. Headless and screenshot runs return the error for automated verification.
