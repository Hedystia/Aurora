//! Built-in drop test for the native 3D runtime.

use anyhow::{Context, Result};
use glam::{Quat, Vec3};

use crate::{
    physics3d::{Backend, BodyId, Cuboid, World3D},
    render::Cube,
};

pub(in crate::app) struct Spatial {
    pub(super) backend: Backend,
    world: World3D,
    handles: Vec<BodyId>,
    pub(super) cubes: Vec<Cube>,
    pub(super) ticks: u64,
}

impl Spatial {
    pub(super) fn new(backend: Backend) -> Result<Self> {
        let mut world = World3D::new(backend)?;
        let mut handles = Vec::new();
        let mut cubes = Vec::new();
        let platform = Cuboid {
            center: Vec3::new(0.0, -0.24, 0.0),
            extent: Vec3::new(3.6, 0.24, 2.65),
            orientation: Quat::IDENTITY,
            velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
            dynamic: false,
        };
        let cyan = Cuboid {
            center: Vec3::new(-1.45, 3.3, 0.2),
            extent: Vec3::splat(0.62),
            orientation: Quat::from_rotation_y(0.25),
            dynamic: true,
            ..platform
        };
        let gold = Cuboid {
            center: Vec3::new(0.0, 4.7, -0.7),
            extent: Vec3::splat(0.59),
            orientation: Quat::from_rotation_y(-0.32),
            dynamic: true,
            ..platform
        };
        let coral = Cuboid {
            center: Vec3::new(1.55, 2.9, 0.7),
            extent: Vec3::new(0.52, 0.78, 0.52),
            orientation: Quat::from_rotation_y(0.38),
            dynamic: true,
            ..platform
        };
        for (name, body, color) in [
            ("Platform", platform, [0.078, 0.155, 0.24]),
            ("Cyan", cyan, [0.045, 0.70, 0.78]),
            ("Amber", gold, [0.92, 0.49, 0.10]),
            ("Coral", coral, [0.82, 0.22, 0.29]),
        ] {
            let handle = world.insert(body)?;
            handles.push(handle);
            cubes.push(Cube {
                name,
                extent: body.extent.to_array(),
                color,
                state: world.state(handle)?,
            });
        }
        Ok(Self {
            backend,
            world,
            handles,
            cubes,
            ticks: 0,
        })
    }

    pub(super) fn tick(&mut self) -> Result<()> {
        let next = self.ticks.checked_add(1).context("3D clock exhausted")?;
        self.world.step()?;
        for (cube, handle) in self.cubes.iter_mut().zip(self.handles.iter().copied()) {
            cube.state = self.world.state(handle)?;
        }
        self.ticks = next;
        Ok(())
    }
}
