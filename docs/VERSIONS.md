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
| scriptc | 0.4.16 | Pinned standalone GNU/Linux release |
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
