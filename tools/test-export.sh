#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

cargo build --locked --bin aurora-editor --bin aurora-player
work="$(mktemp -d /tmp/aurora-export.XXXXXXXX)"
trap 'rm -rf -- "$work"' EXIT
destination="$work/JumpyExport"

target/debug/aurora-editor \
    --project scripts/game-template/jumpy \
    --export "$destination" \
    >"$work/export.log" 2>&1

for file in \
    export.json \
    main.scene \
    game \
    aurora-player \
    run-game.sh; do
    if [[ ! -f "$destination/$file" ]]; then
        printf 'Missing export file %s\n' "$file" >&2
        sed -n '1,120p' "$work/export.log" >&2
        exit 1
    fi
done
[[ -x "$destination/game" ]]
[[ -x "$destination/aurora-player" ]]
[[ -x "$destination/run-game.sh" ]]

python3 - "$destination/export.json" <<'PY'
import json
import pathlib
import sys

manifest = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert manifest.get("version"), "export manifest has no version"
files = manifest.get("files")
assert isinstance(files, list) and files, "export manifest has no files"
for entry in files:
    path = entry.get("path")
    assert path, "export manifest entry has no path"
    assert (pathlib.Path(sys.argv[1]).parent / path).is_file(), f"missing packaged file: {path}"
PY

"$destination/run-game.sh" \
    --headless --frames 2 \
    --asset-cache "$work/cache" \
    >"$work/state.json"

python3 - "$work/state.json" <<'PY'
import json
import pathlib
import sys

state = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert "nodes" in state or "entities" in state, "headless bundle produced no scene state"
PY

# An explicit target stamps the manifest and renames the runtime without
# executing the cross-built player on this host.
fake_player="$work/cross-built-player.exe"
printf 'cross-built player' > "$fake_player"
cross="$work/CrossExport"
target/debug/aurora-editor \
    --project scripts/game-template/jumpy \
    --export "$cross" \
    --export-target windows-x86_64 \
    --player "$fake_player" \
    >"$work/export-cross.log" 2>&1

for file in export.json main.scene game aurora-player.exe run-game.cmd; do
    if [[ ! -f "$cross/$file" ]]; then
        printf 'Missing cross-target export file %s\n' "$file" >&2
        sed -n '1,120p' "$work/export-cross.log" >&2
        exit 1
    fi
done
[[ ! -e "$cross/aurora-player" ]]
[[ ! -e "$cross/run-game.sh" ]]
grep -q 'aurora-player.exe' "$cross/run-game.cmd"
python3 - "$cross/export.json" "$cross" <<'PY'
import hashlib
import json
import pathlib
import sys

manifest_path = pathlib.Path(sys.argv[1])
manifest = json.loads(manifest_path.read_text())
assert manifest["target"] == "windows-x86_64", manifest["target"]
names = {entry["path"] for entry in manifest["files"]}
assert "aurora-player.exe" in names and "run-game.cmd" in names, names
for entry in manifest["files"]:
    payload = (manifest_path.parent / entry["path"]).read_bytes()
    assert hashlib.sha256(payload).hexdigest() == entry["sha256"], entry["path"]
PY

# An asset the scene never references must still ship in the bundle and be hashed.
source="$work/source"
cp -R scripts/game-template/jumpy "$source"
cp "$source/assets/sky.png" "$source/assets/unreferenced.png"
full="$work/FullExport"
target/debug/aurora-editor \
    --project "$source" \
    --export "$full" \
    >"$work/export-full.log" 2>&1

python3 - "$full/export.json" <<'PY'
import hashlib
import json
import pathlib
import sys

manifest_path = pathlib.Path(sys.argv[1])
manifest = json.loads(manifest_path.read_text())
entries = {entry["path"]: entry for entry in manifest["files"]}
relative = "assets/unreferenced.png"
assert relative in entries, "unreferenced on-demand asset is missing from the manifest"
payload = (manifest_path.parent / relative).read_bytes()
assert hashlib.sha256(payload).hexdigest() == entries[relative]["sha256"], (
    "on-demand asset hash does not match the manifest"
)
PY

"$full/run-game.sh" \
    --headless --frames 2 \
    --asset-cache "$work/cache-full" \
    >"$work/state-full.json"

printf 'export smoke test passed: %s\n' "$destination"
