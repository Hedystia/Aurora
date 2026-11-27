//! Instanced cuboids and imported meshes with a perspective camera and depth testing.

use anyhow::{Result, ensure};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Quat, Vec3};
use image::{Rgba, RgbaImage};
use wgpu::{
    util::{BufferInitDescriptor, DeviceExt},
    *,
};

use super::Cube;
use crate::assets::{AssetId, Assets};
use crate::scene::{
    NodeState,
    spatial::{Role, View3D, matrices},
};

use crate::scene::spatial::Geometry;
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

pub(super) struct MeshScene<'a> {
    meshes: Vec<(Arc<Geometry>, Instance, Option<AssetId>)>,
    camera: View3D,
    instances: Vec<Instance>,
    assets: Option<&'a Assets>,
}

impl<'a> MeshScene<'a> {
    pub(super) fn authored(
        nodes: &'a [NodeState],
        assets: &'a Assets,
        selection: crate::save::CameraSelection,
    ) -> Option<Self> {
        if !nodes.iter().any(|n| n.spatial.is_some() && n.visible) {
            return None;
        }
        let worlds = matrices(nodes);
        let camera = View3D::resolve_selected(nodes, &worlds, selection)?;
        let instances = nodes
            .iter()
            .filter(|n| n.visible)
            .filter_map(|n| {
                let spatial = n.spatial.as_ref()?;
                (spatial.role == Role::Cube).then(|| {
                    Instance::new(
                        worlds[&n.id] * Mat4::from_scale(Vec3::splat(0.5)),
                        spatial.color,
                        spatial.metallic,
                        spatial.roughness,
                    )
                })
            })
            .collect();
        let meshes = nodes
            .iter()
            .filter(|n| n.visible)
            .filter_map(|n| {
                let s = n.spatial.as_ref()?;
                Some((
                    Arc::clone(s.geometry.as_ref()?),
                    Instance::new(worlds[&n.id], s.color, s.metallic, s.roughness),
                    s.texture_asset,
                ))
            })
            .collect();
        Some(Self {
            camera,
            instances,
            meshes,
            assets: Some(assets),
        })
    }

    pub(super) fn demo(cubes: &[Cube]) -> Self {
        let instances = cubes
            .iter()
            .map(|cube| {
                Instance::new(
                    Mat4::from_scale_rotation_translation(
                        Vec3::from_array(cube.extent),
                        Quat::from_array(cube.state.orientation),
                        Vec3::from_array(cube.state.center),
                    ),
                    cube.color,
                    0.,
                    0.5,
                )
            })
            .collect();
        Self {
            camera: View3D::default(),
            instances,
            meshes: Vec::new(),
            assets: None,
        }
    }
}

impl Instance {
    fn new(matrix: Mat4, color: [f32; 3], metallic: f32, roughness: f32) -> Self {
        Self {
            model: matrix.to_cols_array_2d(),
            color: [color[0], color[1], color[2], 1.],
            params: [metallic, roughness, 0., 0.],
        }
    }
}

const DEPTH_FORMAT: TextureFormat = TextureFormat::Depth32Float;
const VERTICES: u32 = 36;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    model: [[f32; 4]; 4],
    color: [f32; 4],
    params: [f32; 4],
}

const VERTEX_ATTRIBUTES: [VertexAttribute; 3] =
    vertex_attr_array![0 => Float32x3, 1 => Float32x3, 7 => Float32x2];
const INSTANCE_ATTRIBUTES: [VertexAttribute; 6] = vertex_attr_array![
    2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 8 => Float32x4
];

struct MaterialBinding {
    _texture: Texture,
    _view: TextureView,
    bind_group: BindGroup,
}

pub(super) struct MeshPass {
    pipeline: RenderPipeline,
    camera: Buffer,
    bindings: BindGroup,
    material_layout: BindGroupLayout,
    material: MaterialBinding,
    textures: HashMap<AssetId, MaterialBinding>,
    imported: HashMap<usize, (Weak<Geometry>, Buffer, u32)>,
    vertices: Buffer,
    instances: Buffer,
    depth: Option<(Extent3d, TextureView)>,
}

