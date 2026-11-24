//! Runtime window and event loop.

/// Portable native scene bundle format.
pub mod bundle;
mod clock;
mod demo;
/// Offscreen player frames for the editor preview.
pub mod preview;
mod runtime;
mod script_lifecycle;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

use crate::{
    input::{InputEvent, PointerPhase},
    render::Renderer,
    scripting::Script,
};
use clock::Clock;
use runtime::Runtime;

/// Native program and authored binding order belonging to one packaged scene.
#[derive(Clone)]
pub struct SceneProgram {
    /// Native executable, or no program for a scene without script bindings.
    pub executable: Option<PathBuf>,
    /// Authored node indices in native program binding order.
    pub bindings: Vec<usize>,
}

/// Scenario selection and synthetic input for deterministic runs.
#[derive(Default)]
pub struct Scenario<'a> {
    /// Load the bird and pipes scene.
    pub bird: bool,
    /// Run the 3D drop test with the selected solver.
    pub spatial: Option<crate::physics3d::Backend>,
    /// Registered authored scene files. The first is active on startup.
    pub scenes: &'a [PathBuf],
    /// Authored node indices in script binding order. An empty slice defaults to node zero.
    pub script_nodes: &'a [usize],
    /// Per-scene native programs, in the same order as `scenes`. Empty uses the shared script.
    pub scene_programs: &'a [SceneProgram],
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
        if screenshot.is_some() || !scenario.scene_programs.is_empty() {
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
        cursor: None,
        mouse_down: false,
    };
    EventLoop::new()?.run_app(&mut app)?;
    app.result?;
    app.demo.stop_script()?;
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
    struct Snapshot<'a> {
        ticks: u64,
        nodes: Vec<crate::scene::NodeState>,
        #[serde(skip_serializing_if = "Option::is_none")]
        bird: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        spatial: Option<SpatialSnapshot<'a>>,
    }
    #[derive(serde::Serialize)]
    struct SpatialSnapshot<'a> {
        backend: crate::physics3d::Backend,
        cubes: &'a [crate::render::Cube],
    }
    let mut demo = Runtime::new(sprite, cache, &scenario, true)?;
    demo.attach_script(script)?;
    advance(&mut demo, ticks, scenario.input)?;
    let snapshot = serde_json::to_string(&Snapshot {
        ticks: demo.ticks(),
        nodes: demo.node_states()?,
        bird: demo.bird_stats()?,
        spatial: demo
            .spatial_state()
            .map(|(backend, cubes)| SpatialSnapshot { backend, cubes }),
    })?;
    demo.stop_script()?;
    Ok(snapshot)
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
    /// Last cursor position inside the window in normalized coordinates.
    cursor: Option<[f32; 2]>,
    /// Whether the primary mouse button is currently held.
    mouse_down: bool,
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
                if self.demo.spatial_state().is_some() {
                    self.result = Err(error);
                    event_loop.exit();
                    return;
                }
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
        let viewport = [window.inner_size().width, window.inner_size().height];
        // The window backend reports mouse input separately from touch. Mouse
        // contacts use the same pointer events as a single finger.
        let mut pushed = false;
        match &event {
            WindowEvent::CursorMoved { position, .. } if viewport[0] > 0 && viewport[1] > 0 => {
                let normalized = [
                    (position.x / f64::from(viewport[0])) as f32,
                    (position.y / f64::from(viewport[1])) as f32,
                ];
                if (0.0..=1.0).contains(&normalized[0]) && (0.0..=1.0).contains(&normalized[1]) {
                    self.cursor = Some(normalized);
                    if self.mouse_down
                        && self.input.len() < 64
                        && let Some(event) =
                            InputEvent::mouse(PointerPhase::Move, normalized, viewport)
                    {
                        self.input.push(event);
                        pushed = true;
                    }
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } if !self.mouse_down => {
                self.mouse_down = true;
                if let Some(position) = self.cursor
                    && self.input.len() < 64
                    && let Some(event) = InputEvent::mouse(PointerPhase::Start, position, viewport)
                {
                    self.input.push(event);
                    pushed = true;
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } if self.mouse_down => {
                self.mouse_down = false;
                if let Some(position) = self.cursor
                    && self.input.len() < 64
                    && let Some(event) = InputEvent::mouse(PointerPhase::End, position, viewport)
                {
                    self.input.push(event);
                    pushed = true;
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let Some(position) = self.cursor {
                    let delta = match delta {
                        MouseScrollDelta::LineDelta(x, y) => [*x, *y],
                        MouseScrollDelta::PixelDelta(position) => {
                            [position.x as f32, position.y as f32]
                        }
                    };
                    if self.input.len() < 64 {
                        self.input.push(InputEvent::Wheel {
                            position,
                            delta,
                            viewport,
                        });
                        pushed = true;
                    }
                }
            }
            WindowEvent::Focused(false) => {
                // Losing focus can interrupt the mouse and any number of fingers.
                self.mouse_down = false;
                if self.input.len() < 64 {
                    self.input.push(InputEvent::CancelPointers);
                }
            }
            _ => {}
        }
        if !pushed
            && self.input.len() < 64
            && let Some(event) = InputEvent::from_window_in_view(&event, viewport)
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
            WindowEvent::RedrawRequested => match self
                .demo
                .frame()
                .and_then(|frame| renderer.draw(window, self.screenshot.as_deref(), frame))
            {
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
