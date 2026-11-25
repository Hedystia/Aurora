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

printf 'export smoke test passed: %s\n' "$destination"
