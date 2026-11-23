//! Fixed-step cuboid physics shared by the Rapier and optional Jolt backends.

pub(crate) mod authored;

use anyhow::{Context, Result, ensure};
use glam::{Quat, Vec3};
use serde::Serialize;

use rapier3d::prelude::{ColliderBuilder, PhysicsWorld, Pose, RigidBodyBuilder, RigidBodyHandle};

const STEP: f32 = 1.0 / 60.0;

/// Which solver advances a 3D world.
#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    /// Default pure-Rust solver.
    #[default]
    Rapier,
    /// Optional solver enabled by the `physics-jolt` feature.
    Jolt,
}

/// A handle scoped to its owning 3D world.
#[derive(Clone, Copy, Debug)]
pub struct BodyId(usize);

/// Initial cuboid pose and motion in meters and seconds.
#[derive(Clone, Copy, Debug)]
pub struct Cuboid {
    /// Center in world space, with positive Y up.
    pub center: Vec3,
    /// Positive local half extents.
    pub extent: Vec3,
    /// Initial orientation.
    pub orientation: Quat,
    /// Initial linear velocity for a dynamic body.
    pub velocity: Vec3,
    /// Whether gravity and contacts move the body.
    pub dynamic: bool,
}

/// Resolved pose and velocity in backend-independent coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct BodyState {
    /// World-space center.
    pub center: [f32; 3],
    /// Unit quaternion in XYZW order.
    pub orientation: [f32; 4],
    /// World-space linear velocity.
    pub velocity: [f32; 3],
}

enum Solver {
    Rapier(Box<PhysicsWorld>, Vec<RigidBodyHandle>),
    #[cfg(feature = "physics-jolt")]
    Jolt(Box<oxijolt::PhysicsWorld>, Vec<oxijolt::BodyId>),
}

/// A fixed 60 Hz 3D simulation independent of the renderer.
pub struct World3D(Solver);

impl World3D {
    /// Opens a world with the selected backend.
    pub fn new(backend: Backend) -> Result<Self> {
        Ok(Self(match backend {
            Backend::Rapier => {
                let mut world = PhysicsWorld::default();
                world.integration_parameters.dt = STEP;
                Solver::Rapier(Box::new(world), Vec::new())
            }
            Backend::Jolt => {
                #[cfg(feature = "physics-jolt")]
                {
                    Solver::Jolt(
                        Box::new(oxijolt::PhysicsWorld::new(
                            oxijolt::WorldSettings::default(),
                        )?),
                        Vec::new(),
                    )
                }
                #[cfg(not(feature = "physics-jolt"))]
                anyhow::bail!("Jolt requires the physics-jolt cargo feature");
            }
        }))
    }

    /// Adds a static or dynamic cuboid to the world.
    pub fn insert(&mut self, shape: Cuboid) -> Result<BodyId> {
        ensure!(
            shape.center.is_finite()
                && shape.velocity.is_finite()
                && shape.extent.is_finite()
                && shape.extent.cmpgt(Vec3::ZERO).all()
                && shape.orientation.is_finite()
                && shape.orientation.is_normalized(),
            "Invalid 3D cuboid transform or extent"
        );
        let h = shape.extent;
        let rotation = shape.orientation.normalize();
        let velocity = if shape.dynamic {
            shape.velocity
        } else {
            Vec3::ZERO
        };
        let handle = match &mut self.0 {
            Solver::Rapier(world, ids) => {
                let body = if shape.dynamic {
                    RigidBodyBuilder::dynamic()
                } else {
                    RigidBodyBuilder::fixed()
                };
                let body = body
                    .pose(Pose::from_parts(shape.center, rotation))
                    .linvel(velocity);
                let collider = ColliderBuilder::cuboid(h.x, h.y, h.z)
                    .friction(0.65)
                    .restitution(0.12);
                ids.push(world.insert(body, collider).0);
                ids.len() - 1
            }
            #[cfg(feature = "physics-jolt")]
            Solver::Jolt(world, ids) => {
                use oxijolt::prelude::math::{Quat as JoltQuat, RVec3, Vec3 as JoltVec};
                let c = shape.center;
                let body = if shape.dynamic {
                    oxijolt::BodySettings::new_dynamic()
                } else {
                    oxijolt::BodySettings::new_static()
                };
                let settings = body
                    .position(RVec3::new(c.x, c.y, c.z))
                    .rotation(JoltQuat::from_xyzw(
                        rotation.x, rotation.y, rotation.z, rotation.w,
                    ))
                    .linear_velocity(JoltVec::new(velocity.x, velocity.y, velocity.z))
                    .friction(0.65)
                    .restitution(0.12);
                let collider = oxijolt::Shape::new_box(JoltVec::new(h.x, h.y, h.z))?;
                ids.push(world.create_body(&collider, &settings)?);
                ids.len() - 1
            }
        };
        Ok(BodyId(handle))
    }

