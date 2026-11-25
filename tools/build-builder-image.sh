#!/usr/bin/env bash
# Build one Aurora build-toolchain image locally with Docker or Podman.
set -euo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

usage() {
    cat >&2 <<'EOF'
Usage: tools/build-builder-image.sh TARGET [options]

TARGET is linux, windows, or android. Windows and Android images are blocked
toolchain groundwork and need --allow-blocked.

Options:
  --engine docker|podman   Container engine (default: AURORA_CONTAINER_ENGINE or auto)
  --tag REF                Image reference (default: ghcr.io/hedystia/aurora-builder:VERSION-TARGET)
  --allow-blocked          Build a blocked target's toolchain image (never publishable)
  -h, --help               Show this help
EOF
}

target="${1:-}"
if [[ -z "$target" ]]; then
    usage
    exit 2
fi
shift

engine="${AURORA_CONTAINER_ENGINE:-auto}"
tag=""
allow_blocked=0
while (($#)); do
    case "$1" in
        --engine)
            engine="${2:?Missing engine}"
            shift 2
            ;;
        --engine=*)
            engine="${1#--engine=}"
            shift
            ;;
        --tag)
            tag="${2:?Missing tag}"
            shift 2
            ;;
        --tag=*)
            tag="${1#--tag=}"
            shift
            ;;
        --allow-blocked)
            allow_blocked=1
            shift
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        *)
            printf 'Unknown option: %s\n' "$1" >&2
            usage
            exit 2
            ;;
    esac
done

case "$target" in
    linux | windows | android) ;;
    macos)
        printf 'macOS images are not available: Apple SDK/Xcode licensing requires an authorized macOS host.\n' >&2
        exit 2
        ;;
    *)
        printf 'Unknown target: %s\n' "$target" >&2
        exit 2
        ;;
esac
if [[ "$target" != linux && $allow_blocked -ne 1 ]]; then
    printf 'Target %s is blocked; pass --allow-blocked to build its unpublishable toolchain groundwork.\n' "$target" >&2
    exit 2
fi

version="$(sed -n 's/^version = "\([^"]*\)".*/\1/p' Cargo.toml | head -n 1)"
if [[ -z "$version" ]]; then
    printf 'Could not read the workspace version from Cargo.toml\n' >&2
    exit 2
fi
tag="${tag:-ghcr.io/hedystia/aurora-builder:$version-$target}"

case "$engine" in
    auto)
        if command -v docker >/dev/null 2>&1; then
            engine=docker
        elif command -v podman >/dev/null 2>&1; then
            engine=podman
        else
            printf 'Docker or Podman is required. Install one locally; Docker Desktop is not required.\n' >&2
            exit 2
        fi
        ;;
    docker | podman) ;;
    *)
        printf 'Engine must be docker, podman, or auto, not %s\n' "$engine" >&2
        exit 2
        ;;
esac

if ! "$engine" info >/dev/null 2>&1; then
    printf '%s is installed but its daemon is not reachable.\n' "$engine" >&2
    printf 'Start it, add your user to the docker group, or use rootless Podman. Docker Desktop is not required.\n' >&2
    exit 2
fi

printf 'Building %s with %s (Aurora %s)\n' "$tag" "$engine" "$version"
"$engine" build \
    --file "build/images/$target/Containerfile" \
    --tag "$tag" \
    --build-arg "AURORA_VERSION=$version" \
    .

image_id="$("$engine" image inspect --format '{{.Id}}' "$tag")"
printf '\nBuilt %s\nImage ID: %s\n' "$tag" "$image_id"
printf 'This is a build-toolchain image, not an engine release. Publishing requires a reviewed, deliberately triggered workflow.\n'
