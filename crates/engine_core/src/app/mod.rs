//! Runtime window and event loop.

use std::{path::PathBuf, sync::Arc};

use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};

use crate::render::Renderer;

/// Opens the player window and optionally saves its first presented frame.
pub fn run(screenshot: Option<PathBuf>) -> anyhow::Result<()> {
    let mut app = Player {
        display: None,
        screenshot,
        result: Ok(()),
    };
    EventLoop::new()?.run_app(&mut app)?;
    app.result?;
    anyhow::ensure!(
        app.screenshot.is_none(),
        "Player closed before screenshot capture"
    );
    Ok(())
}

struct Player {
    display: Option<(Arc<Window>, Renderer)>,
    screenshot: Option<PathBuf>,
    result: anyhow::Result<()>,
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
            WindowEvent::RedrawRequested => match renderer.draw(window, self.screenshot.as_deref())
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
