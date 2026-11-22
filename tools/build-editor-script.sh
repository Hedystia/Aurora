#!/usr/bin/env bash
# Compile the editor-generated entry and print the path of a socket client launcher.
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
work="${1:?Preview workspace required}"
runtime="$("$root/tools/setup.sh" --print-runtime)"
cargo run --quiet --locked --manifest-path "$root/Cargo.toml" --bin aurora-codegen -- "$work/entry.ts" "$work/metadata"
mkdir -p "$root/.aurora/scripts/metadata"
cp "$work/metadata/protocol.mts" "$root/.aurora/scripts/metadata/protocol.mts"
if "$runtime" -e 'process.exit(process.versions.bun ? 0 : 1)'; then
    "$runtime" build "$work/entry.ts" --target=node --tsconfig-override "$work/tsconfig.json" --outfile="$work/entry.mjs" >&2
else
    if [[ ! -x "$root/scripts/node_modules/.bin/esbuild" ]]; then
        npm install --prefix "$root/scripts" --ignore-scripts --no-package-lock >&2
    fi
    "$root/scripts/node_modules/.bin/esbuild" "$work/entry.ts" --bundle --platform=node --format=esm \
        --tsconfig="$work/tsconfig.json" --outfile="$work/entry.mjs" >&2
fi
printf '#!/bin/bash\nexec %q %q\n' "$runtime" "$work/entry.mjs" > "$work/game-script"
chmod +x "$work/game-script"
