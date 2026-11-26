//! Aurora's Android application shell.
//!
//! This is unverified groundwork: no Android SDK, NDK, device or emulator was
//! available when it was written. The shell owns a `NativeActivity` winit loop
//! and deliberately starts no script child, because a normal Android app
//! cannot spawn Aurora's Unix shared-memory script transport.

#[cfg(target_os = "android")]
mod shell;

#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;

// Keep the Android-only dependency set declared when the cdylib is checked on
// a non-Android host, where the shell module is intentionally absent.
#[cfg(not(target_os = "android"))]
use {anyhow as _, engine_core as _, pollster as _, wgpu as _, winit as _};

/// Native `NativeActivity` entry point called by the `android-activity` glue.
///
/// The event loop receives the `AndroidApp` directly as required by winit
/// 0.30; creating it with `EventLoop::new()` would panic on Android. All
/// simulation runs in this process with no script bindings.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub fn android_main(app: AndroidApp) {
    shell::run(app);
}
