//! Native Aurora editor shell.

mod docks;
mod strings;
mod theme;

use std::path::PathBuf;

use clap::Parser;
use egui::{Event, Ui, UserData, ViewportBuilder, ViewportCommand};
use egui_dock::{DockArea, DockState, Style};

#[derive(Parser)]
#[command(version, about = strings::TITLE)]
struct Options {
    /// Capture the startup layout to a PNG and exit.
    #[arg(long)]
    screenshot: Option<PathBuf>,
}

struct Editor {
    layout: DockState<docks::Dock>,
    screenshot: Option<PathBuf>,
    requested_size: Option<[usize; 2]>,
    capture_result: std::rc::Rc<std::cell::Cell<Option<image::ImageResult<()>>>>,
}

impl eframe::App for Editor {
    fn ui(&mut self, ui: &mut Ui, _: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let style = Style::from_egui(ui.style().as_ref());
        DockArea::new(&mut self.layout)
            .style(style)
            .show_leaf_close_all_buttons(false)
            .show_inside(ui, &mut docks::Viewer);

        let Some(path) = &self.screenshot else {
            return;
        };
        let pixels = ctx.viewport_rect().size() * ctx.pixels_per_point();
        let size = [pixels.x.round() as usize, pixels.y.round() as usize];
        let frame = ui.input(|input| {
            input.events.iter().find_map(|event| match event {
                Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(frame) = &frame
            && self.requested_size == Some(size)
            && frame
                .size
                .iter()
                .zip(size)
                .all(|(actual, expected)| actual.abs_diff(expected) <= 1)
        {
            let [width, height] = frame.size;
            let pixels: Vec<_> = frame
                .pixels
                .iter()
                .flat_map(|color| color.to_srgba_unmultiplied())
                .collect();
            self.capture_result.set(Some(image::save_buffer_with_format(
                path,
                &pixels,
                width as u32,
                height as u32,
                image::ColorType::Rgba8,
                image::ImageFormat::Png,
            )));
            self.screenshot = None;
            ctx.send_viewport_cmd(ViewportCommand::Close);
        } else if frame.is_some() || self.requested_size.is_none() {
            // A compositor resize may change the swapchain while a capture is in flight.
            self.requested_size = Some(size);
            ctx.send_viewport_cmd(ViewportCommand::Screenshot(UserData::default()));
        }
    }
}

fn main() -> anyhow::Result<()> {
    let options = Options::parse();
    let capture_requested = options.screenshot.is_some();
    let capture_result = std::rc::Rc::new(std::cell::Cell::new(None));
    let result = capture_result.clone();
    eframe::run_native(
        strings::TITLE,
        eframe::NativeOptions {
            viewport: ViewportBuilder::default().with_inner_size(theme::WINDOW_SIZE),
            ..Default::default()
        },
        Box::new(move |context| {
            theme::apply(&context.egui_ctx);
            Ok(Box::new(Editor {
                layout: docks::initial_layout(),
                screenshot: options.screenshot,
                requested_size: None,
                capture_result: result,
            }))
        }),
    )
    .map_err(|error| anyhow::anyhow!("{error}"))?;
    if capture_requested {
        capture_result
            .take()
            .ok_or_else(|| anyhow::anyhow!("Editor closed before screenshot capture"))??;
    }
    Ok(())
}
