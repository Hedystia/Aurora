//! wgpu surface ownership and render pass for the player.

mod mesh;
mod sprite;

use std::{path::Path, sync::Arc};

use anyhow::{Context, ensure};
use wgpu::*;
use winit::{dpi::PhysicalSize, window::Window};

use crate::{
    assets::Assets,
    scene::{Camera2D, NodeState},
};
use mesh::{MeshPass, MeshScene};
use serde::Serialize;
use sprite::SpriteBatch;

#[derive(Serialize)]
pub(crate) struct Cube {
    pub(crate) name: &'static str,
    pub(crate) extent: [f32; 3],
    pub(crate) color: [f32; 3],
    pub(crate) state: crate::physics3d::BodyState,
}

pub(crate) enum Frame<'a> {
    Sprites {
        assets: &'a Assets,
        nodes: Vec<NodeState>,
        camera: Camera2D,
        view3d: crate::save::CameraSelection,
    },
    Cubes(&'a [Cube]),
}

pub(crate) struct Renderer {
    instance: Instance,
    surface: Surface<'static>,
    gpu: GpuScene,
    config: SurfaceConfiguration,
    size: PhysicalSize<u32>,
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
        let gpu = GpuScene::new(device, queue, config.format);
        let mut renderer = Self {
            instance,
            surface,
            gpu,
            config,
            size,
        };
        renderer.resize(size);
        Ok(renderer)
    }

    pub(crate) fn resize(&mut self, size: PhysicalSize<u32>) {
        self.size = size;
        if size.width > 0 && size.height > 0 {
            self.config.width = size.width;
            self.config.height = size.height;
            self.surface.configure(&self.gpu.device, &self.config);
        }
    }

    pub(crate) fn draw(
        &mut self,
        window: &Arc<Window>,
        screenshot: Option<&Path>,
        render_frame: Frame<'_>,
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
        let (encoder, readback) =
            self.gpu
                .encode(&frame.texture, render_frame, screenshot.is_some())?;
        self.gpu.queue.submit([encoder.finish()]);
        window.pre_present_notify();
        frame.present();
        if let Some((path, readback)) = screenshot.zip(readback) {
            let pixels = self.gpu.read(&readback)?;
            image::save_buffer_with_format(
                path,
                &pixels,
                readback.extent.width,
                readback.extent.height,
                image::ColorType::Rgba8,
                image::ImageFormat::Png,
            )?;
        }
        Ok(true)
    }
}

struct Readback {
    buffer: Buffer,
    row: u32,
    extent: Extent3d,
}

struct GpuScene {
    device: Device,
    queue: Queue,
    format: TextureFormat,
    sprites: SpriteBatch,
    mesh: Option<MeshPass>,
}

impl GpuScene {
    fn new(device: Device, queue: Queue, format: TextureFormat) -> Self {
        let sprites = SpriteBatch::new(&device, format);
        Self {
            device,
            queue,
            format,
            sprites,
            mesh: None,
        }
    }

    fn encode(
        &mut self,
        target: &Texture,
        frame: Frame<'_>,
        capture: bool,
    ) -> anyhow::Result<(CommandEncoder, Option<Readback>)> {
        let extent = target.size();
        let view = target.create_view(&TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor::default());
        match frame {
            Frame::Sprites {
                assets,
                nodes,
                camera,
                view3d,
            } => {
                let spatial = MeshScene::authored(&nodes, assets, view3d);
                let overlay = spatial.is_some();
                if let Some(scene) = spatial {
                    self.mesh
                        .get_or_insert_with(|| {
                            MeshPass::new(&self.device, &self.queue, self.format)
                        })
                        .encode(
                            &self.device,
                            &self.queue,
                            &mut encoder,
                            &view,
                            extent,
                            scene,
                        )?;
                }
                self.encode_sprites(
                    &mut encoder,
                    &view,
                    extent,
                    assets,
                    &nodes,
                    (camera, overlay),
                )?;
            }
            Frame::Cubes(cubes) => {
                self.mesh
                    .get_or_insert_with(|| MeshPass::new(&self.device, &self.queue, self.format))
                    .encode(
                        &self.device,
                        &self.queue,
                        &mut encoder,
                        &view,
                        extent,
                        MeshScene::demo(cubes),
                    )?;
            }
        }
        let readback = capture.then(|| {
            let row = (extent.width * 4).next_multiple_of(COPY_BYTES_PER_ROW_ALIGNMENT);
            let buffer = self.device.create_buffer(&BufferDescriptor {
                label: Some("Frame readback"),
                size: u64::from(row) * u64::from(extent.height),
                usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                target.as_image_copy(),
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
            Readback {
                buffer,
                row,
                extent,
            }
        });
        Ok((encoder, readback))
    }

    fn encode_sprites(
        &mut self,
        encoder: &mut CommandEncoder,
        view: &TextureView,
        extent: Extent3d,
        assets: &Assets,
        nodes: &[NodeState],
        (camera, overlay): (Camera2D, bool),
    ) -> anyhow::Result<()> {
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
                view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: if overlay {
                        LoadOp::Load
                    } else {
                        LoadOp::Clear(Color {
                            r: 0.04,
                            g: 0.09,
                            b: 0.16,
                            a: 1.0,
                        })
                    },
                    store: StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        let [sx, sy] = camera.scale([extent.width, extent.height]);
        let width = (camera.design_size[0] * sx).round() as u32;
        let height = (camera.design_size[1] * sy).round() as u32;
        let width = width.clamp(1, extent.width);
        let height = height.clamp(1, extent.height);
        pass.set_scissor_rect(
            (extent.width - width) / 2,
            (extent.height - height) / 2,
            width,
            height,
        );
        self.sprites.draw(&mut pass);
        drop(pass);
        Ok(())
    }

    fn read(&self, readback: &Readback) -> anyhow::Result<Vec<u8>> {
        let slice = readback.buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        self.device.poll(PollType::wait_indefinitely())?;
        rx.recv()??;
        let mapped = slice.get_mapped_range();
        let mut pixels = Vec::with_capacity(
            readback.extent.width as usize * readback.extent.height as usize * 4,
        );
        for padded in mapped.chunks_exact(readback.row as usize) {
            pixels.extend_from_slice(&padded[..readback.extent.width as usize * 4]);
        }
        if matches!(
            self.format,
            TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb
        ) {
            for pixel in pixels.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
        }
        Ok(pixels)
    }
}

pub(crate) struct Offscreen {
    gpu: GpuScene,
    target: Texture,
}

impl Offscreen {
    pub(crate) async fn new([width, height]: [u32; 2]) -> anyhow::Result<Self> {
        let instance = Instance::new(InstanceDescriptor::new_without_display_handle().with_env());
        let adapter = instance
            .request_adapter(&RequestAdapterOptions::default())
            .await?;
        let (device, queue) = adapter.request_device(&DeviceDescriptor::default()).await?;
        let format = TextureFormat::Rgba8UnormSrgb;
        let target = device.create_texture(&TextureDescriptor {
            label: Some("Editor preview"),
            size: Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsages::COPY_SRC | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        Ok(Self {
            gpu: GpuScene::new(device, queue, format),
            target,
        })
    }

    pub(crate) fn frame(&mut self, frame: Frame<'_>) -> anyhow::Result<Vec<u8>> {
        let (encoder, readback) = self.gpu.encode(&self.target, frame, true)?;
        self.gpu.queue.submit([encoder.finish()]);
        self.gpu
            .read(&readback.context("Missing preview readback")?)
    }
}
