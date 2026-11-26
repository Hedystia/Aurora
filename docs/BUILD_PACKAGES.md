# Engine releases, builder images, and game packages

Aurora has three different deliverables that must not be confused:

1. **Engine releases** are versioned editor/player/codegen applications for the host OS. The tag-triggered [release workflow](../.github/workflows/release.yml.disabled) packages Linux x86-64, Windows x86-64, and macOS, includes license notices, and publishes checksums. The separate [package CI](../.github/workflows/packages.yml.disabled) builds and tests release artifacts on pull requests and branch pushes, then publishes per-architecture engine archives, host kits, and the Linux builder image on a version tag. Both workflows are currently disabled by request.
2. **Builder images** are OCI images containing a build toolchain, the engine source for one version, and the pinned native script compiler and runtime. They build a project; they are not distributed to players and are not engine releases.
3. **Game packages** are the output for a player: the exported scene data, assets, native scripts, player binary, launcher, and integrity manifest. A Linux package is a runnable folder on the build host.

The hashes in a game package detect file changes relative to `export.json` but do not authenticate a publisher or protect a modified manifest. Engine version, target, builder identity, and toolchain versions are recorded in `build.json`.

## Local container builds

`aurora build` runs the existing editor export inside an immutable, engine-versioned builder image. It is implemented in `crates/cli`; the image definitions and entry script live in `build/images/`.

```bash
cargo run --locked --bin aurora -- targets
cargo run --locked --bin aurora -- build \
  --project path/to/project \
  --target linux \
  --output path/to/output
```

`.aurora` scripts and native compilation stay inside the image, so the host needs only a container engine, not Rust, Bun, scriptc, an Android SDK, or an IDE.

### Lifecycle

1. The CLI validates the target, project (a directory or one saved scene), output path, and included scenes.
2. It resolves Docker, then Podman (`--engine` or `AURORA_CONTAINER_ENGINE` override). Docker Desktop is never required; rootless Podman is supported.
3. It pulls the exact image reference for the engine version and target. If the registry is unreachable but the same reference already exists locally, the build continues with the local image and prints a warning. Otherwise the pull error is reported with a hint about the Docker daemon or Podman.
4. It runs one container with the project mounted read-only and a private temporary staging directory mounted read-write. The output is copied to the requested host directory only after the package verifies.
5. The container and image are ephemeral. The container uses `--rm`; the CLI removes the pulled image after success and after failure. `--keep-image` is the explicit opt-in to cache it. Cleanup failures are warnings, not silent passes.
6. Nonzero container exits propagate as a failed command with the exit code; container logs stream to the terminal.
7. `--dry-run` prints the resolved container command without pulling or running it.

### Isolation and paths

- `--network=none`: a game build never downloads dependencies. Everything needed is baked into the image.
- `--cap-drop=ALL`, `--security-opt=no-new-privileges`, and a non-root image user.
- The host Docker/Podman socket is never mounted into the builder, and only the project (read-only) and the staging directory (read-write) are exposed. Output metadata and unrelated host files are not mounted.
- Volumes are passed as separate argument vectors, so project and output paths with spaces work.
- Bind mounts are not relabeled, because relabeling would mutate the user's project. Podman runs this unprivileged, network-less container with `--security-opt=label=disable` instead. Docker uses the host's default label policy; a host that forbids unlabeled bind mounts must be configured by the user.
- The staged package is copied into the user-owned output directory, so output ownership does not depend on the container's uid mapping.
- Cancellation is delegated to the engine: `--rm` removes the container after a normal exit or forwarded interrupt. A killed CLI process cannot guarantee cleanup of a still-running container; `docker ps`/`podman ps` and the `aurora-build-<pid>-<n>` name identify it. Image cleanup runs whenever the CLI owns the flow and the build returns normally.

### Reproducibility and identity

A built package contains:

- `export.json`: engine version, target, sorted file SHA-256 records, and a bundle digest. The player verifies it before loading.
- `build.json` format 1: engine version, target and triple, enabled features, `builder_kind` (`container` or `host`), the builder image reference and resolved image ID for container builds, the bundle digest, and the `org.aurora.toolchain.*` labels reported by the image.
- `run-game.sh`/`run-game.cmd` and the target player.

