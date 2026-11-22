# Aurora Engine

Read the project skills in `.codex/skills/` for the subsystem being changed.

## Build and verification

- Run `tools/setup.sh` from a Linux host with Bash, curl, unzip, tar, sha256sum and a C linker. Use `tools/setup.sh --print-runtime` to resolve the development runtime and `tools/setup.sh --print-scriptc` to resolve the release compiler.
- Run `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings` and `cargo test --locked --workspace`.
- Run `cargo run --locked --bin aurora-player -- --screenshot /tmp/aurora-player.png` and `cargo run --locked --bin aurora-editor -- --screenshot /tmp/aurora-editor.png` from a graphical session. For X11 CI, use Xvfb and Mesa Vulkan.

## Conventions

Use `src/mod.rs` for Cargo targets and subdirectory `mod.rs` files for modules. Document public Rust items. Keep the editor independent of the player's window and keep editor code out of engine_core. Record version changes in docs/VERSIONS.md and architecture decisions in docs/ADR.md.

## Milestone status

M0 supplies a dockable editor shell and a GPU clear pass in a native player window. ECS, sprite rendering and fixed step start in M1. Protocol and TS SDK implementation start in M2. The 600-frame demo golden starts in M6.
