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

## Remaining work

- **Scripting and export.** Extend runtime addition and removal of individual components, pointer hit testing and richer input packets, and general asset bundles and loading. Aurora supports scripted prefab clones, component lookup, custom node propagation, scene-registered resources, and multi-scene native bundles. Verify the expanded SDK and export paths across the development and scriptc backends.
- **Authored 2D gameplay.** Expose Rapier rigid bodies, box/circle/polygon colliders, and contact listeners on authored scenes. The player sample uses Rapier, while authored scenes currently use overlap sensors. Expand the input and component audio APIs beyond the current game and sample paths.
- **2D rendering and authoring.** Add runtime text and font import, Rich Text, particle systems, tile maps, and sprite atlas and nine-slice support. Finish the matching inspector and node library controls, group editing, and timeline curve editing. Add end-to-end editor tests for project setup, scene creation, reference drops, and Play/reload.
- **Jumpy Bird editor walkthrough.** Recreate the checked-in project through the editor and verify the full tutorial workflow. The existing editable scene and TypeScript demo already run and have a golden replay.
- **Authored 3D scenes.** Add spatial nodes and transforms, cameras, lights, glTF meshes and materials, 3D editing tools, and script-visible 3D physics.
- **Distribution and verification.** Verify hosted CI and packaged releases, exercise clean Linux installs on both X11 and Wayland with live audio and different GPU drivers, and verify the Windows and macOS settings paths. Add an explicit repository license before publishing.
