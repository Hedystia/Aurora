#!/usr/bin/env bash
# Assemble a verified target-template archive for one export target.
#
# The archive is flat: template.json plus the target player and launcher. The
# manifest records the engine version, target, triple, a SHA-256 per file and
# a digest over the sorted file/hash pairs. `aurora build --template` verifies
# all of them before caching the archive contents.
set -euo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
max_files=8
max_bytes=$((512 * 1024 * 1024))

usage() {
    printf 'Usage: tools/package-target-template.sh <linux|windows|macos> <archive.tar.gz> [debug|release]\n' >&2
    exit 2
}

target=${1:-}
archive=${2:-}
profile=${3:-debug}
[[ -n "$target" && -n "$archive" ]] || usage
case "$profile" in
    debug | release) ;;
    *) usage ;;
esac

manifest_target=""
triple=""
player=""
launcher=""
case "$target" in
    linux)
        manifest_target=linux-x86_64
        triple=x86_64-unknown-linux-gnu
        player=aurora-player
        launcher=run-game.sh
        ;;
    windows)
        manifest_target=windows-x86_64
        triple=x86_64-pc-windows-gnu
        player=aurora-player.exe
        launcher=run-game.cmd
        ;;
    macos)
        case "$(uname -m)" in
            arm64 | aarch64)
                manifest_target=macos-aarch64
                triple=aarch64-apple-darwin
                ;;
            x86_64)
                manifest_target=macos-x86_64
                triple=x86_64-apple-darwin
                ;;
            *)
                printf 'Unsupported macOS architecture: %s\n' "$(uname -m)" >&2
                exit 2
                ;;
        esac
        player=aurora-player
        launcher=run-game.sh
        ;;
    android)
        printf 'Android target templates are not supported: no Android application runtime.\n' >&2
        exit 3
        ;;
    *)
        printf 'Unknown target: %s\n' "$target" >&2
        exit 2
        ;;
esac

binary=""
candidate=""
for candidate in \
    "$root/target/$profile/$player" \
    "$root/target/$triple/$profile/$player"; do
    if [[ -f "$candidate" && ! -L "$candidate" ]]; then
        binary=$candidate
        break
    fi
done
if [[ -z "$binary" ]]; then
    printf 'Missing player binary %s; build it first, for example:\n' "$player" >&2
    printf '  cargo build --locked --profile %s --bin aurora-player --target %s\n' "$profile" "$triple" >&2
    exit 2
fi

hash_file() {
    if command -v sha256sum >/dev/null; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

version="$(
    cargo metadata --locked --no-deps --format-version 1 |
        python3 -c 'import json,sys; packages=json.load(sys.stdin)["packages"]; assert packages; print(packages[0]["version"])'
)"
work="$(mktemp -d "${TMPDIR:-/tmp}/aurora-template.XXXXXXXX")"
trap 'rm -rf -- "$work"' EXIT
stage="$work/stage"
mkdir "$stage"
cp -- "$binary" "$stage/$player"

if [[ "$launcher" == run-game.cmd ]]; then
    printf '@echo off\r\n"%%~dp0aurora-player.exe" --bundle "%%~dp0" %%*\r\n' > "$stage/$launcher"
else
    cat > "$stage/$launcher" <<'EOF'
#!/bin/sh
set -eu
cd -- "$(dirname -- "$0")"
exec ./aurora-player --bundle . "$@"
EOF
    chmod 755 "$stage/$launcher"
fi

player_hash="$(hash_file "$stage/$player")"
launcher_hash="$(hash_file "$stage/$launcher")"

python3 - "$stage/template.json" "$version" "$manifest_target" "$triple" \
    "$player" "$player_hash" "$launcher" "$launcher_hash" <<'PY'
import hashlib
import json
import pathlib
import sys

destination = pathlib.Path(sys.argv[1])
version, manifest_target, triple = sys.argv[2:5]
names = sys.argv[5:]
files = [
    {"path": names[index], "sha256": names[index + 1]}
    for index in range(0, len(names), 2)
]
files.sort(key=lambda entry: entry["path"])
digest = hashlib.sha256()
for entry in files:
    path = entry["path"]
    digest.update(len(path).to_bytes(8, "little"))
    digest.update(path.encode("utf-8"))
    digest.update(entry["sha256"].encode("ascii"))
manifest = {
    "version": 1,
    "engine_version": version,
    "target": manifest_target,
    "triple": triple,
    "bundle_digest": digest.hexdigest(),
    "files": files,
}
destination.write_text(json.dumps(manifest, indent=2) + "\n")
PY

count=0
for file in "$stage"/*; do
    count=$((count + 1))
    size="$(wc -c < "$file")"
    if ((size > max_bytes)); then
        printf 'Template file exceeds the size limit: %s\n' "$file" >&2
        exit 2
    fi
done
if ((count > max_files)); then
    printf 'Template has too many files: %s\n' "$count" >&2
    exit 2
fi

mkdir -p -- "$(dirname -- "$archive")"
tar -czf "$archive" -C "$stage" template.json "$player" "$launcher"

# Verify the published archive itself, not just the staged files.
mapfile -t names < <(tar -tzf "$archive")
if ((${#names[@]} != 3)); then
    printf 'Template archive has an unexpected entry count: %s\n' "${#names[@]}" >&2
    exit 1
fi
verify="$work/verify"
mkdir "$verify"
tar -xzf "$archive" -C "$verify"
python3 - "$verify/template.json" "$verify" "$manifest_target" "$triple" "$version" <<'PY'
import hashlib
import json
import pathlib
import sys

manifest = json.loads(pathlib.Path(sys.argv[1]).read_text())
root = pathlib.Path(sys.argv[2])
assert manifest["target"] == sys.argv[3], "wrong template target"
assert manifest["triple"] == sys.argv[4], "wrong template triple"
assert manifest["engine_version"] == sys.argv[5], "wrong engine version"
digest = hashlib.sha256()
files = sorted(manifest["files"], key=lambda entry: entry["path"])
for entry in files:
    payload = (root / entry["path"]).read_bytes()
    assert hashlib.sha256(payload).hexdigest() == entry["sha256"], entry["path"]
    digest.update(len(entry["path"]).to_bytes(8, "little"))
    digest.update(entry["path"].encode("utf-8"))
    digest.update(entry["sha256"].encode("ascii"))
assert digest.hexdigest() == manifest["bundle_digest"], "wrong bundle digest"
assert {entry.name for entry in root.iterdir()} == {"template.json"} | {entry["path"] for entry in files}
PY

printf 'Target template: %s\n' "$archive"
printf '  target %s (%s), engine %s\n' "$manifest_target" "$triple" "$version"
printf '  player %s\n' "$player_hash"
printf '  launcher %s\n' "$launcher_hash"
