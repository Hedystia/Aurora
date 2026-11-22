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

## Next work

- Finish scene-script binding. Support multiple attached components, stable references across scene edits, and runtime prefab instantiation from TypeScript.
- Extend 2D rendering and authoring. Add native text rendering, sprite atlas and slicing controls, group editing, particles, and tile maps. Expand editor integration tests around reference drops and scene construction.
- Add authored 3D scenes. Introduce spatial node transforms, camera components, glTF mesh import, lighting and materials, and 3D editor tools. Extend physics with additional collider shapes and script-visible 3D contacts.
- Verify release packaging and hosted CI. Exercise clean installs on X11 and Wayland, live audio output, and alternate GPU drivers.
