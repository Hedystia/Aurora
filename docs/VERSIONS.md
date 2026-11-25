# Version pins

Pins are declared in the workspace [Cargo.toml](../Cargo.toml), [scripts/package.json](../scripts/package.json), [rust-toolchain.toml](../rust-toolchain.toml), and [tools/setup.sh](../tools/setup.sh). Cargo.lock records the M0 dependency graph.

## 2026-11-22 · Initial setup

| Component | Pin | Reason |
| --- | --- | --- |
| Rust | 1.99.1 | Installed stable patch, meets the requested minimum |
| egui, egui-wgpu, egui-winit, eframe | 0.35.0 | Retain the requested wgpu 29 compatibility |
| wgpu | 29.0.4 | Patch update from 29.0.0 |
| egui_dock | 0.20.1 | Compatible with egui 0.35 |
| winit | 0.30.13 | Patch update from 0.30.5 |
| glyphon | 0.11.0 | Compatible with wgpu 29 |
| Rapier 2D and 3D | 0.35.3 | Patch update from 0.35.2 |
| oxijolt | 0.6.0+jolt-5.6.0 | Retain requested line. Cargo requirements omit build metadata. |
| kira, ab_glyph, image, symphonia | 0.12.6, 0.2.32, 0.25.10, 0.5.5 | Patch updates on the requested lines |
| serde, serde_json, memmap2 | 1.0.228, 1.0.150, 0.9.10 | Patch updates on the requested lines |
| anyhow, thiserror, tracing, tracing-subscriber | 1.0.104, 2.0.18, 0.1.44, 0.3.23 | Patch updates on the requested lines |
| clap | 4.5.60 | Patch update from 4.5.41 |
| pollster | 0.4.0 | Runs wgpu initialization on the native event-loop thread |
| TypeScript, esbuild, Vitest | 5.9.3, 0.25.12, 3.2.7 | Retain TypeScript line and take esbuild/Vitest patches |
| scriptc | 0.4.16 | Pinned standalone Linux and macOS releases |
| Portable Bun | 1.2.22 | Pin a published Linux release with checksums for x86-64 baseline and ARM64 |

## M1 runtime dependencies

| Component | Pin | Reason |
| --- | --- | --- |
| hecs | 0.11.2 | Activates the existing workspace pin for scene storage |
| bytemuck | 1.25.2 | Uses the existing transitive version to encode GPU vertices |
| sha2 | 0.10.9 | SHA-256 identities and integrity checks for imported textures |
| tempfile | 3.27.0 | Atomic cache publication using staging files in the destination directory |

Serde activates at `1.0.229`, matching the patch already resolved transitively in the M0 lockfile.

## M2 scripting dependencies

Bincode `2.0.1` and nix `0.30.1` activate their existing pins. Memmap2 activates at `0.9.12`, matching the patch previously resolved transitively. The native script uses the existing scriptc `0.4.16` pin.

## M3 physics and audio

Rapier 2D activates at `0.35.3` with `enhanced-determinism`. This release does not publish the requested `simd-stable` feature. The same feature policy is set for the future Rapier 3D dependency. Kira `0.12.6` enables CPAL, WAV, PCM, and MP3 and requires Symphonia 0.6. The lockfile resolves Symphonia `0.6.1`; the workspace pin follows that line to keep one decoder stack.

## M5 editor dependencies

Notify activates the existing `8.1.0` workspace pin. Egui Dock stays at `0.20.1` and enables its Serde feature for saved layouts. The editor links the existing engine core and shared metadata parser.

## M6 golden snapshot

Insta activates the existing `1.43.2` workspace pin. The golden runner uses existing SHA-256 and scriptc versions.

## M7 3D dependencies

Rapier 3D activates at the workspace pin `0.35.3` with enhanced determinism. The optional Jolt backend resolves `oxijolt 0.6.0+jolt-5.6.0`. Glam `0.33.12` matches the version already used by Rapier's math API.


## 2026-11-22 · TypeScript tooling update

| Component | Pin | Reason |
| --- | --- | --- |
| TypeScript | 7.0.2 | Update from 5.9.3 to the stable npm release for workspace and project type checking. |
| Node declarations | 20.19.43 | Type-check the socket launcher against Node 20, the development fallback’s minimum. |

The npm TypeScript pin applies to the standalone checker. The pinned scriptc distribution supplies its own compiler for native game builds. Bun and Node remain the development launchers.

