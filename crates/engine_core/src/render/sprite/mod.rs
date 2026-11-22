//! Textured quad batches, preserving the scene's compositing order.

use std::{
    collections::{HashMap, hash_map::Entry},
    ops::Range,
};

use anyhow::{Context, Result, ensure};
use bytemuck::{Pod, Zeroable};
use wgpu::*;

use crate::{
    assets::{AssetId, Assets},
    scene::{Camera2D, NodeState},
};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 2],
    uv: [f32; 2],
    tint: [f32; 4],
}

const ATTRIBUTES: [VertexAttribute; 3] =
    vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4];

pub(super) struct SpriteBatch {
    pipeline: RenderPipeline,
    layout: BindGroupLayout,
    sampler: Sampler,
    textures: HashMap<AssetId, BindGroup>,
    buffer: Buffer,
    vertices: Vec<Vertex>,
    batches: Vec<(AssetId, Range<u32>)>,
}

impl SpriteBatch {
    pub(super) fn new(device: &Device, format: TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("Sprite texture layout"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Sprite pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(include_wgsl!("quad.wgsl"));
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Sprite pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: PipelineCompilationOptions::default(),
                buffers: &[VertexBufferLayout {
                    array_stride: size_of::<Vertex>() as u64,
                    step_mode: VertexStepMode::Vertex,
                    attributes: &ATTRIBUTES,
                }],
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: PipelineCompilationOptions::default(),
                targets: &[Some(ColorTargetState {
                    format,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&SamplerDescriptor {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            layout,
            sampler,
            textures: HashMap::new(),
            buffer: vertex_buffer(device, 1024),
            vertices: Vec::new(),
            batches: Vec::new(),
        }
    }

    pub(super) fn prepare(
        &mut self,
        device: &Device,
        queue: &Queue,
        assets: &Assets,
        nodes: &[NodeState],
        camera: Camera2D,
        viewport: [u32; 2],
    ) -> Result<()> {
        self.vertices.clear();
        self.batches.clear();
        for node in nodes.iter().filter(|node| node.visible) {
            let Some(sprite) = &node.sprite else { continue };
            let id = sprite.texture;
            if let Entry::Vacant(entry) = self.textures.entry(id) {
                let image = assets
                    .texture(id)
                    .with_context(|| format!("Missing sprite texture {id}"))?;
                let extent = Extent3d {
                    width: image.width(),
                    height: image.height(),
                    depth_or_array_layers: 1,
                };
                let texture = device.create_texture(&TextureDescriptor {
                    label: Some("Imported sprite"),
                    size: extent,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: TextureFormat::Rgba8UnormSrgb,
                    usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                queue.write_texture(
                    texture.as_image_copy(),
                    image.as_raw(),
                    TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(4 * extent.width),
                        rows_per_image: None,
                    },
                    extent,
                );
                let view = texture.create_view(&TextureViewDescriptor::default());
                entry.insert(device.create_bind_group(&BindGroupDescriptor {
                    label: Some("Imported sprite bindings"),
                    layout: &self.layout,
                    entries: &[
                        BindGroupEntry {
                            binding: 0,
                            resource: BindingResource::TextureView(&view),
                        },
                        BindGroupEntry {
                            binding: 1,
                            resource: BindingResource::Sampler(&self.sampler),
                        },
                    ],
                }));
            }
            let start = u32::try_from(self.vertices.len())?;
            let appearance = node.appearance.unwrap_or_default();
            let corners = appearance.corners(sprite.size);
            let uv = [[0.0, 0.0], [0.0, 1.0], [1.0, 0.0], [1.0, 1.0]];
            let [r, g, b, a] = appearance.color.map(|value| f32::from(value) / 255.0);
            let tint = [
                linear(r),
                linear(g),
                linear(b),
                a * (f32::from(appearance.opacity) / 255.0),
            ];
            self.vertices.extend([0, 1, 2, 2, 1, 3].map(|index| Vertex {
                position: camera.project(node.world.point(corners[index]), viewport),
                uv: uv[index],
                tint,
            }));
            let end = u32::try_from(self.vertices.len())?;
            if let Some((last, range)) = self.batches.last_mut()
                && *last == id
            {
                range.end = end;
            } else {
                self.batches.push((id, start..end));
            }
        }
        let bytes = bytemuck::cast_slice(&self.vertices);
        let buffer_size = bytes.len() as u64;
        let limit = device.limits().max_buffer_size;
        ensure!(
            buffer_size <= limit,
            "Sprite batch exceeds the device buffer limit"
        );
        if buffer_size > self.buffer.size() {
            self.buffer = vertex_buffer(device, buffer_size.next_power_of_two().min(limit));
        }
        if !bytes.is_empty() {
            queue.write_buffer(&self.buffer, 0, bytes);
        }
        Ok(())
    }

    pub(super) fn draw(&self, pass: &mut RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(0, self.buffer.slice(..));
        for (id, vertices) in &self.batches {
            pass.set_bind_group(0, &self.textures[id], &[]);
            pass.draw(vertices.clone(), 0..1);
        }
    }
}

fn vertex_buffer(device: &Device, size: u64) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label: Some("Sprite vertices"),
        size,
        usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}
