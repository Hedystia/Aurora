#!/usr/bin/env bash
# Build and smoke-test a scriptless macOS .app on an Apple-branded host.
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

if [[ "$(uname -s)" != Darwin ]]; then
  echo "macOS app packaging must run on an Apple-branded macOS host." >&2
  exit 2
fi

output="${1:-}"
profile="${2:-debug}"
case "$profile" in
  debug) flags=() ;;
  release) flags=(--release) ;;
  *) echo "Unknown build profile: $profile" >&2; exit 2 ;;
esac

if [[ -n "$output" && -e "$output" ]]; then
  echo "Smoke-test output already exists: $output" >&2
  exit 2
fi

case "$(uname -m)" in
  arm64)
    expected_target=macos-aarch64
    expected_triple=aarch64-apple-darwin
    ;;
  x86_64)
    expected_target=macos-x86_64
    expected_triple=x86_64-apple-darwin
    ;;
  *)
    echo "Unsupported macOS architecture: $(uname -m)" >&2
    exit 2
    ;;
esac

stage="$(mktemp -d "${TMPDIR:-/tmp}/aurora-macos-test.XXXXXX")"
trap 'rm -rf -- "$stage"' EXIT

mkdir -p "$stage/project"
cat > "$stage/project/main.scene" <<'SCENE'
{
  "version": 1,
  "nodes": [{ "name": "Window" }]
}
SCENE

cargo build --locked --workspace "${flags[@]}"
bin="${CARGO_TARGET_DIR:-target}/$profile"
app="$stage/Game.app"

"$bin/aurora" build \
  --project "$stage/project/main.scene" \
  --target macos \
  --output "$app" \
  --sign -

test -x "$app/Contents/MacOS/aurora-player"
test -f "$app/Contents/Info.plist"
test -f "$app/Contents/PkgInfo"
test -f "$app/Contents/Resources/game/export.json"
test -f "$app/Contents/Resources/game/build.json"
test ! -e "$app/Contents/Resources/game/aurora-player"
test ! -e "$app/Contents/Resources/game/run-game.sh"
plutil -lint "$app/Contents/Info.plist" >/dev/null
test "$(plutil -extract CFBundleIdentifier raw -o - "$app/Contents/Info.plist")" = dev.aurora.game
codesign --verify --deep --strict "$app"

python3 - "$app" "$expected_target" "$expected_triple" <<'PY'
import json
import pathlib
import sys

app = pathlib.Path(sys.argv[1])
target = sys.argv[2]
triple = sys.argv[3]
game = app / "Contents" / "Resources" / "game"
export = json.loads((game / "export.json").read_text())
build = json.loads((game / "build.json").read_text())
assert export["target"] == target, (export["target"], target)
assert build["target"] == target, (build["target"], target)
assert build["target_triple"] == triple, (build["target_triple"], triple)
assert build["builder_kind"] == "host", build["builder_kind"]
assert build["builder_image"] is None, build["builder_image"]
assert build["builder_image_id"] is None, build["builder_image_id"]
PY

"$app/Contents/MacOS/aurora-player" --headless --frames 60 > "$stage/run.json"
python3 - "$stage/run.json" <<'PY'
import json
import sys

scene = json.load(open(sys.argv[1]))
assert scene["ticks"] == 60, scene["ticks"]
assert scene["nodes"], scene["nodes"]
PY

if [[ -n "$output" ]]; then
  mkdir -p "$(dirname -- "$output")"
  cp -R "$app" "$output"
  echo "Copied smoke-test app to $output"
fi
echo "macOS app smoke test passed for $expected_target"