`build.json` is written before the integrity manifest is regenerated, so it is part of the verified file set. Builds are reproducible in the sense that the same project, engine version, and image produce the same file contents; no wall-clock timestamp is recorded. Image references should be pinned by digest for release builds (`--image ghcr.io/...@sha256:...`).

### Builder images

| Target | Image definition | Status | Evidence and next step |
| --- | --- | --- | --- |
| Linux x86-64 | `build/images/linux/Containerfile` | Supported | Exports and packages the project, including native scripts. Verification: `tools/test-builder-images.sh` builds Jumpy Bird, runs it headless, and checks cleanup on success and failure. |
| Windows x86-64 | `build/images/windows/Containerfile` | Blocked | A named-pipe script transport, Windows native-script FFI manifest and target-aware manifest stamping are implemented. No Windows std, MinGW linker or Windows scriptc is available here, so no package has been built or run. A native Windows runner is required for validation. The image remains unpublishable groundwork and requires `--allow-blocked`. |
| Android ARM64 | `build/images/android/Containerfile` | Blocked | The image provisions the SDK, NDK and Rust target. A NativeActivity `cdylib` shell, manifest, Gradle files and an APK build script exist, but no SDK/NDK, emulator or device is available here and the shell is scriptless-first. Google SDK/NDK terms must be reviewed before publishing this image. |
| macOS ARM64/x86-64 | none | Host only | `aurora build --target macos` runs the editor export on the Apple host and packages a `.app`. There is no container image: Apple's SLA permits macOS virtualization only on Apple-branded hardware, so `dockurr/macos`-style images are not supported. Hosted macOS CI runners, Tart on Apple silicon, and EC2 Mac are valid hosts. |

The default reference is `ghcr.io/hedystia/aurora-builder:<engine-version>-<target>`. **No builder image has been published yet.** The GHCR path is a versioned naming convention until the disabled package CI is enabled for a release tag. The GitHub Actions files stay disabled; image definitions can be built and tested locally:

```bash
tools/build-builder-image.sh linux
tools/build-builder-image.sh windows --allow-blocked
tools/test-builder-images.sh
```

`tools/test-builder-images.sh` prints an explicit `SKIP` when neither engine is installed or the daemon is unreachable; it never reports untested images as passing.

### macOS host builds

macOS is a **host-only** target. `aurora targets` shows it as `host-only`; the CLI never tries to find Docker or Podman for it. From an Apple-branded Mac with `aurora` and `aurora-editor` built for the host architecture:

```bash
cargo run --locked --bin aurora -- build \
  --project scripts/game-template \
  --target macos \
  --output path/to/Game.app
```

Optional signing and notarization (Developer ID certificate and a `notarytool` keychain profile created outside the repository):

```bash
cargo run --locked --bin aurora -- build ... --sign "Developer ID Application: ..." --notarize aurora-notary
```

The CLI resolves the sibling `aurora-editor` (or `AURORA_EDITOR`), exports the project into a private staging directory, verifies the export manifest, and assembles:

```text
Game.app/Contents/MacOS/aurora-player
Game.app/Contents/Resources/game/...
Game.app/Contents/Info.plist
Game.app/Contents/PkgInfo
```

`Contents/Resources/game` contains `export.json` and the regenerated `build.json` with `builder_kind: "host"`; the image fields are absent. The player discovers this adjacent bundle when launched without an explicit mode. The bundle is host-architecture only: an Apple silicon host produces `macos-aarch64`, and an Intel host produces `macos-x86_64`. A universal binary needs a later lipo step.

**Scripted macOS exports use the Unix transport.** The player passes a shared-memory descriptor to the native script as fd 3 over a Unix socket. Linux allocates a sealed `memfd`; macOS allocates an unlinked POSIX shared-memory object with `shm_open`/`shm_unlink`, maps it, and sets close-on-exec before the launch. The native endpoint is the same portable C ring. `tools/setup.sh` provisions Bun and the matching `scriptc` release for Linux and macOS (x64 and arm64), and `tools/compile-native.sh` uses the host `clang`/`ar`. Windows remains unsupported because it has no Unix descriptor passing.

