//! Instanced cuboids with a perspective camera and depth testing.

use anyhow::{Result, ensure};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Quat, Vec3};
use wgpu::{
    util::{BufferInitDescriptor, DeviceExt},
    *,
};

use super::Cube;
use crate::scene::{
    NodeState,
    spatial::{Role, View3D, matrices},
};

use crate::scene::spatial::Geometry;
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

pub(super) struct MeshScene {
    meshes: Vec<(Arc<Geometry>, Instance)>,
    camera: View3D,
    instances: Vec<Instance>,
}

impl MeshScene {
    pub(super) fn authored(nodes: &[NodeState]) -> Option<Self> {
        if !nodes.iter().any(|n| n.spatial.is_some() && n.visible) {
            return None;
        }
        let worlds = matrices(nodes);
        let camera = View3D::resolve(nodes, &worlds);
        let instances = nodes
            .iter()
            .filter(|n| n.visible)
            .filter_map(|n| {
                let spatial = n.spatial.as_ref()?;
                (spatial.role == Role::Cube).then(|| {
                    Instance::new(
                        worlds[&n.id] * Mat4::from_scale(Vec3::splat(0.5)),
                        spatial.color,
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
                    Instance::new(worlds[&n.id], s.color),
                ))
            })
            .collect();
        Some(Self {
            camera,
            instances,
            meshes,
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
                )
            })
            .collect();
        Self {
            camera: View3D::default(),
            instances,
            meshes: Vec::new(),
        }
    }
}

impl Instance {
    fn new(matrix: Mat4, color: [f32; 3]) -> Self {
        Self {
            model: matrix.to_cols_array_2d(),
            color: [color[0], color[1], color[2], 1.],
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
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    model: [[f32; 4]; 4],
    color: [f32; 4],
}

const VERTEX_ATTRIBUTES: [VertexAttribute; 2] = vertex_attr_array![0 => Float32x3, 1 => Float32x3];
const INSTANCE_ATTRIBUTES: [VertexAttribute; 5] = vertex_attr_array![2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4];

pub(super) struct MeshPass {
    pipeline: RenderPipeline,
    camera: Buffer,
    bindings: BindGroup,
    imported: HashMap<usize, (Weak<Geometry>, Buffer, u32)>,
    vertices: Buffer,
    instances: Buffer,
    depth: Option<(Extent3d, TextureView)>,
}

impl MeshPass {
    pub(super) fn new(device: &Device, format: TextureFormat) -> Self {
        let shader = device.create_shader_module(include_wgsl!("cube.wgsl"));
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Opaque 3D cuboids"),
            layout: None,
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
            size: 80,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bindings = device.create_bind_group(&BindGroupDescriptor {
            label: Some("3D camera bindings"),
            layout: &pipeline.get_bind_group_layout(0),
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
        Self {
            pipeline,
            camera,
            bindings,
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
        scene: MeshScene,
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
        let mut uniform = [0.0_f32; 20];
        uniform[..16].copy_from_slice(&projection.to_cols_array());
        uniform[16..19].copy_from_slice(&scene.camera.light.to_array());
        queue.write_buffer(&self.camera, 0, bytemuck::cast_slice(&uniform));
        let mut instances = scene.instances;
        let cube_count = u32::try_from(instances.len())?;
        let mut draws = Vec::new();
        self.imported
            .retain(|_, (geometry, _, _)| geometry.strong_count() > 0);
        for (geometry, instance) in scene.meshes {
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
                    vertices.extend([a, b, c].map(|position| Vertex {
                        position: position.to_array(),
                        normal,
                    }));
                }
                let buffer = device.create_buffer_init(&BufferInitDescriptor {
                    label: Some("Imported static mesh"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: BufferUsages::VERTEX,
                });
                entry.insert((Arc::downgrade(&geometry), buffer, count));
            }
            draws.push((key, u32::try_from(instances.len())?));
            instances.push(instance);
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
                    load: LoadOp::Clear(Color {
                        r: 0.018,
                        g: 0.033,
                        b: 0.065,
                        a: 1.0,
                    }),
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
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_vertex_buffer(1, self.instances.slice(..));
        pass.draw(0..VERTICES, 0..cube_count);
        for (key, index) in draws {
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
        }));
    }
    vertices
}
