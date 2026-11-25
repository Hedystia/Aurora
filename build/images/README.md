# Builder images

These OCI images build Aurora game packages; they are not engine releases.

| Directory | Target | Status |
| --- | --- | --- |
| `linux/` | Linux x86-64 | Supported. Build with `tools/build-builder-image.sh linux`; smoke-test with `tools/test-builder-images.sh`. |
| `windows/` | Windows x86-64 | Blocked. MinGW cross-toolchain groundwork only; script IPC and export target stamping prevent a package. Requires `--allow-blocked`. |
| `android/` | Android ARM64 | Blocked. Provisions command-line SDK/NDK but Aurora has no Android application runtime. Requires `--allow-blocked`. Do not publish without reviewing Google's SDK/NDK terms. |

macOS has no image. `aurora build --target macos` is a host-only path on an Apple-branded Mac because Apple's SLA permits macOS virtualization only on Apple hardware; `dockurr/macos`-style Linux images are not supported.

No image is published yet. The GHCR reference in `crates/cli/src/target/mod.rs` is a versioned naming convention. The disabled `packages.yml` workflow publishes the Linux image to GHCR on a release tag; Windows and Android images remain unpublishable, so the disabled workflows stay disabled until publishing is authorized.

Every image runs `aurora-container-build.sh`, which starts the normal editor export with the project read-only at `/project` and a staging directory at `/out`. The host CLI verifies and publishes the result.
