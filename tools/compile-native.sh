#!/usr/bin/env bash
# Link an Aurora script entry against the native IPC transport.
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
entry="${1:?Missing script entry}"
output="${2:?Missing output path}"
stage="$(mktemp -d)"
trap 'rm -rf -- "$stage"' EXIT
compiler="$("$root/tools/setup.sh" --print-scriptc)"
platform="${AURORA_SCRIPT_PLATFORM:-$(uname -s | tr '[:upper:]' '[:lower:]')}"
case "$platform" in
  mingw*|msys*|windows*|win32*)
    transport=transport_win.c
    manifest=ffi-win.json
    compiler_cc="${CC:-x86_64-w64-mingw32-gcc}"
    archiver="${AR:-x86_64-w64-mingw32-ar}"
    ;;
  *)
    transport=transport.c
    manifest=ffi.json
    compiler_cc="${CC:-clang}"
    archiver="${AR:-ar}"
    ;;
esac
"$compiler_cc" -std=c11 -Wall -Wextra -Werror -O2 \
  -c "$root/scripts/build/$transport" -o "$stage/transport.o"
"$archiver" rcs "$stage/libaurora-transport.a" "$stage/transport.o"
cp "$root/scripts/build/$manifest" "$stage/ffi.json"
"$compiler" build "$entry" --tier static-only --ffi "$stage/ffi.json" -o "$output" >&2
