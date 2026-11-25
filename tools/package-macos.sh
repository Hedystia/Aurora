#!/usr/bin/env bash
# Package a relocatable macOS host kit for the current Apple architecture.
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

if [[ "$(uname -s)" != Darwin ]]; then
  echo "macOS engine packages must be built on an Apple-branded macOS host." >&2
  exit 2
fi

destination="${1:?Usage: tools/package-macos.sh NEW_ARCHIVE.tar.gz [debug|release]}"
profile="${2:-release}"
case "$profile" in
  debug) flags=() ;;
  release) flags=(--release) ;;
  *) echo "Unknown build profile: $profile" >&2; exit 2 ;;
esac

case "$(uname -m)" in
  arm64) arch=aarch64 ;;
  x86_64) arch=x86_64 ;;
  *) echo "Unsupported macOS architecture: $(uname -m)" >&2; exit 2 ;;
esac

if [[ -e "$destination" ]]; then
  echo "Archive already exists: $destination" >&2
  exit 2
fi

stage="$(mktemp -d "${TMPDIR:-/tmp}/aurora-macos-package.XXXXXX")"
archive="$(mktemp "${destination}.XXXXXXXX")"
trap 'rm -rf -- "$stage"; rm -f -- "$archive"' EXIT

cargo build --locked --workspace "${flags[@]}"
kit="$stage/aurora-macos-$arch"
mkdir -p "$kit/sdk/tools" "$kit/sdk/scripts" "$kit/examples"
mkdir -p "$kit/share/licenses/aurora"
install -m 644 LICENSE "$kit/LICENSE"
install -m 644 LICENSE "$kit/share/licenses/aurora/LICENSE"
cp -R docs/licenses/. "$kit/share/licenses/aurora/"
for binary in aurora-player aurora-editor aurora-codegen aurora; do
  install -m 755 "${CARGO_TARGET_DIR:-target}/$profile/$binary" "$kit/$binary"
done
cp tools/{setup,build-editor-script,compile-native}.sh "$kit/sdk/tools/"
cp -R scripts/{aurora,build} scripts/package.json "$kit/sdk/scripts/"
cp -R scripts/game-template/jumpy "$kit/examples/"
find "$kit/examples" -name .aurora -type d -prune -exec rm -rf {} +

tar -czf "$archive" -C "$stage" "aurora-macos-$arch"
chmod 644 "$archive"
mv -f "$archive" "$destination"
echo "Packaged $destination"
