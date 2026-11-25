#!/usr/bin/env bash
# Build and smoke-test Aurora builder images without enabling GitHub Actions.
#
# Skips with an explicit message when no container engine or daemon is
# reachable, so a developer session without Docker/Podman is not reported as a
# pass. Only the Linux target is exercised end to end.
set -euo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

targets=("$@")
if [[ ${#targets[@]} -eq 0 ]]; then
    targets=(linux)
fi

engine="${AURORA_CONTAINER_ENGINE:-auto}"
if [[ "$engine" == auto ]]; then
    if command -v docker >/dev/null 2>&1; then
        engine=docker
    elif command -v podman >/dev/null 2>&1; then
        engine=podman
    else
        printf 'SKIP: neither Docker nor Podman is installed; no image or game build was tested.\n' >&2
        exit 0
    fi
fi
case "$engine" in
    docker | podman) ;;
    *)
        printf 'Engine must be docker, podman, or auto, not %s\n' "$engine" >&2
        exit 2
        ;;
esac
if ! "$engine" info >/dev/null 2>&1; then
    printf 'SKIP: %s is installed but its daemon is not reachable; no image or game build was tested.\n' "$engine" >&2
    exit 0
fi

version="$(sed -n 's/^version = "\([^"]*\)".*/\1/p' Cargo.toml | head -n 1)"
work="$(mktemp -d)"
tag=""
cleanup() {
    status=$?
    rm -rf -- "$work"
    if [[ -n "$tag" ]]; then
        "$engine" rmi -f "$tag" >/dev/null 2>&1 || true
    fi
    exit "$status"
}
trap cleanup EXIT

for target in "${targets[@]}"; do
    case "$target" in
        linux) ;;
        windows | android)
            printf 'SKIP: %s is a blocked target; only Linux is exercised end to end.\n' "$target" >&2
            continue
            ;;
        macos)
            printf 'SKIP: macOS requires an authorized Apple host (no Linux image).\n' >&2
            continue
            ;;
        *)
            printf 'Unknown target: %s\n' "$target" >&2
            exit 2
            ;;
    esac

    tag="ghcr.io/hedystia/aurora-builder:$version-$target"
    printf '\n== Building %s ==\n' "$tag"
    bash tools/build-builder-image.sh "$target" --engine "$engine" --tag "$tag"

    project="$work/project with space"
    output="$work/output with space"
    cp -a scripts/game-template/jumpy "$project"
    rm -rf -- "$project/.aurora"

    printf '\n== Building and running a scripted Jumpy Bird package ==\n'
    cargo run --quiet --locked --bin aurora -- build \
        --project "$project" \
        --target "$target" \
        --output "$output" \
        --engine "$engine" \
        --keep-image
    test -f "$output/build.json"
    test -f "$output/export.json"
    grep -q '"target": "linux-x86_64"' "$output/build.json"
    grep -q '"engine_version": "'"$version"'"' "$output/build.json"
    "$output/run-game.sh" --headless --frames 5 > "$work/run.json"
    grep -q '"' "$work/run.json"

    printf '\n== Building and running a scriptless package ==\n'
    scriptless="$work/scriptless project"
    scriptless_output="$work/scriptless output"
    mkdir -p "$scriptless"
    printf '{"version":1,"nodes":[{"name":"Root"}]}' > "$scriptless/main.scene"
    cargo run --quiet --locked --bin aurora -- build \
        --project "$scriptless" \
        --target "$target" \
        --output "$scriptless_output" \
        --engine "$engine" \
        --keep-image
    test -f "$scriptless_output/export.json"
    if find "$scriptless_output" -name game -type f | grep -q .; then
        printf 'FAIL: a scriptless package contains a native script program.\n' >&2
        exit 1
    fi
    "$scriptless_output/run-game.sh" --headless --frames 5 > "$work/scriptless.json"
    grep -q '"' "$work/scriptless.json"

    printf '\n== Checking cleanup after a failed build ==\n'
    mkdir -p "$work/not a project"
    if cargo run --quiet --locked --bin aurora -- build \
        --project "$work/not a project" \
        --target "$target" \
        --output "$work/failed output" \
        --engine "$engine"; then
        printf 'FAIL: an invalid project produced a successful build.\n' >&2
        exit 1
    fi
    if "$engine" image inspect "$tag" >/dev/null 2>&1; then
        printf 'FAIL: builder image survived a failed build.\n' >&2
        exit 1
    fi

    printf '\n== Checking cleanup after a successful build ==\n'
    bash tools/build-builder-image.sh "$target" --engine "$engine" --tag "$tag"
    third_output="$work/third output"
    cargo run --quiet --locked --bin aurora -- build \
        --project "$project" \
        --target "$target" \
        --output "$third_output" \
        --engine "$engine"
    if "$engine" image inspect "$tag" >/dev/null 2>&1; then
        printf 'FAIL: builder image survived a successful ephemeral build.\n' >&2
        exit 1
    fi

    printf '\nPASS: %s builder image built, packaged Jumpy Bird, ran headless, and cleaned up on success and failure.\n' "$target"
done
