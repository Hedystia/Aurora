//! NativeActivity event loop and minimal render surface for the Android shell.
//!
//! The engine's sprite renderer and long-lived `Runtime` are private to
//! `engine_core`, so this shell steps the public scriptless headless runtime
//! once per tick and presents a simple changing clear colour. That is enough
//! groundwork to prove the activity, surface, input and simulation all run in
//! process; replacing the clear pass with the engine renderer is a separate
//! engine-side change.

use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use engine_core::{
    app::{Scenario, run_headless},
    input::InputEvent,
};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    platform::android::{EventLoopBuilderExtAndroid, activity::AndroidApp},
    window::{Window, WindowId},
};

/// Fixed simulation step, matching the player's 60 Hz clock.
const FRAME: Duration = Duration::from_nanos(16_666_667);
/// Cap catch-up work after the activity has been suspended or stalled.
const MAX_STEPS_PER_FRAME: u64 = 4;
/// Emit one liveness line roughly every five seconds of simulation.
const LOG_EVERY_TICKS: u64 = 300;

/// Starts the winit loop with the activity handle supplied by the glue crate.
pub(super) fn run(app: AndroidApp) {
    let cache = app
        .internal_data_path()
        .map(|directory| directory.join("aurora-cache"));
    let event_loop = match EventLoop::builder().with_android_app(app).build() {
        Ok(event_loop) => event_loop,
        Err(error) => {
            eprintln!("Aurora Android: cannot create event loop: {error:#}");
            return;
        }
    };
    let mut shell = Shell {
        cache,
        gpu: None,
        last: None,
        accumulator: Duration::ZERO,
        tick: 0,
        input: Vec::new(),
    };
    if let Err(error) = event_loop.run_app(&mut shell) {
        eprintln!("Aurora Android: event loop failed: {error:#}");
    }
}

/// Owns the surface and drives one scriptless engine tick per frame step.
struct Shell {
    /// Persistent decoded-texture cache under the app's private data directory.
    cache: Option<PathBuf>,
    gpu: Option<Gpu>,
    last: Option<Instant>,
    accumulator: Duration,
    tick: u64,
    input: Vec<InputEvent>,
}

impl Shell {
    /// Creates the window and surface after the first Android resume event.
    fn open(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let window = Arc::new(
            event_loop.create_window(
                Window::default_attributes()
                    .with_title("Aurora")
                    .with_inner_size(winit::dpi::LogicalSize::new(800, 480)),
            )?,
        );
        let gpu = pollster::block_on(Gpu::new(Arc::clone(&window)))?;
        window.request_redraw();
        self.gpu = Some(gpu);
        self.last = Some(Instant::now());
        self.accumulator = Duration::ZERO;
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + FRAME));
        Ok(())
    }

    /// Advances the engine runtime with any input queued since the last step.
    fn step(&mut self) -> Result<()> {
        let scheduled = std::mem::take(&mut self.input);
        let ticks = scheduled.len().clamp(1, 64) as u64;
        let pending: Vec<(u64, InputEvent)> = scheduled
            .into_iter()
            .enumerate()
            .map(|(tick, event)| (tick as u64, event))
            .collect();
        let scenario = Scenario {
            input: &pending,
            ..Scenario::default()
        };
        let snapshot = run_headless(None, self.cache.clone(), ticks, None, scenario)?;
        let previous = self.tick / LOG_EVERY_TICKS;
        self.tick += ticks;
        if self.tick / LOG_EVERY_TICKS != previous {
            eprintln!(
                "Aurora Android: tick {} (scriptless snapshot {} bytes)",
                self.tick,
                snapshot.len()
            );
        }
        Ok(())
    }

    /// Records a fatal error and leaves the event loop without panicking.
    fn fail(&self, event_loop: &ActiveEventLoop, error: anyhow::Error) {
        eprintln!("Aurora Android: {error:#}");
        event_loop.exit();
    }
}

impl ApplicationHandler for Shell {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }
        if let Err(error) = self.open(event_loop) {
            self.fail(event_loop, error);
        }
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        // Android invalidates the native surface on suspend; it is recreated
        // in `resumed` after the next SurfaceView is ready.
        self.gpu = None;
        self.last = None;
        self.accumulator = Duration::ZERO;
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_none() {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }
        let now = Instant::now();
        let Some(previous) = self.last.replace(now) else {
            event_loop.set_control_flow(ControlFlow::WaitUntil(now + FRAME));
            return;
        };
        self.accumulator += now.saturating_duration_since(previous);
        let mut steps = 0;
        while self.accumulator >= FRAME && steps < MAX_STEPS_PER_FRAME {
            self.accumulator -= FRAME;
            steps += 1;
            if let Err(error) = self.step() {
                self.fail(event_loop, error);
                return;
            }
        }
        if steps > 0
            && let Some(gpu) = self.gpu.as_ref()
        {
            gpu.window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            now + FRAME.saturating_sub(self.accumulator),
        ));
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        if gpu.window.id() != id {
            return;
        }
        let window = Arc::clone(&gpu.window);
        let viewport = [window.inner_size().width, window.inner_size().height];
        if self.input.len() < 64
            && let Some(input) = InputEvent::from_window_in_view(&event, viewport)
        {
            self.input.push(input);
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.resize(size);
                }
                window.request_redraw();
            }
            WindowEvent::Occluded(false) => window.request_redraw(),
            WindowEvent::RedrawRequested => {
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.draw(self.tick);
                }
            }
            _ => {}
        }
    }
}

/// Just enough wgpu state to own the Android surface and present a frame.
struct Gpu {
    instance: wgpu::Instance,
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
}

impl Gpu {
    async fn new(window: Arc<Window>) -> Result<Self> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle().with_env());
        let surface = instance
            .create_surface(Arc::clone(&window))
            .context("Cannot create an Android wgpu surface")?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .context("No wgpu adapter compatible with the Android surface")?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await?;
        let size = window.inner_size();
        let config = surface
            .get_default_config(&adapter, size.width, size.height)
            .context("No compatible Android surface configuration")?;
        surface.configure(&device, &config);
        Ok(Self {
            instance,
            window,
            surface,
            device,
            queue,
            config,
        })
    }

    fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.device, &self.config);
    }

    fn draw(&mut self, tick: u64) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                self.window.request_redraw();
                return;
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.resize(self.window.inner_size());
                self.window.request_redraw();
                return;
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                // Android can replace the native window at any time.
                let surface = self.instance.create_surface(Arc::clone(&self.window));
                if let Ok(surface) = surface {
                    self.surface = surface;
                    self.resize(self.window.inner_size());
                }
                self.window.request_redraw();
                return;
            }
            _ => return,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let phase = (tick % 60) as f64 / 60.0;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Aurora Android clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.04 + phase * 0.1,
                        g: 0.09,
                        b: 0.16,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        drop(pass);
        self.queue.submit(Some(encoder.finish()));
        self.window.pre_present_notify();
        frame.present();
    }
}
