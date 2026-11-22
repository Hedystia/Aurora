# Aurora Engine

Read the project skills in `.codex/skills/` for the subsystem being changed.

## Build and verification

- Run `tools/setup.sh` from a Linux host with Bash, curl, unzip, tar, sha256sum and a C linker. Use `tools/setup.sh --print-runtime` to resolve the development runtime and `tools/setup.sh --print-scriptc` to resolve the release compiler.
- Run `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings` and `cargo test --locked --workspace`.
- Run `cargo run --locked --bin aurora-player -- --screenshot /tmp/aurora-player.png` and `cargo run --locked --bin aurora-editor -- --screenshot /tmp/aurora-editor.png` from a graphical session. For X11 CI, use Xvfb and Mesa Vulkan.

- Run `cargo run --locked --bin aurora-player -- --headless --frames 600` to emit a deterministic scene snapshot with no display or GPU. Compare repeated output with `cmp` or inspect its SHA-256.
- Pass `--sprite path/to/image.png` to load a custom PNG in either player mode. Imported textures default to `.aurora/import`. Use `--asset-cache PATH` to redirect the cache.

- Build scripts with `tools/build-script.sh native` or `tools/build-script.sh dev`. Pass the printed executable to `aurora-player --script PATH` or `aurora-player --script-dev PATH` respectively. Add `--headless --frames 120` to inspect the resulting scene state.
- The script build emits inspector metadata and SDK declarations under `.aurora/scripts/metadata/<sample>`.

- Build the bird sample with `tools/build-script.sh native bird`. Run `cargo run --locked --bin aurora-player -- --bird --script .aurora/scripts/bird-native` and press Space or Up, or tap the window.
- For deterministic verification use `--bird --headless --frames 140 --flap-at 0 --touch-at 75`. The JSON includes flap, pipe-hit, contact-exit, and one-shot counts. For a graphical capture replace `--headless --frames 140` with `--capture-after 140 --screenshot /tmp/aurora-bird.png`.
- Linux build prerequisites include ALSA development headers for CPAL.

## Conventions

Use `src/mod.rs` for Cargo targets and subdirectory `mod.rs` files for modules. Document public Rust items. Keep the editor independent of the player's window and keep editor code out of engine_core. Record version changes in docs/VERSIONS.md and architecture decisions in docs/ADR.md.

## Milestone status

M1 adds the hecs scene tree, fixed-step player, headless scene snapshots, PNG import cache, camera and sprite renderer. M2 adds a frame-locked script child, native shared-memory IPC, a development socket backend and the TypeScript component SDK. M3 adds Rapier 2D, Kira one-shots, and fixed-tick keyboard/touch input. Prefabs, persistence, animation and director APIs start in M4. The Jumpy Bird golden baseline starts in M6.
