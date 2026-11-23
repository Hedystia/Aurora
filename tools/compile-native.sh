#!/usr/bin/env bash
# Link an Aurora script entry against the native IPC transport.
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
entry="${1:?Missing script entry}"
output="${2:?Missing output path}"
stage="$(mktemp -d)"
trap 'rm -rf -- "$stage"' EXIT
compiler="$("$root/tools/setup.sh" --print-scriptc)"
clang -std=c11 -Wall -Wextra -Werror -O2 -c "$root/scripts/build/transport.c" -o "$stage/transport.o"
ar rcs "$stage/libaurora-transport.a" "$stage/transport.o"
cp "$root/scripts/build/ffi.json" "$stage/"
"$compiler" build "$entry" --tier static-only --ffi "$stage/ffi.json" -o "$output" >&2
