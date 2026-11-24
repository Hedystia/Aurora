#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

runtime="$(tools/setup.sh --print-runtime)"
compiler="$root/scripts/node_modules/typescript/bin/tsc"
if [[ ! -f "$compiler" ]]; then
    printf 'TypeScript is missing; install the scripts workspace dependencies first.\n' >&2
    exit 1
fi

stage="$(mktemp -d /tmp/aurora-sdk-types.XXXXXXXX)"
trap 'rm -rf -- "$stage"' EXIT
"$runtime" "$compiler" --declaration --emitDeclarationOnly --outDir "$stage" \
    --target ES2022 --module ESNext --moduleResolution Bundler \
    --allowImportingTsExtensions --experimentalDecorators \
    "$root/scripts/aurora/index.ts"

mkdir -p crates/codegen/src/sdk
for module in index events resources scheduler; do
    sed -E 's/(from "\.\/[^"]+)\.ts"/\1.js"/g' \
        "$stage/$module.d.ts" > "crates/codegen/src/sdk/$module.d.ts"
done
