#!/usr/bin/env bash
# Print paths to Aurora's selected development tools, or provision all prerequisites.
set -euo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
tools="$root/.aurora/tools"
bun_version=1.2.22
scriptc_version=0.4.16

download() {
    local url="$1" destination="$2" digest="$3"
    curl -fsSL --retry 3 "$url" -o "$destination"
    printf '%s  %s\n' "$digest" "$destination" | sha256sum --check --status
}

install_bun() (
    local asset digest stage
    case "$(uname -m)" in
        x86_64)
            asset=bun-linux-x64-baseline
            digest=f753e8d9668078ab0f598ee26a9ac5acbbb822e057459dd50c191b86524d98e8
            ;;
        aarch64)
            asset=bun-linux-aarch64
            digest=a97c687fb5e54de4e2fb0869a7ac9a2d9c3af75ac182e2b68138c1dd8f98131b
            ;;
        *) printf 'Unsupported Bun architecture\n' >&2; return 1 ;;
    esac
    mkdir -p "$tools"
    stage="$(mktemp -d "$tools/.bun.XXXXXXXX")"
    trap 'rm -rf -- "$stage"' EXIT
    download "https://github.com/oven-sh/bun/releases/download/bun-v${bun_version}/${asset}.zip" \
        "$stage/bun.zip" "$digest"
    unzip -q "$stage/bun.zip" -d "$stage"
    "$stage/$asset/bun" --version >/dev/null
    mv -f "$stage/$asset/bun" "$tools/bun"
    printf '%s\n' "$tools/bun"
)

resolve_runtime() {
    if command -v bun >/dev/null && bun --version >/dev/null 2>&1; then
        command -v bun
    elif [[ -x "$tools/bun" ]] && "$tools/bun" --version >/dev/null 2>&1; then
        printf '%s\n' "$tools/bun"
    elif command -v node >/dev/null &&
        node -e 'process.exit(Number(process.versions.node.split(".")[0]) < 20 ? 1 : 0)' >/dev/null 2>&1; then
        command -v node
    else
        install_bun
    fi
}

resolve_scriptc() (
    local install="$tools/scriptc-$scriptc_version/distribution" path arch digest stage asset
    path="$install/bin/scriptc"
    if [[ -x "$path" ]] && [[ "$("$path" --version)" == "$scriptc_version" ]]; then
        printf '%s\n' "$path"
        return
    fi
    case "$(uname -m)" in
        x86_64)
            arch=x64
            digest=63c9647478cb46783071ebe3ff5282fc10c8e4a110b5db464e06afb90f72e893
            ;;
        aarch64)
            arch=arm64
            digest=0b95901d9ad0bf7c38876a5473f8ce0bccfad0a1f8b4d120eaa857710a869ed8
            ;;
        *) printf 'Unsupported scriptc architecture\n' >&2; return 1 ;;
    esac
    mkdir -p "${install%/*}"
    stage="$(mktemp -d "$tools/.scriptc.XXXXXXXX")"
    trap 'rm -rf -- "$stage"' EXIT
    asset="scriptc-$scriptc_version-linux-$arch-gnu.tar.gz"
    download "https://github.com/vercel-labs/scriptc/releases/download/v$scriptc_version/$asset" \
        "$stage/scriptc.tar.gz" "$digest"
    mkdir "$stage/distribution"
    tar -xzf "$stage/scriptc.tar.gz" -C "$stage/distribution"
    [[ "$("$stage/distribution/bin/scriptc" --version)" == "$scriptc_version" ]]
    if ! mv -T "$stage/distribution" "$install"; then
        [[ -x "$path" && "$("$path" --version)" == "$scriptc_version" ]]
    fi
    printf '%s\n' "$path"
)

ensure_rust() {
    local cargo_bin="${CARGO_HOME:-$HOME/.cargo}/bin"
    if ! command -v rustup >/dev/null && [[ -x "$cargo_bin/rustup" ]]; then
        export PATH="$cargo_bin:$PATH"
    fi
    if ! command -v rustup >/dev/null; then
        local bootstrap
        bootstrap="$(mktemp)"
        if curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs -o "$bootstrap" &&
            sh "$bootstrap" -y --profile minimal --default-toolchain none --no-modify-path; then
            rm -f -- "$bootstrap"
            export PATH="$cargo_bin:$PATH"
        else
            rm -f -- "$bootstrap"
            return 1
        fi
    fi
    rustc --version
}

case "${1:-}" in
    --print-runtime) resolve_runtime ;;
    --print-scriptc) resolve_scriptc ;;
    '')
        cd "$root"
        runtime="$(resolve_runtime)"
        compiler="$(resolve_scriptc)"
        ensure_rust
        if [[ "$(git rev-parse --show-toplevel 2>/dev/null || true)" == "$root" ]] &&
            ! git config --get core.hooksPath >/dev/null; then
            git config --local core.hooksPath tools/hooks
        fi
        printf 'Development runtime %s\nNative compiler %s\n' "$runtime" "$compiler"
        ;;
    *) printf 'Usage %s [--print-runtime|--print-scriptc]\n' "$0" >&2; exit 2 ;;
esac
