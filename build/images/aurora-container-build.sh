#!/bin/sh
# Runs Aurora's project export inside a builder image.
#
# The host mounts the project read-only at $AURORA_PROJECT and a writable
# staging directory at the parent of $AURORA_OUTPUT. This script never
# downloads anything: dependencies are baked into the image.
set -eu

project=${AURORA_PROJECT:-}
output=${AURORA_OUTPUT:-}
target=${AURORA_TARGET:-}

if [ -z "$project" ] || [ -z "$output" ] || [ -z "$target" ]; then
    echo "aurora-container-build: AURORA_PROJECT, AURORA_OUTPUT and AURORA_TARGET are required" >&2
    exit 2
fi

case "$target" in
    linux) ;;
    windows)
        echo "aurora-container-build: Windows game packaging is not supported yet." >&2
        echo "Reason: script IPC is Unix-only and export stamps the compiling host target." >&2
        exit 3
        ;;
    android)
        echo "aurora-container-build: Android game packaging is not supported yet." >&2
        echo "Reason: Aurora has no Android application runtime (Activity/SurfaceView shell)." >&2
        exit 3
        ;;
    *)
        echo "aurora-container-build: unknown target $target" >&2
        exit 2
        ;;
esac

if [ ! -e "$project" ]; then
    echo "aurora-container-build: project or scene is missing: $project" >&2
    exit 2
fi
if [ -e "$output" ]; then
    echo "aurora-container-build: output already exists: $output" >&2
    exit 2
fi

cd /opt/aurora
export HOME=${HOME:-/home/builder}

set -- --project "$project" --export "$output"
if [ -n "${AURORA_EXPORT_TARGET:-}" ]; then
    set -- "$@" --export-target "$AURORA_EXPORT_TARGET"
fi
if [ -n "${AURORA_PLAYER:-}" ]; then
    set -- "$@" --player "$AURORA_PLAYER"
fi
if [ -n "${AURORA_INCLUDE_SCENES:-}" ]; then
    previous_ifs=$IFS
    IFS='
'
    for scene in $AURORA_INCLUDE_SCENES; do
        set -- "$@" --include-scene "$scene"
    done
    IFS=$previous_ifs
fi

exec ./target/release/aurora-editor "$@"
