#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
sample="${2:-hello}"
case "$sample" in hello|bird|scene|jumpy) ;; *) printf 'Unknown sample %s\n' "$sample" >&2; exit 2 ;; esac
source="scripts/game-template/$sample.ts"
if [[ "$sample" == jumpy ]]; then source="scripts/game-template/jumpy/assets/Jumpy.ts"; fi
output="$root/.aurora/scripts"
mkdir -p "$output"
stage="$(mktemp -d "$output/.build.XXXXXXXX")"
trap 'rm -rf -- "$stage"' EXIT

case "${1:-}" in native|dev)
    cargo run --quiet --locked --bin aurora-codegen -- "$source" "$output/metadata/$sample"
    mv -f "$output/metadata/$sample/protocol.mts" "$output/metadata/protocol.mts"
    ;;
esac

case "${1:-}" in
    native)
        compiler="$(tools/setup.sh --print-scriptc)"
        clang -std=c11 -Wall -Wextra -Werror -O2 -c scripts/build/transport.c -o "$stage/transport.o"
        ar rcs "$stage/libaurora-transport.a" "$stage/transport.o"
        cp scripts/build/ffi.json "$stage/"
        "$compiler" build scripts/build/"$sample-native.ts" --tier static-only --ffi "$stage/ffi.json" -o "$stage/hello" >&2
        mv -f "$stage/hello" "$output/$sample-native"
        printf '%s\n' "$output/$sample-native"
        ;;
    dev)
        runtime="$(tools/setup.sh --print-runtime)"
        entry="$root/scripts/build/$sample-dev.ts"
        if ! "$runtime" -e 'process.exit(process.versions.bun ? 0 : 1)' &&
           ! "$runtime" --experimental-strip-types -e '' >/dev/null 2>&1; then
            if [[ ! -x scripts/node_modules/.bin/esbuild ]]; then
                npm install --prefix scripts --ignore-scripts --no-package-lock >&2
            fi
            scripts/node_modules/.bin/esbuild "$entry" --bundle --platform=node --format=esm --outfile="$stage/hello.mjs" >&2
            mv -f "$stage/hello.mjs" "$output/$sample.mjs"
            entry="$output/$sample.mjs"
        elif "$runtime" -e 'process.exit(process.versions.bun ? 1 : 0)'; then
            runtime_args=(--experimental-strip-types)
        fi
        {
            printf '#!/bin/bash\nexec %q ' "$runtime"
            if [[ -v 'runtime_args[0]' ]]; then printf '%q ' "${runtime_args[@]}"; fi
            printf '%q\n' "$entry"
        } > "$stage/hello"
        chmod +x "$stage/hello"
        mv -f "$stage/hello" "$output/$sample-dev"
        printf '%s\n' "$output/$sample-dev"
        ;;
    *) printf 'Usage: %s native|dev [hello|bird|scene|jumpy]\n' "$0" >&2; exit 2 ;;
esac
