//! Runtime window and event loop.

mod clock;
mod demo;
/// Offscreen player frames for the editor preview.
pub mod preview;
mod runtime;

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

use crate::{input::InputEvent, render::Renderer, scripting::Script};
use clock::Clock;
use runtime::Runtime;

/// Scenario selection and synthetic input for deterministic runs.
#[derive(Default)]
pub struct Scenario<'a> {
    /// Load the bird and pipes scene.
    pub bird: bool,
    /// Registered authored scene files. The first is active on startup.
    pub scenes: &'a [PathBuf],
    /// Inline node index bound to the script in an authored scene.
    pub script_node: usize,
    /// Optional best-score file for an authored game.
    pub profile: Option<&'a Path>,
    /// Input paired with the zero-based tick at which it is delivered.
    pub input: &'a [(u64, InputEvent)],
}

/// Opens the player window and optionally saves a rendered frame after simulation.
pub fn run(
    sprite: Option<&Path>,
    cache: Option<PathBuf>,
    screenshot: Option<PathBuf>,
    script: Option<&Script>,
    scenario: Scenario<'_>,
    capture_after: u64,
) -> anyhow::Result<()> {
    let mut demo = Runtime::new(sprite, cache, &scenario, screenshot.is_some())?;
    if let Err(error) = demo.attach_script(script) {
        if screenshot.is_some() {
            return Err(error);
        }
        eprintln!("Game script failed to start: {error:#}");
        demo.disconnect_script();
    }
    if screenshot.is_some() {
        advance(&mut demo, capture_after, scenario.input)?;
    }
    let mut app = Player {
        display: None,
        screenshot,
        result: Ok(()),
        demo,
        clock: Clock::default(),
        last_update: None,
        input: Vec::new(),
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
    script: Option<&Script>,
    scenario: Scenario<'_>,
) -> anyhow::Result<String> {
    #[derive(serde::Serialize)]
    struct Snapshot {
        ticks: u64,
        nodes: Vec<crate::scene::NodeState>,
        #[serde(skip_serializing_if = "Option::is_none")]
        bird: Option<serde_json::Value>,
    }
    let mut demo = Runtime::new(sprite, cache, &scenario, true)?;
    demo.attach_script(script)?;
    advance(&mut demo, ticks, scenario.input)?;
    Ok(serde_json::to_string(&Snapshot {
        ticks: demo.ticks(),
        nodes: demo.scene().snapshot()?,
        bird: demo.bird_stats()?,
    })?)
}

fn advance(demo: &mut Runtime, ticks: u64, input: &[(u64, InputEvent)]) -> anyhow::Result<()> {
    let mut input = input.to_vec();
    input.sort_by_key(|&(tick, _)| tick);
    anyhow::ensure!(
        input.last().is_none_or(|&(tick, _)| tick < ticks),
        "Scheduled input is outside the simulation"
    );
    let mut input = input.into_iter().peekable();
    let mut pending = Vec::new();
    for tick in 0..ticks {
        pending.clear();
        while input.peek().is_some_and(|&(at, _)| at == tick) {
            if let Some((_, event)) = input.next() {
                pending.push(event);
            }
        }
        demo.tick(&pending)?;
    }
    Ok(())
}

struct Player {
    display: Option<(Arc<Window>, Renderer)>,
    screenshot: Option<PathBuf>,
    result: anyhow::Result<()>,
    demo: Runtime,
    clock: Clock,
    last_update: Option<Instant>,
    input: Vec<InputEvent>,
}

impl Player {
    fn open(&mut self, event_loop: &ActiveEventLoop) -> anyhow::Result<()> {
        let window = Arc::new(
            event_loop.create_window(
                Window::default_attributes()
                    .with_title(self.demo.title())
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
            let result = self.demo.tick(&self.input);
            self.input.clear();
            if let Err(error) = result {
                eprintln!("Game script stopped: {error:#}");
                self.demo.disconnect_script();
                break;
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
        if self.input.len() < 64
            && let Some(event) = InputEvent::from_window(&event)
        {
            self.input.push(event);
        }
        self.demo.controls(&event);
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                renderer.resize(size);
                window.request_redraw();
            }
            WindowEvent::Occluded(false) => window.request_redraw(),
            WindowEvent::RedrawRequested => match self.demo.scene().snapshot().and_then(|nodes| {
                renderer.draw(
                    window,
                    self.screenshot.as_deref(),
                    self.demo.assets(),
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
