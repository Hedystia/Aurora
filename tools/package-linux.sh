#!/usr/bin/env bash
# Package a relocatable, user-writable Linux development kit.
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
destination="${1:?Usage: tools/package-linux.sh NEW_ARCHIVE.tar.gz [debug|release]}"
profile="${2:-release}"
case "$profile" in
  debug) flags=() ;;
  release) flags=(--release) ;;
  *) echo "Unknown build profile: $profile" >&2; exit 2 ;;
esac
stage="$(mktemp -d)"
archive="$(mktemp -- "${destination}.XXXXXXXX")"
trap 'rm -rf -- "$stage"; rm -f -- "$archive"' EXIT
cargo build --locked --workspace "${flags[@]}"
kit="$stage/aurora"
mkdir -p "$kit/sdk/tools" "$kit/sdk/scripts" "$kit/examples"
mkdir -p "$kit/share/licenses/aurora"
install -m 644 LICENSE "$kit/LICENSE"
install -m 644 LICENSE "$kit/share/licenses/aurora/LICENSE"
cp -R docs/licenses/. "$kit/share/licenses/aurora/"
for binary in aurora-player aurora-editor aurora-codegen; do
  install -s "${CARGO_TARGET_DIR:-target}/$profile/$binary" "$kit/$binary"
done
cp tools/{setup,build-editor-script,compile-native}.sh "$kit/sdk/tools/"
cp -R scripts/{aurora,build} scripts/package.json "$kit/sdk/scripts/"
cp -R scripts/game-template/jumpy "$kit/examples/"
# Local caches and generated SDK files do not belong in the sample.
find "$kit/examples" -name .aurora -type d -prune -exec rm -rf -- {} +
tar -czf "$archive" -C "$stage" aurora
chmod 644 "$archive"
ln -- "$archive" "$destination"
