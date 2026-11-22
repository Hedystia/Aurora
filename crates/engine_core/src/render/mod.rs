//! wgpu surface ownership and render pass for the player.

mod sprite;

use std::{path::Path, sync::Arc};

use anyhow::{Context, ensure};
use wgpu::*;
use winit::{dpi::PhysicalSize, window::Window};

use crate::{
    assets::Assets,
    scene::{Camera2D, NodeState},
};
use sprite::SpriteBatch;

pub(crate) struct Renderer {
    instance: Instance,
    surface: Surface<'static>,
    device: Device,
    queue: Queue,
    config: SurfaceConfiguration,
    size: PhysicalSize<u32>,
    sprites: SpriteBatch,
}

impl Renderer {
    pub(crate) async fn new(window: Arc<Window>, capture: bool) -> anyhow::Result<Self> {
        let instance = Instance::new(InstanceDescriptor::new_without_display_handle().with_env());
        let size = window.inner_size();
        let surface = instance.create_surface(window)?;
        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await?;
        let (device, queue) = adapter.request_device(&DeviceDescriptor::default()).await?;
        let mut config = surface
            .get_default_config(&adapter, size.width, size.height)
            .context("No compatible surface configuration")?;
        if capture {
            let caps = surface.get_capabilities(&adapter);
            ensure!(
                caps.usages.contains(TextureUsages::COPY_SRC),
                "Surface cannot be captured"
            );
            config.usage |= TextureUsages::COPY_SRC;
            config.format = caps
                .formats
                .into_iter()
                .find(|format| {
                    matches!(
                        format,
                        TextureFormat::Rgba8UnormSrgb
                            | TextureFormat::Bgra8UnormSrgb
                            | TextureFormat::Rgba8Unorm
                            | TextureFormat::Bgra8Unorm
                    )
                })
                .context("No supported screenshot pixel format")?;
        }
        let sprites = SpriteBatch::new(&device, config.format);
        let mut renderer = Self {
            instance,
            surface,
            device,
            queue,
            config,
            size,
            sprites,
        };
        renderer.resize(size);
        Ok(renderer)
    }

    pub(crate) fn resize(&mut self, size: PhysicalSize<u32>) {
        self.size = size;
        if size.width > 0 && size.height > 0 {
            self.config.width = size.width;
            self.config.height = size.height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    pub(crate) fn draw(
        &mut self,
        window: &Arc<Window>,
        screenshot: Option<&Path>,
        assets: &Assets,
        nodes: &[NodeState],
        camera: Camera2D,
    ) -> anyhow::Result<bool> {
        if self.size.width == 0 || self.size.height == 0 {
            return Ok(false);
        }
        let frame = match self.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(frame) | CurrentSurfaceTexture::Suboptimal(frame) => {
                frame
            }
            CurrentSurfaceTexture::Occluded => return Ok(false),
            CurrentSurfaceTexture::Timeout => {
                window.request_redraw();
                return Ok(false);
            }
            CurrentSurfaceTexture::Lost => {
                self.surface = self.instance.create_surface(window.clone())?;
                self.resize(window.inner_size());
                window.request_redraw();
                return Ok(false);
            }
            CurrentSurfaceTexture::Outdated => {
                self.resize(window.inner_size());
                window.request_redraw();
                return Ok(false);
            }
            CurrentSurfaceTexture::Validation => anyhow::bail!("Surface texture validation failed"),
        };
        let extent = frame.texture.size();
        let view = frame.texture.create_view(&TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor::default());
        self.sprites.prepare(
            &self.device,
            &self.queue,
            assets,
            nodes,
            camera,
            [extent.width, extent.height],
        )?;
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("2D scene"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Color {
                        r: 0.04,
                        g: 0.09,
                        b: 0.16,
                        a: 1.0,
                    }),
                    store: StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        self.sprites.draw(&mut pass);
        drop(pass);
        let readback = screenshot.map(|_| {
            let row = (extent.width * 4).next_multiple_of(COPY_BYTES_PER_ROW_ALIGNMENT);
            let buffer = self.device.create_buffer(&BufferDescriptor {
                label: Some("Player screenshot"),
                size: u64::from(row) * u64::from(extent.height),
                usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                frame.texture.as_image_copy(),
                TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row),
                        rows_per_image: None,
                    },
                },
                extent,
            );
            (buffer, row)
        });
        self.queue.submit([encoder.finish()]);
        window.pre_present_notify();
        frame.present();
        if let Some((path, (buffer, row))) = screenshot.zip(readback) {
            self.save_capture(path, &buffer, row, extent)?;
        }
        Ok(true)
    }

    fn save_capture(
        &self,
        path: &Path,
        buffer: &Buffer,
        row: u32,
        extent: Extent3d,
    ) -> anyhow::Result<()> {
        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        self.device.poll(PollType::wait_indefinitely())?;
        rx.recv()??;
        let mapped = slice.get_mapped_range();
        let mut pixels = Vec::new();
        for padded in mapped.chunks_exact(row as usize) {
            pixels.extend_from_slice(&padded[..extent.width as usize * 4]);
        }
        if matches!(
            self.config.format,
            TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb
        ) {
            for pixel in pixels.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
        }
        image::save_buffer_with_format(
            path,
            &pixels,
            extent.width,
            extent.height,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        )?;
        Ok(())
    }
}
