//! Runtime window and event loop.

mod clock;
mod demo;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

use crate::render::Renderer;
use clock::Clock;
use demo::Demo;

/// Opens the player window and optionally saves its first presented frame.
pub fn run(
    sprite: Option<&Path>,
    cache: Option<PathBuf>,
    screenshot: Option<PathBuf>,
) -> anyhow::Result<()> {
    let mut app = Player {
        display: None,
        screenshot,
        result: Ok(()),
        demo: Demo::new(sprite, cache)?,
        clock: Clock::default(),
        last_update: None,
    };
    EventLoop::new()?.run_app(&mut app)?;
    app.result?;
    anyhow::ensure!(
        app.screenshot.is_none(),
        "Player closed before screenshot capture"
    );
    Ok(())
}

/// Runs the demo for exactly the requested number of ticks without creating a window or GPU.
pub fn run_headless(
    sprite: Option<&Path>,
    cache: Option<PathBuf>,
    ticks: u64,
) -> anyhow::Result<String> {
    #[derive(serde::Serialize)]
    struct Snapshot {
        ticks: u64,
        nodes: Vec<crate::scene::NodeState>,
    }
    let mut demo = Demo::new(sprite, cache)?;
    for _ in 0..ticks {
        demo.tick()?;
    }
    Ok(serde_json::to_string(&Snapshot {
        ticks: demo.ticks,
        nodes: demo.scene.snapshot()?,
    })?)
}

struct Player {
    display: Option<(Arc<Window>, Renderer)>,
    screenshot: Option<PathBuf>,
    result: anyhow::Result<()>,
    demo: Demo,
    clock: Clock,
    last_update: Option<Instant>,
}

impl Player {
    fn open(&mut self, event_loop: &ActiveEventLoop) -> anyhow::Result<()> {
        let window = Arc::new(
            event_loop.create_window(
                Window::default_attributes()
                    .with_title("Aurora Player")
                    .with_inner_size(LogicalSize::new(800, 480)),
            )?,
        );
        let renderer =
            pollster::block_on(Renderer::new(window.clone(), self.screenshot.is_some()))?;
        window.request_redraw();
        self.display = Some((window, renderer));
        self.last_update = Some(Instant::now());
        Ok(())
    }
}

impl ApplicationHandler for Player {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.display.is_none() {
            self.result = self.open(event_loop);
            if self.result.is_err() {
                event_loop.exit();
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some((window, _)) = &self.display else {
            return;
        };
        if self.screenshot.is_some() {
            return;
        }
        let now = Instant::now();
        let Some(previous) = self.last_update.replace(now) else {
            return;
        };
        let ticks = self.clock.advance(now - previous);
        for _ in 0..ticks {
            if let Err(error) = self.demo.tick() {
                self.result = Err(error);
                event_loop.exit();
                return;
            }
        }
        if ticks > 0 {
            window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(now + self.clock.until_tick()));
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some((window, renderer)) = &mut self.display else {
            return;
        };
        if window.id() != id {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                renderer.resize(size);
                window.request_redraw();
            }
            WindowEvent::Occluded(false) => window.request_redraw(),
            WindowEvent::RedrawRequested => match self.demo.scene.snapshot().and_then(|nodes| {
                renderer.draw(
                    window,
                    self.screenshot.as_deref(),
                    &self.demo.assets,
                    &nodes,
                    demo::CAMERA,
                )
            }) {
                Ok(true) if self.screenshot.is_some() => {
                    self.screenshot = None;
                    event_loop.exit();
                }

                Ok(_) => {}
                Err(error) => {
                    self.result = Err(error);
                    event_loop.exit();
                }
            },
            _ => {}
        }
    }
}
