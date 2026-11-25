# Aurora roadmap

## M7 3D groundwork

The 3D drop test runs three dynamic cuboids against a fixed platform. The window, screenshot, and headless paths share the same 60 Hz simulation. The GPU path renders the physics poses through a perspective camera, directional lighting, instancing, and a depth buffer. Rapier is the default. A feature-enabled build can also select Jolt.

```bash
cargo run --locked --bin aurora-player -- --3d
cargo run --locked --bin aurora-player -- --3d --headless --frames 240
cargo run --locked --bin aurora-player -- --3d --capture-after 240 --screenshot /tmp/aurora-3d.png
cargo run --locked --bin aurora-player --features physics-jolt -- --3d --physics-backend jolt
```

Building the optional Jolt backend requires CMake and a C++ toolchain. Each backend owns its solver state. Tests compare settled poses with tolerances across solvers and repeated results within a solver when required. The Jumpy Bird golden continues to cover the 2D runtime.

## Implemented authored workflows

- **Authored 2D physics.** Scene and prefab nodes can own Fixed, Kinematic, or Dynamic Rapier bodies with solid or trigger box, circle, and polygon colliders. Fixed-tick contacts use enter/exit handling and Inspector controls; existing overlap sensors remain supported. Scripted bodies can set linear velocity.
- **2D visuals and editor.** Runtime Text/Rich Text and custom font rasterization, particles, tile maps, atlas sprites, and nine-slice panels are editable in the Inspector and rendered in Scene and Play views. Grouping preserves sibling poses. Timeline curves support key dragging, Step/Linear/Cubic interpolation, and cubic tangent editing.
- **Mixed 2D/3D projects.** The project manager offers 2D, 3D, mixed 2D/3D, and Jumpy Bird starters. Scene assets record a selectable 2D, 3D, or mixed authoring mode; all modes can mix pixel-space and spatial nodes. Authored spatial transforms, cameras, directional lights, cube materials, cuboid physics, and glTF mesh plus base-color material import are wired through the editor and player.
- **Jumpy Bird.** The authored scene and TypeScript sample are editable in the native editor. The fixed 600-tick replay remains byte-stable against its committed snapshot. [Build Jumpy Bird in the editor](JUMPY_TUTORIAL.md) walks through creating the scene tree, pillar prefab, frame and sound banks, wing animation, component binding, and Play verification from a blank 2D project.
- **Scripting and export.** Runtime component addition and removal, pointer hit testing, richer input packets, project-wide prefab catalogs, a global `assetManager`, bundle-wide preload/load progress, cancellation, and isolated per-component handles are implemented. The development and native backends share the same TypeScript API, and `tools/test-export.sh` exports a native-script project, verifies its manifest and launcher, and runs the bundled game headless.

## Remaining work

- **3D authoring depth.** The Scene view uses an orbitable perspective camera and supports projected selection, X/Y translation, local-Z rotation, and uniform scale tools. glTF import preserves UVs and writes base-color textures beside the prefab, and the mesh renderer shades metallic/roughness materials with those textures. Animated or skinned glTF and script-visible 3D physics remain future work. Authored 3D physics currently supports cuboids.
- **On-demand asset import.** Audio and sprite-frame loading still resolves scene-registered banks, and the player owns their decoded data. Arbitrary disk-backed runtime import with reference-counted player memory release remains future work.
- **Distribution and platform verification.** Engine releases, builder images, and game packages are separate products. The tagged release workflow builds desktop engine archives and checksums. Editor export creates a locally runnable host-OS folder containing a matching player, launcher, and integrity manifest. `aurora build` adds local Docker/Podman builds from a versioned builder image: Linux is implemented end to end, the image is ephemeral by default, and the package records its engine, target, image ID, and toolchain. Downloadable target templates and hosted project builds are not implemented; image publishing is limited to the disabled package CI. Scripted builds run on Linux and macOS through the Unix transport; Windows has cross-toolchain groundwork but package export is blocked by Unix-only script IPC and host-target stamping. Android has SDK/NDK image groundwork but no application runtime. macOS is host-only: `aurora build --target macos` exports and packages a `.app` on an Apple-branded host, including native scripts through the Unix transport, with optional signing/notarization; there is no macOS container image because Apple's SLA permits virtualization only on Apple hardware. A disabled package CI covers aarch64 and x86_64 macOS app/engine artifacts, Linux and Windows release builds, and on a version tag publishes per-platform engine archives, host kits, and the Linux builder image. See [the build package model and platform matrix](BUILD_PACKAGES.md). Local Linux package, X11/Wayland Vulkan, editor Play, PipeWire output, Jolt, and Rust checks passed; the container smoke test and native Windows/macOS, Android devices, Web, and alternate GPU drivers remain unverified.