## 2026-11-22 · Scripting authoring

Tree-sitter `0.27.2` and its TypeScript grammar `0.23.2` are pinned for syntax-based editor metadata extraction and native decorator lowering.

## 2026-11-23 · Script protocol 10

Protocol 10 adds key release, identity-tracked pointer phases, node-targeted pointer delivery, and a scene-start reset. Existing native scripts must be rebuilt because the player rejects older protocol versions. The development launcher and the SDK declarations are regenerated from the same protocol definitions.

## 2026-11-23 · Script protocol 11

Protocol 11 adds global and hit-tested mouse wheel events, emits mouse-specific event names alongside the pointer phases, and registers component classes so `addComponent` accepts constructors. Existing native scripts must be rebuilt. `aurora-codegen` and the SDK declarations are regenerated from the same protocol definitions.

## 2026-11-23 · Script protocol 12

Protocol 12 streams local node transforms to the script and adds a per-tick tween phase. `NodeRef` now reads cached position, scale and angle; `tween` interpolates node positions on the fixed tick. Node-targeted pointer transactions now carry node-local pixels, which the SDK exposes as `event.nodeX`/`event.nodeY` while the normalized frame coordinates stay on `event.x`/`event.y`. Existing native scripts must be rebuilt.

## 2026-11-23 · Script protocol 13

Protocol 13 adds queued ScaleNode and RotateNode edits, so scripts and tweens can change local scale and rotation alongside position. `NodeRef` and `Node` gain `setScale`, `setAngle`, and writable `scaleX`, `scaleY`, and `angle` (degrees), and `tween` drives all three channels. The demo launcher now advances tweens and drains queued edits. `@property` accepts component classes and the editor serializes component references. Existing native scripts must be rebuilt.

## 2026-11-23 · Script protocol 14

Protocol 14 adds bounded, chunked `SetText` edits for replacing the content of authored Text nodes at runtime. `NodeRef.setText` and `Node.setText` expose the API; replacements are UTF-8 validated and limited to 1512 bytes. The Inspector metadata also adds integer, multiline text, Vector2, color, enum, read-only, and image properties. Existing native scripts must be rebuilt.

## Authored static mesh import

The editor uses gltf 1.4.1 for validated glTF 2.0 accessors, base64 0.22.1 for embedded buffers and the existing glam pin for coordinate conversion. Runtime mesh rendering uses the existing wgpu pipeline. Serde's rc feature shares immutable mesh geometry across scene snapshots and prefab instances.

## Authored text rasterization

The runtime activates the workspace pin for `ab_glyph 0.2.32` and the locked `epaint_default_fonts 0.35.0` font-data crate. Ubuntu Light provides a portable default when the scene has no project font. Custom TrueType and OpenType fonts use the same rasterizer.

## Export package manifest

The editor emits `export.json` version 1 for single- and multi-scene exports. It records the engine version, exporting host target, each package file's SHA-256, and a deterministic digest of the ordered file records. The player verifies packages with this manifest and continues to load legacy bundles without it. Local exports also include the matching host player and a launcher.

## 2026-11-25 · Local container build CLI

| Component | Pin | Reason |
| --- | --- | --- |
| aurora-cli | workspace version | New `aurora` binary for local Docker/Podman game builds; uses existing `clap`, `anyhow`, `serde`, `serde_json`, `sha2`, `tempfile`, and `engine_core` pins. |
| Linux builder base image | `rust:1.99.1-bookworm` | Matches the Rust toolchain pin; base image digest pinning is required before publishing. |
| Builder toolchain reports | scriptc 0.4.16, Bun 1.2.22 | Reuse the workspace pins and expose them as `org.aurora.toolchain.*` image labels recorded in `build.json`. |

Builder image references are `ghcr.io/hedystia/aurora-builder:<engine-version>-<target>`. No builder image is published yet, and the disabled GitHub Actions workflows remain disabled. The Linux engine package now includes the `aurora` command alongside the player, editor, and codegen.

## 2026-11-25 · macOS host application bundles

