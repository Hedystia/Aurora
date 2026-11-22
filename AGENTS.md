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

- Build the authored scene sample with `tools/build-script.sh native scene`. Run the player with `--scene scripts/game-template/scenes/gallery.aurora-scene --scene scripts/game-template/scenes/lookout.aurora-scene --script .aurora/scripts/scene-native`. Press **P** to pause or resume and **Tab** to switch scenes. Add `--headless --frames 120` to verify the scripted transition and pause.
- See [scene files and animation](docs/SCENES.md) for the persistence format and pool API.

- Run `target/debug/aurora-editor --project /path/to/project` after building both binaries. Use `--play --screenshot /tmp/aurora-preview.png` to verify the embedded player.

- Open `scripts/game-template/jumpy` in the editor to play Jumpy Bird. Run `tools/golden_test.sh` for the committed 600-frame check. See [the game and baseline workflow](docs/JUMPY_BIRD.md).

## Conventions

Use `src/mod.rs` for Cargo targets and subdirectory `mod.rs` files for modules. Document public Rust items. Keep the editor independent of the player's window and keep editor code out of engine_core. Record version changes in docs/VERSIONS.md and architecture decisions in docs/ADR.md.

## Milestone status

M1 adds the hecs scene tree, fixed-step player, headless scene snapshots, PNG import cache, camera and sprite renderer. M2 adds a frame-locked script child, native shared-memory IPC, a development socket backend and the TypeScript component SDK. M3 adds Rapier 2D, Kira one-shots, and fixed-tick keyboard/touch input. M4 adds authored scenes and prefabs, a scene-bound node pool, fixed-tick clip playback, and director controls. M5 adds the document editor, asset and script workflow, timeline, saved docks and child-player Game view. See docs/EDITOR.md for the supported editor workflow and remaining limits.

M6 adds the authored Jumpy Bird game and the committed 600-frame golden snapshot.