impl MeshPass {
    pub(super) fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
        let shader = device.create_shader_module(include_wgsl!("cube.wgsl"));
        let camera_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("3D camera layout"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX_FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let material_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("3D material layout"),
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
        let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("3D mesh layout"),
            bind_group_layouts: &[Some(&camera_layout), Some(&material_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Opaque 3D meshes"),
            layout: Some(&layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: PipelineCompilationOptions::default(),
                buffers: &[
                    VertexBufferLayout {
                        array_stride: size_of::<Vertex>() as u64,
                        step_mode: VertexStepMode::Vertex,
                        attributes: &VERTEX_ATTRIBUTES,
                    },
                    VertexBufferLayout {
                        array_stride: size_of::<Instance>() as u64,
                        step_mode: VertexStepMode::Instance,
                        attributes: &INSTANCE_ATTRIBUTES,
                    },
                ],
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: PipelineCompilationOptions::default(),
                targets: &[Some(ColorTargetState {
                    format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: PrimitiveState::default(),
            depth_stencil: Some(DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(CompareFunction::Less),
                stencil: StencilState::default(),
                bias: DepthBiasState::default(),
            }),
            multisample: MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let camera = device.create_buffer(&BufferDescriptor {
            label: Some("3D camera"),
            size: 96,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bindings = device.create_bind_group(&BindGroupDescriptor {
            label: Some("3D camera bindings"),
            layout: &camera_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: camera.as_entire_binding(),
            }],
        });
        let vertices = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("Unit cuboid vertices"),
            contents: bytemuck::cast_slice(&unit_cube()),
            usage: BufferUsages::VERTEX,
        });
        let material = material_binding(
            device,
            queue,
            &material_layout,
            &RgbaImage::from_pixel(1, 1, Rgba([255, 255, 255, 255])),
        );
        Self {
            pipeline,
            camera,
            bindings,
            material_layout,
            material,
            textures: HashMap::new(),
            imported: HashMap::new(),
            vertices,
            instances: instance_buffer(device, size_of::<Instance>() as u64),
            depth: None,
        }
    }

    pub(super) fn encode(
        &mut self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        view: &TextureView,
        extent: Extent3d,
        scene: MeshScene<'_>,
    ) -> Result<()> {
        if self.depth.as_ref().is_none_or(|(size, _)| *size != extent) {
            let depth = device.create_texture(&TextureDescriptor {
                label: Some("3D depth"),
                size: extent,
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            self.depth = Some((extent, depth.create_view(&TextureViewDescriptor::default())));
        }
        let depth = &self
            .depth
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing depth attachment"))?
            .1;
        let projection = scene
            .camera
            .projection(extent.width as f32 / extent.height as f32);
        let mut uniform = [0.0_f32; 24];
        uniform[..16].copy_from_slice(&projection.to_cols_array());
        uniform[16..19].copy_from_slice(&scene.camera.light.to_array());
        let eye = scene.camera.view.inverse().w_axis.truncate();
        uniform[20..23].copy_from_slice(&eye.to_array());
        queue.write_buffer(&self.camera, 0, bytemuck::cast_slice(&uniform));
        let assets = scene.assets;
        let mut instances = scene.instances;
        let cube_count = u32::try_from(instances.len())?;
        let mut draws = Vec::new();
        self.imported
            .retain(|_, (geometry, _, _)| geometry.strong_count() > 0);
        let mut needed = Vec::new();
        for (geometry, instance, texture) in scene.meshes {
            let key = Arc::as_ptr(&geometry) as usize;
            if let std::collections::hash_map::Entry::Vacant(entry) = self.imported.entry(key) {
                let count = u32::try_from(geometry.indices.len())?;
                ensure!(
                    u64::from(count) * size_of::<Vertex>() as u64
                        <= device.limits().max_buffer_size,
                    "Imported mesh exceeds device limit"
                );
                let mut vertices = Vec::with_capacity(count as usize);
                for triangle in geometry.indices.as_chunks::<3>().0 {
                    let [a, b, c] =
                        triangle.map(|i| Vec3::from_array(geometry.positions[i as usize]));
                    let normal = (b - a).cross(c - a).normalize_or_zero().to_array();
                    for (vertex, index) in [a, b, c].into_iter().zip(triangle) {
                        let uv = geometry
                            .uvs
                            .as_ref()
                            .map_or([0., 0.], |uvs| uvs[*index as usize]);
                        vertices.push(Vertex {
                            position: vertex.to_array(),
                            normal,
                            uv,
                        });
                    }
                }
                let buffer = device.create_buffer_init(&BufferInitDescriptor {
                    label: Some("Imported static mesh"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: BufferUsages::VERTEX,
                });
                entry.insert((Arc::downgrade(&geometry), buffer, count));
            }
            draws.push((key, u32::try_from(instances.len())?, texture));
            instances.push(instance);
        }
        if let Some(assets) = assets {
            for texture in draws.iter().filter_map(|(_, _, texture)| *texture) {
                if !self.textures.contains_key(&texture)
                    && let Some(image) = assets.texture(texture)
                {
                    needed.push((texture, image.clone()));
                }
            }
        }
        for (texture, image) in needed {
            self.textures.insert(
                texture,
                material_binding(device, queue, &self.material_layout, &image),
            );
        }
        let bytes = bytemuck::cast_slice(&instances);

        let capacity = bytes.len() as u64;
        let limit = device.limits().max_buffer_size;
        ensure!(
            capacity <= limit,
            "3D instance buffer exceeds device limits"
        );
        if capacity > self.instances.size() {
            self.instances = instance_buffer(device, capacity.next_power_of_two().min(limit));
        }
        if !bytes.is_empty() {
            queue.write_buffer(&self.instances, 0, bytes);
        }
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("3D view"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(super::BLACK),
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(Operations {
                    load: LoadOp::Clear(1.0),
                    store: StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bindings, &[]);
        pass.set_bind_group(1, &self.material.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_vertex_buffer(1, self.instances.slice(..));
        pass.draw(0..VERTICES, 0..cube_count);
        for (key, index, texture) in draws {
            let material = texture
                .and_then(|texture| self.textures.get(&texture))
                .unwrap_or(&self.material);
            pass.set_bind_group(1, &material.bind_group, &[]);
            let (_, buffer, count) = &self.imported[&key];
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.draw(0..*count, index..index + 1);
        }
        Ok(())
    }
}

fn instance_buffer(device: &Device, size: u64) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label: Some("Cuboid transforms"),
        size,
        usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn unit_cube() -> Vec<Vertex> {
    let faces = [
        (Vec3::X, Vec3::Y, Vec3::Z),
        (-Vec3::X, Vec3::Y, -Vec3::Z),
        (Vec3::Y, Vec3::Z, Vec3::X),
        (-Vec3::Y, Vec3::Z, -Vec3::X),
        (Vec3::Z, Vec3::X, Vec3::Y),
        (-Vec3::Z, Vec3::X, -Vec3::Y),
    ];
    let uvs = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]];
    let mut vertices = Vec::with_capacity(VERTICES as usize);
    for (normal, u, v) in faces {
        let corners = [
            normal - u - v,
            normal + u - v,
            normal + u + v,
            normal - u + v,
        ];
        vertices.extend([0, 1, 2, 0, 2, 3].map(|i| Vertex {
            position: corners[i].to_array(),
            normal: normal.to_array(),
            uv: uvs[i],
        }));
    }
    vertices
}

fn material_binding(
    device: &Device,
    queue: &Queue,
    layout: &BindGroupLayout,
    image: &RgbaImage,
) -> MaterialBinding {
    let size = Extent3d {
        width: image.width(),
        height: image.height(),
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("3D base color"),
        size,
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
            bytes_per_row: Some(4 * image.width()),
            rows_per_image: Some(image.height()),
        },
        size,
    );
    let view = texture.create_view(&TextureViewDescriptor::default());
    let sampler = device.create_sampler(&SamplerDescriptor {
        label: Some("3D base color sampler"),
        address_mode_u: AddressMode::Repeat,
        address_mode_v: AddressMode::Repeat,
        address_mode_w: AddressMode::Repeat,
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..Default::default()
    });
    let bind_group = device.create_bind_group(&BindGroupDescriptor {
        label: Some("3D material bindings"),
        layout,
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(&view),
            },
            BindGroupEntry {
                binding: 1,
                resource: BindingResource::Sampler(&sampler),
            },
        ],
    });
    MaterialBinding {
        _texture: texture,
        _view: view,
        bind_group,
    }
}
