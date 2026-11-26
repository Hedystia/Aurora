#!/bin/sh
# Aurora Android builder image entry point.
#
# The host mounts the project read-only at $AURORA_PROJECT and a writable
# staging directory at the parent of $AURORA_OUTPUT. This script builds the
# Android cdylib and assembles a debug APK with the SDK/NDK tools baked into
# the image. It never downloads anything at build time.
#
# UNVERIFIED: no Android SDK/NDK, device or emulator was available where this
# script was written. A clear error is emitted when the toolchain is absent.
set -eu

project=${AURORA_PROJECT:-}
output=${AURORA_OUTPUT:-}
target=${AURORA_TARGET:-}

if [ -z "$project" ] || [ -z "$output" ] || [ -z "$target" ]; then
    echo "android-container-build: AURORA_PROJECT, AURORA_OUTPUT and AURORA_TARGET are required" >&2
    exit 2
fi
if [ "$target" != "android" ]; then
    echo "android-container-build: this entry point only builds the android target, got '$target'" >&2
    exit 2
fi
if [ ! -e "$project" ]; then
    echo "android-container-build: project or scene is missing: $project" >&2
    exit 2
fi
if [ -e "$output" ]; then
    echo "android-container-build: output already exists: $output" >&2
    exit 2
fi

sdk=${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}
ndk=${ANDROID_NDK_HOME:-}
platform=${ANDROID_PLATFORM:-android-34}
if [ -z "$sdk" ] || [ ! -d "$sdk" ] || [ -z "$ndk" ] || [ ! -d "$ndk" ]; then
    echo "android-container-build: Android SDK/NDK are not installed in this image." >&2
    echo "Set ANDROID_HOME and ANDROID_NDK_HOME to the provisioned toolchain." >&2
    exit 3
fi

build_tools=""
if [ -n "${ANDROID_BUILD_TOOLS:-}" ] && [ -d "$sdk/build-tools/$ANDROID_BUILD_TOOLS" ]; then
    build_tools="$sdk/build-tools/$ANDROID_BUILD_TOOLS"
else
    for candidate in "$sdk"/build-tools/*; do
        if [ -d "$candidate" ]; then
            build_tools=$candidate
        fi
    done
fi
if [ -z "$build_tools" ] || [ ! -x "$build_tools/aapt2" ] \
    || [ ! -x "$build_tools/zipalign" ] || [ ! -x "$build_tools/apksigner" ]; then
    echo "android-container-build: SDK build-tools (aapt2, zipalign, apksigner) are missing." >&2
    exit 3
fi
if [ ! -f "$sdk/platforms/$platform/android.jar" ]; then
    echo "android-container-build: Android platform $platform is missing from the SDK." >&2
    exit 3
fi
for tool in cargo zip keytool python3; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "android-container-build: required host tool '$tool' is missing." >&2
        exit 3
    fi
done

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT HUP INT TERM
export HOME=${HOME:-/home/builder}

cd /opt/aurora
echo "android-container-build: compiling libmain.so for aarch64-linux-android"
cargo build --locked --release \
    --target aarch64-linux-android \
    --manifest-path crates/android_player/Cargo.toml
so=target/aarch64-linux-android/release/libmain.so
if [ ! -f "$so" ]; then
    echo "android-container-build: cargo did not produce $so" >&2
    exit 4
fi

echo "android-container-build: assembling unsigned APK"
mkdir -p "$stage/gen" "$stage/native/lib/arm64-v8a"
cp "$so" "$stage/native/lib/arm64-v8a/libmain.so"
"$build_tools/aapt2" link \
    --manifest crates/android_player/AndroidManifest.xml \
    -I "$sdk/platforms/$platform/android.jar" \
    --min-sdk-version 26 --target-sdk-version 34 \
    --version-code 1 --version-name 0.1.0 \
    --java "$stage/gen" \
    -o "$stage/unsigned.apk"
(cd "$stage/native" && zip -q -r "$stage/unsigned.apk" lib)

echo "android-container-build: signing a debug APK"
mkdir -p "$HOME/.android"
keystore="$HOME/.android/debug.keystore"
if [ ! -f "$keystore" ]; then
    keytool -genkeypair -keystore "$keystore" \
        -storepass android -keypass android \
        -alias androiddebugkey -dname "CN=Android Debug,O=Android,C=US" \
        -keyalg RSA -keysize 2048 -validity 10000 -noprompt
fi
"$build_tools/zipalign" -f -p 4 "$stage/unsigned.apk" "$stage/aligned.apk"
"$build_tools/apksigner" sign \
    --ks "$keystore" --ks-key-alias androiddebugkey \
    --ks-pass pass:android --key-pass pass:android \
    --out "$stage/aurora-debug.apk" "$stage/aligned.apk"
"$build_tools/apksigner" verify "$stage/aurora-debug.apk"

echo "android-container-build: exporting the authored scene"
set -- --project "$project" --export "$stage/export"
if [ -n "${AURORA_INCLUDE_SCENES:-}" ]; then
    previous_ifs=$IFS
    IFS='
'
    for scene in $AURORA_INCLUDE_SCENES; do
        set -- "$@" --include-scene "$scene"
    done
    IFS=$previous_ifs
fi
./target/release/aurora-editor "$@"
# The desktop player binary is not usable on Android; the APK above replaces it.
rm -f "$stage/export/aurora-player"

mkdir -p "$output"
cp -R "$stage/export/." "$output/"
cp "$stage/aurora-debug.apk" "$output/aurora-debug.apk"

# The editor stamps the export with the compiling host target. Rewrite the
# manifest so it describes the Android package and covers the added APK.
python3 - "$output" "android-aarch64" <<'PY'
import hashlib
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
target = sys.argv[2]
existing = json.loads((root / "export.json").read_text())
files = []
for path in sorted(p for p in root.rglob("*") if p.is_file() and p.name != "export.json"):
    files.append(
        {
            "path": path.relative_to(root).as_posix(),
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        }
    )
files.sort(key=lambda item: item["path"])
bundle = b"".join(
    len(item["path"]).to_bytes(8, "little") + item["path"].encode() + item["sha256"].encode()
    for item in files
)
manifest = {
    "version": existing["version"],
    "engine_version": existing["engine_version"],
    "target": target,
    "source_bundle_sha256": hashlib.sha256(bundle).hexdigest(),
    "files": files,
}
(root / "export.json").write_text(json.dumps(manifest, indent=2) + "\n")
PY

echo "android-container-build: wrote $output (APK: aurora-debug.apk)"
