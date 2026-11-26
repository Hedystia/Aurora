//! Minimal WebGPU canvas surface for the browser shell.
//!
//! This mirrors the surface-lifetime invariants of engine_core's native
//! renderer (zero-size surfaces are undrawable, outdated configurations are
//! reapplied, lost surfaces are recreated) without reaching into its private
//! render module. Only a clear-colour pass is implemented: sprite, text and
//! mesh batches remain native-only groundwork.

use web_sys::HtmlCanvasElement;
use wgpu::{
    CommandEncoderDescriptor, CurrentSurfaceTexture, Device, Instance, InstanceDescriptor, Queue,
    RenderPassColorAttachment, RenderPassDescriptor, RequestAdapterOptions, StoreOp, Surface,
    SurfaceConfiguration, SurfaceTarget, SurfaceTexture, TextureUsages, TextureViewDescriptor,
};

/// Owns the WebGPU device and the canvas surface presented to the browser.
pub(crate) struct SurfaceRenderer {
    canvas: HtmlCanvasElement,
    instance: Instance,
    surface: Surface<'static>,
    device: Device,
    queue: Queue,
    config: SurfaceConfiguration,
    size: [u32; 2],
}

impl SurfaceRenderer {
    /// Creates a WebGPU surface for an existing canvas element.
    pub(crate) async fn new(canvas: HtmlCanvasElement) -> Result<Self, String> {
        let size = [canvas.width(), canvas.height()];
        let instance = Instance::new(InstanceDescriptor::default());
        let surface = instance
            .create_surface(SurfaceTarget::Canvas(canvas.clone()))
            .map_err(|error| format!("create canvas surface: {error}"))?;
        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(|error| format!("request WebGPU adapter: {error}"))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|error| format!("request WebGPU device: {error}"))?;
        let mut config = surface
            .get_default_config(&adapter, size[0], size[1])
            .ok_or_else(|| "canvas has no compatible WebGPU configuration".to_owned())?;
        config.usage |= TextureUsages::RENDER_ATTACHMENT;
        surface.configure(&device, &config);
        Ok(Self {
            canvas,
            instance,
            surface,
            device,
            queue,
            config,
            size,
        })
    }

    /// Applies a new canvas backing-store size. Zero-sized surfaces are skipped.
    pub(crate) fn resize(&mut self, size: [u32; 2]) {
        if self.size == size {
            return;
        }
        self.size = size;
        if size[0] > 0 && size[1] > 0 {
            self.config.width = size[0];
            self.config.height = size[1];
            self.surface.configure(&self.device, &self.config);
        }
    }

    /// Clears the canvas to `color`. Geometry is intentionally not drawn yet.
    pub(crate) fn draw(&mut self, color: [f32; 4]) -> Result<(), String> {
        if self.size[0] == 0 || self.size[1] == 0 {
            return Ok(());
        }
        let Some(frame) = self.acquire()? else {
            return Ok(());
        };
        let view = frame.texture.create_view(&TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("Aurora web clear"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(color[0]),
                            g: f64::from(color[1]),
                            b: f64::from(color[2]),
                            a: f64::from(color[3]),
                        }),
                        store: StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            // No geometry is submitted: this pass proves the surface, device
            // and RAF loop are wired before the scene renderer is ported.
            let _ = &mut pass;
        }
        self.queue.submit([encoder.finish()]);
        frame.present();
        Ok(())
    }

    /// Handles the recoverable surface states and returns a drawable texture.
    fn acquire(&mut self) -> Result<Option<SurfaceTexture>, String> {
        match self.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(frame) | CurrentSurfaceTexture::Suboptimal(frame) => {
                Ok(Some(frame))
            }
            CurrentSurfaceTexture::Timeout | CurrentSurfaceTexture::Occluded => Ok(None),
            CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                Ok(None)
            }
            CurrentSurfaceTexture::Lost => {
                self.surface = self
                    .instance
                    .create_surface(SurfaceTarget::Canvas(self.canvas.clone()))
                    .map_err(|error| format!("recreate canvas surface: {error}"))?;
                self.surface.configure(&self.device, &self.config);
                Ok(None)
            }
            CurrentSurfaceTexture::Validation => {
                Err("surface texture validation failed".to_owned())
            }
        }
    }
}
