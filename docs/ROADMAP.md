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
- **Mixed 2D/3D projects.** The project manager offers 2D, 3D, mixed 2D/3D, and Jumpy Bird starters. Scene assets record a selectable 2D, 3D, or mixed authoring mode; all modes can mix pixel-space and spatial nodes. Authored spatial transforms, cameras, directional lights, cube materials, cuboid physics, and static glTF mesh import are wired through the editor and player.
- **Jumpy Bird.** The authored scene and TypeScript sample are editable in the native editor. The fixed 600-tick replay remains byte-stable against its committed snapshot.

## Remaining work

- **Scripting and export.** Extend runtime addition and removal of individual components, pointer hit testing and richer input packets, and general asset bundles and loading. Aurora supports scripted prefab clones, component lookup, custom node propagation, scene-registered resources, and multi-scene native bundles. Verify the expanded SDK and export paths across the development and scriptc backends.
- **3D authoring depth.** The Scene view uses an isometric projection and supports projected selection, X/Y translation, local-Z rotation, and uniform scale tools. A perspective editor camera, texture/PBR material import, animated or skinned glTF, and script-visible 3D physics remain future work. Authored 3D physics currently supports cuboids.
- **Jumpy Bird tutorial.** The sample's scene, prefab, script, and golden replay are present and the editor Play path is testable. A guided walkthrough that creates and assembles the project entirely through UI steps remains to be authored.
- **Distribution and platform verification.** Engine releases and game export packages are separate products. The tagged release workflow builds desktop engine archives and checksums. Editor export now creates a locally runnable host-OS folder containing a matching player, launcher, and integrity manifest; downloadable target templates and hosted project builds are not implemented. Scripted builds remain Linux-only. The target set is Linux, Windows, macOS, Android, and Web, with builds provisioned in CI so users do not need Android Studio/Xcode. Portable script runtime/IPC gates scripted exports outside Linux. See [the build package model and platform matrix](BUILD_PACKAGES.md). Local Linux package, X11/Wayland Vulkan, editor Play, PipeWire output, Jolt, and Rust checks passed; native Windows/macOS, Android devices, Web, and alternate GPU drivers remain unverified.
