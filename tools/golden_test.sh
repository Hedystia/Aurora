#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
case "${1:-}" in
    '') export INSTA_UPDATE=no ;;
    --bless) export INSTA_UPDATE=always ;;
    *) printf 'Usage: %s [--bless]\n' "$0" >&2; exit 2 ;;
esac
export INSTA_FORCE_PASS=0
export AURORA_GOLDEN_SCRIPT
AURORA_GOLDEN_SCRIPT="$(tools/build-script.sh native jumpy)"
cargo test --locked -p player --test golden -- --ignored --nocapture
