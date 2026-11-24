# Engine releases and game build packages

Aurora distributes two different products:

1. **Engine releases** are versioned editor/player/codegen applications for the host OS. The tag-triggered [release workflow](../.github/workflows/release.yml) packages Linux x86-64, Windows x86-64, and macOS ARM64, includes license notices, and publishes checksums.
2. **Game build packages** (export templates) are target runtimes and build adapters that the editor downloads by engine version and target. A project can use them to create Linux, Windows, macOS, Android, or Web builds without installing the target IDE. Packages must be signed or checksum-verified, ABI/versioned, and installed side-by-side so old projects can keep their selected engine version.

This separation follows the useful part of [JasperEngine's release workflow](https://github.com/Arakuma-Studio/JasperEngine/blob/main/.github/workflows/build-release.yml): CI builds player archives per target and publishes them with the engine release. Aurora must add its own target package format and project builder; it does not reuse Jasper code.

These are not interchangeable. Installing the editor gives a user the tools to author a project; it does not install every target's SDK or mean that project export is supported for that target. Current editor exports include `export.json` with the engine version, exporting host target, sorted file hashes, and a bundle digest. They also include the matching `aurora-player` binary and a `run-game` launcher, so the folder is runnable on the editor's host OS; the player verifies file integrity before loading. This is a local host-runtime package, not yet a downloadable target template or cross-compilation package. The hashes detect file changes relative to the manifest but do not authenticate a publisher or protect a modified manifest. When scripts are present, export compiles a Linux-only script child, and script IPC is currently implemented only on Linux.

## Desired workflow

- The editor lists available engine versions and installed export packages independently.
- A user installs or selects an engine version, authors a project, then selects a target in Build Settings.
- The editor downloads the matching target package, verifies its manifest and SHA-256, and invokes its build adapter with the project bundle.
- Small packages that can build from a Rust target without a vendor IDE may support offline local export. Android and Apple platform jobs can provision vendor SDKs on CI and build remotely; users download the completed app without installing Android Studio or Xcode. A downloaded package must not pretend to contain a redistributable proprietary SDK.
- Every completed game artifact identifies its engine version, target triple/ABI, enabled features, and source bundle hash. The build remains reproducible from the archived package and the project.

## Platform status and blockers

| Target | Engine host release | Game build package | Main remaining requirement |
| --- | --- | --- | --- |
| Linux x86-64 | Packaged and locally smoke-tested | Locally runnable host package; no versioned template yet | Editor-managed runtime template, release download and hosted build workflow |
| Windows x86-64 | CI release job defined | Host package possible for scriptless projects; not validated | Portable script runtime/IPC and target runner validation |
| macOS ARM64 | CI release job defined | Host package possible for scriptless projects; not validated | Portable script runtime/IPC and signed/notarized app packaging |
| Android ARM64 | No player release | Not implemented | Android application runtime, portable scripts, CI-provisioned NDK build and device validation |
| Web | No engine release | Not implemented | wasm-compatible runtime dependencies, web shell, and script/input/audio support |

The scripting transport is a gating dependency for all platforms beyond Linux. Coordinate its cross-platform design with the scripting migration rather than adding platform-specific script files from the editor. A no-script renderer smoke app would not count as a usable game export package.

## Sokol evaluation

The official [Sokol project](https://github.com/floooh/sokol) is a small C graphics, audio, input, and app-layer library. [sokol-rs](https://github.com/code-disaster/sokol-rs) provides generated Rust bindings and compiles the Sokol C sources through a Cargo build script. It is a possible rendering/application backend, not a cross-platform package manager: building for Android or Apple still needs that platform's linker, SDK, and packaging/signing steps. Replacing Aurora's existing wgpu/winit renderer with Sokol would be a major renderer/editor integration and would not solve script IPC or project export.

Aurora already uses wgpu/winit, which provide cross-platform GPU and window abstractions. Keep them for now; use hosted CI to provision vendor tools where needed, and build versioned export packages around the existing runtime after script portability is resolved. Re-evaluate Sokol only if a measured renderer or platform gap remains.
