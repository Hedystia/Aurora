# Script protocol

M0 establishes the script_api crate. The first wire format and protocol version land together in M2. The planned transport uses Unix socket control messages and one single-producer, single-consumer shared-memory ring per direction for frame data. Record each subsequent wire change and bump its protocol version.