**Why not `dockurr/macos` or another macOS container image?** Those images require Linux with `/dev/kvm`. Apple's macOS software licence (SLA §2B(iii)) permits up to two additional macOS instances only in virtual environments on Apple-branded hardware already running macOS, for development/testing, and prohibits use on non-Apple hardware. Running the image on an ordinary Linux builder therefore breaches Apple's terms, independent of whether it boots. Aurora will not ship or recommend that path. Compliant options are the user's own Mac, a hosted macOS CI runner (for example GitHub Actions' macOS runners), [Tart](https://tart.run/) on Apple silicon hosts, or AWS EC2 Mac under Apple's CI lease terms. No automation for those services is included; they are execution environments for the same native command.

Ad-hoc signing (`--sign -`) is useful for local testing but is not trusted by Gatekeeper on other machines. Notarization requires a Developer ID signature and network access to Apple's notary service; the CLI runs `notarytool submit --wait` and `stapler staple`, and any failure aborts before publication.

`tools/test-macos-app.sh [OUTPUT.app] [debug|release]` builds a scriptless fixture, packages it with an ad-hoc signature, checks the plist, manifest, and bundle layout, and runs the app headless. `tools/package-macos.sh ARCHIVE.tar.gz [debug|release]` creates the per-architecture engine kit `aurora-macos-<arch>.tar.gz` containing the player, editor, codegen, CLI, and licence files. The package CI runs both on `macos-14` (aarch64) and `macos-15-intel` (x86_64), including the native/development transport comparison. Like the other workflows it stays disabled until Actions is authorized. There is no universal binary: each runner publishes its host architecture, matching the Windows and Linux per-architecture model.

### Package CI and publishing workflow

`.github/workflows/packages.yml.disabled` is the package CI and publishing path. It runs on pull requests, pushes to `main`, and manual dispatches to build and test release artifacts without publishing. A `v*` tag creates the release after the release-build tests pass, uploads packages, and publishes the supported builder image. It stays disabled with the other workflows until publishing is authorized. When enabled it:

1. Validates that every workspace package shares one version and, on a tag, that the tag matches it.
2. Runs formatting, clippy, and workspace tests on Linux and both macOS architectures; the Windows job builds the release workspace and smoke-tests the executable.
3. Builds the full Linux kit, runs it headless, and runs `tools/test-builder-images.sh`, so the Docker/Podman builder path is exercised. It publishes the kit on a tag.
4. Builds and smoke-tests the macOS `.app`, compares native and development script transports, builds the full macOS kit, and verifies the kit contents for both Apple architectures. It publishes the kit on a tag; the smoke app is test-only.
5. Publishes engine binaries with [`taiki-e/upload-rust-binary-action`](https://github.com/taiki-e/upload-rust-binary-action) for Linux x86-64, macOS aarch64/x86-64, and Windows x86-64 on a tag. The action builds the workspace, writes a leading-directory archive with the four binaries and `LICENSE`, and emits a SHA-256 checksum.
6. Smoke-tests the packaged Windows archive on a tag.
7. Records Android as blocked: no APK or Android image is uploaded.
8. Publishes the Linux builder image to GHCR on a release tag. GitHub-hosted runners use Docker; the local scripts also accept Podman through `AURORA_CONTAINER_ENGINE`. Windows and Android builder images are never published because they cannot produce a runnable package.

The Linux builder image reference is `ghcr.io/<owner>/aurora-builder:<version>-linux`. The CLI's default reference is `ghcr.io/hedystia/...`; forks that publish must update the default or pass `--image` explicitly.

### Registry and publishing policy

- Publish only immutable, engine-versioned tags, preferably with digests. Record the target triple, architecture, toolchain versions, and supported engine/script ABI as image labels.
- The disabled package CI is the only publish path, and it publishes the Linux image only on a release tag. Review and scan images before enabling it; do not activate the currently disabled workflows as part of a normal change.
- Verify pulled and published image identities. A build records the resolved image ID so an artifact can be traced to its toolchain.
- Do not put signing keys, tokens, or other secrets in an image, a log, the project archive, or committed source. Android signing keys, when Android packaging exists, must be supplied at build time from an external secret store or a user-provided file path, never baked into the image.

### Web, cloud, and hosted builds

A wasm-bindgen browser shell with a WebGPU canvas and `requestAnimationFrame` loop exists in `crates/web_player`, but the Web target remains blocked: no wasm toolchain, `wasm-bindgen` CLI or browser harness is available here, and the shell is scriptless-first with no audio path. Hosted/premium cloud builds are a later extension: no accounts, billing, project upload, or premium UI is implemented. Target metadata and local image execution are separated in the CLI, but there is no provider interface or upload protocol for a hosted backend yet. The current implementation is local only.

### Target templates

`tools/package-target-template.sh <target> <archive> [debug|release]` builds a bounded archive containing a target player, its launcher and a hashed `template.json`. `aurora build --template <PATH|URL>` verifies the engine version, target, triple, per-file SHA-256 values and bundle digest before caching the archive under `.aurora/templates/<version>-<target>/`. The editor's `--export-target` and `--player` flags stamp the requested target and substitute the template runtime; host-default export is unchanged. Template publication steps are present in the disabled package CI but no release has been published. Templates do not provide hosted builds or cross-compilation by themselves; a target still needs a runnable player for that platform.

### Engine release packaging

The editor still creates a locally runnable host folder with the matching player, launcher, and integrity manifest (`--export`). That path is independent of the container builder: it requires the host player binary and compiles scripts on the host. It remains useful for work without Docker or Podman, but it is not cross-compilation.

### Sokol evaluation

The official [Sokol project](https://github.com/floooh/sokol) is a small C graphics, audio, input, and app-layer library. [sokol-rs](https://github.com/code-disaster/sokol-rs) provides generated Rust bindings and compiles the Sokol C sources through a Cargo build script. It is a possible rendering/application backend, not a cross-platform package manager: building for Android or Apple still needs that platform's linker, SDK, and packaging/signing steps. Replacing Aurora's existing wgpu/winit renderer with Sokol would be a major renderer/editor integration and would not solve script IPC, Android's application shell, or Apple's licensing. Keep wgpu/winit and use builder images for the toolchains they can legally contain.

### Verification status

- The `aurora build` orchestration has 27 unit tests covering image pull, local-image fallback, scene-file projects, path handling with spaces, integrity verification, template verification/caching/rejection, failure propagation, blocked targets, cleanup on success and failure, and macOS bundle packaging, manifest regeneration, plist escaping, script-program rejection, and non-Apple-host refusal.
- `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, and `cargo test --locked --workspace` pass.
- `tools/golden_test.sh` passes with the committed Jumpy Bird hash `64e4400479526792aa932d5df8a8a49a83f4f69e716d62a5b960091c6e04b165`; the baseline was not changed.
- The container image definitions and the end-to-end smoke script have **not** been executed on this host because the Docker daemon socket is not accessible to the current user (`permission denied` on `/var/run/docker.sock`). This is a blocker for claiming a verified container build, not a passing check.
- Windows named-pipe IPC, the Android NativeActivity shell and the Web wasm shell were written and locally reviewed/type-checked where possible, but none was compiled to its native target or run: Windows std/MinGW/scriptc, Android SDK/NDK/device and the wasm toolchain/browser are absent. macOS `.app` packaging is unit-tested on Linux with a fake player and export; the complete native macOS build, `codesign`, and `notarize` paths were **not** executed because no Apple host is available here. `script_api`, `engine_core`, and `aurora-cli` pass `cargo check --target aarch64-apple-darwin` and `x86_64-apple-darwin`; the macOS `shm_open` path and native script launch have not run. `tools/test-macos-app.sh`, `tools/package-macos.sh`, and the package CI pass `bash -n`/YAML review only and have not run on an Apple runner or a tag. Windows, Android, and Web remain blocked as described above. Graphical X11/Wayland checks were not run in this session.