`aurora build --target macos` packages a project into a `.app` on an Apple-branded host, with no container engine. The bundle contains `Contents/MacOS/aurora-player`, `Contents/Resources/game`, `Info.plist`, and `PkgInfo`; `build.json` records `builder_kind: "host"` and omits image fields. Optional `--sign` and `--notarize` use the host's `codesign`, `notarytool`, and `stapler`. The player discovers the adjacent bundle when launched without an explicit mode. Apple's SLA permits macOS virtualization only on Apple hardware, so no `dockurr/macos`-style builder image is added. `tools/test-macos-app.sh` and `tools/package-macos.sh` cover host smoke tests and per-architecture engine kits; the disabled package CI runs them on aarch64 and x86_64.

## 2026-11-25 · Unix shared-memory script transport

Script IPC now compiles on Unix hosts, not only Linux. The player keeps the same frame-locked protocol and fd-3 descriptor passing: Linux allocates a sealed `memfd`, while macOS creates an unlinked POSIX shared-memory object with `shm_open`, sets its size, maps it, and marks the descriptor close-on-exec. The child-side C ring is unchanged. Session supervision uses the portable Unix process-group and descriptor APIs, and `tools/setup.sh` resolves Bun plus the matching scriptc release for Linux and macOS on x64 and arm64. `aurora build --target macos` therefore packages native scripts instead of refusing them. Windows remains unsupported because it has no Unix descriptor passing.

Verification: `script_api` tests pass on Linux, `script_api`, `engine_core`, and `aurora-cli` pass `cargo check --target aarch64-apple-darwin`, and the macOS native/development transport comparison is defined in the disabled package CI. It has not run on an Apple host here.

## 2026-11-25 · Package CI and release publishing

Adds a disabled `.github/workflows/packages.yml.disabled` and removes the separate macOS workflow. Pull requests, `main` pushes, and manual dispatches build and test release artifacts: Linux formatting/clippy/tests, full kit, and Docker/Podman builder; macOS formatting/clippy/tests, app, native/development transport, and kit on aarch64 and x86-64; Windows release build and executable smoke test. A `v*` tag creates the release, uploads engine archives with `taiki-e/upload-rust-binary-action`, host kits, smoke-tests the packaged Windows archive, records Android as blocked, and pushes the Linux builder image to GHCR. Existing workflows remain disabled.

## 2026-11-25 · Perspective Scene camera

The editor Scene view now projects spatial nodes through an orbitable perspective camera instead of the fixed isometric projection. Middle-drag orbits the camera, **Reset view** restores the default angle, and projected picking, translation on the node's XY plane, and gizmo handles use the same camera. Authored 3D data and the player's runtime camera are unchanged.

Verification: `cargo fmt --all --check`, `cargo clippy --locked -p editor --all-targets -- -D warnings`, and all 41 editor tests pass, including perspective target projection, orbit pitch clamping, screen-drag round trips, and gizmo picking.

## 2026-11-25 · Scripting asset manager and export verification

The TypeScript SDK adds a global `assetManager`, cached bundle views, project-wide prefab enumeration, and `AssetBundle.preloadAll`/`loadAll` callbacks with per-asset progress and errors. Bundle release drops its script handles, while the player continues to own scene-registered audio and sprite-frame data. The generated script catalog now registers every prefab exported with the project.

`tools/test-export.sh` builds the editor and player, exports Jumpy Bird with its native scriptc game, validates `export.json` and the launcher, and runs the bundle headless.

Verification: Bun typecheck and 25 script tests pass; the export smoke test passes on Linux.

## 2026-11-25 · 3D animation and skin baking

The animation model adds nine spatial scalar channels (XYZ translation in metres, XYZ Euler rotation in degrees and positive XYZ scale) applied to `SpatialNode` before physics. `PrefabFile` gains an optional `animations` list; prefab clips and `capture_with` helpers round-trip through save/load, and instance tracks are rebased into the scene's flat node order. The glTF importer converts step and linear samplers to 60 Hz clips, decomposes rotation quaternions into lossy Euler keys, rejects cubic-spline and morph-target channels, and bakes skin weights at the rest pose using inverse bind matrices and joint world transforms. No new dependency pins are required: glam, gltf and serde are already in the workspace.

Verification: `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo test --locked --workspace`, `cargo run --locked --bin aurora-player -- --3d --headless --frames 240`, `tools/test-export.sh` and `tools/golden_test.sh` pass; the Jumpy Bird digest remains `64e4400479526792aa932d5df8a8a49a83f4f69e716d62a5b960091c6e04b165`. No shader was changed, so no WGSL validation was needed. No graphical or visual verification was performed for the new animation and skin paths.