    /// Advances exactly one fixed tick.
    pub fn step(&mut self) -> Result<()> {
        match &mut self.0 {
            Solver::Rapier(world, _) => world.step(),
            #[cfg(feature = "physics-jolt")]
            Solver::Jolt(world, _) => {
                let report = world.step(STEP)?;
                ensure!(
                    report.is_complete(),
                    "Jolt dropped contacts during step: {report:?}"
                );
            }
        }
        Ok(())
    }

    /// Reads the pose and linear velocity of a body.
    pub fn state(&self, id: BodyId) -> Result<BodyState> {
        Ok(match &self.0 {
            Solver::Rapier(world, ids) => {
                let body = world
                    .bodies
                    .get(*ids.get(id.0).context("Unknown 3D body")?)
                    .context("Missing Rapier 3D body")?;
                BodyState {
                    center: body.translation().to_array(),
                    orientation: body.rotation().to_array(),
                    velocity: body.linvel().to_array(),
                }
            }
            #[cfg(feature = "physics-jolt")]
            Solver::Jolt(world, ids) => {
                let body = world.body(*ids.get(id.0).context("Unknown 3D body")?)?;
                let c = body.position();
                let q = body.rotation();
                let v = body.linear_velocity();
                BodyState {
                    center: [c.x, c.y, c.z],
                    orientation: [q.x, q.y, q.z, q.w],
                    velocity: [v.x, v.y, v.z],
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drop_box(backend: Backend) -> Result<BodyState> {
        let mut world = World3D::new(backend)?;
        let floor = Cuboid {
            center: Vec3::new(0.0, -0.25, 0.0),
            extent: Vec3::new(2.0, 0.25, 2.0),
            orientation: Quat::IDENTITY,
            velocity: Vec3::ZERO,
            dynamic: false,
        };
        let platform = world.insert(floor)?;
        let fixed = world.state(platform)?;
        let box_id = world.insert(Cuboid {
            center: Vec3::new(0.0, 3.0, 0.0),
            extent: Vec3::splat(0.5),
            dynamic: true,
            ..floor
        })?;
        for _ in 0..20 {
            world.step()?;
        }
        let falling = world.state(box_id)?;
        assert!(falling.center[1] < 3.0 && falling.velocity[1] < 0.0);
        for _ in 20..240 {
            world.step()?;
        }
        let landed = world.state(box_id)?;
        assert_eq!(world.state(platform)?, fixed);
        assert!((landed.center[1] - 0.5).abs() < 0.015, "{landed:?}");
        assert!(landed.velocity[1].abs() < 0.01, "{landed:?}");
        Ok(landed)
    }

    #[test]
    fn rapier_lands_reproducibly() -> Result<()> {
        assert_eq!(drop_box(Backend::Rapier)?, drop_box(Backend::Rapier)?);
        Ok(())
    }

    #[cfg(feature = "physics-jolt")]
    #[test]
    fn jolt_lands_on_the_same_platform() -> Result<()> {
        assert_eq!(drop_box(Backend::Jolt)?, drop_box(Backend::Jolt)?);
        Ok(())
    }
}
