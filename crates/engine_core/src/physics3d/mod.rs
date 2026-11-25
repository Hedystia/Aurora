//! Fixed-step cuboid physics shared by the Rapier and optional Jolt backends.

pub(crate) mod authored;

use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, ensure};
use glam::{Quat, Vec3};
use serde::Serialize;

use rapier3d::prelude::{ColliderBuilder, PhysicsWorld, Pose, RigidBodyBuilder, RigidBodyHandle};

const STEP: f32 = 1.0 / 60.0;

/// Backend used by authored worlds instantiated from now on.
///
/// The player selects this once when it starts; authored worlds capture it when
/// they are constructed, so scene changes keep the same solver. Defaults to
/// Rapier, which keeps every non-Jolt build and host unchanged.
static AUTHORED_JOLT: AtomicBool = AtomicBool::new(false);

/// Selects the solver for authored 3D worlds created after this call.
///
/// This is a process-wide player option: the player calls it before it builds
/// its runtime, and `AuthoredWorld` reads it when a scene is instantiated. It
/// is not a per-scene setting.
pub fn set_authored_backend(backend: Backend) -> Result<()> {
    match backend {
        Backend::Rapier => AUTHORED_JOLT.store(false, Ordering::Relaxed),
        Backend::Jolt => {
            #[cfg(feature = "physics-jolt")]
            AUTHORED_JOLT.store(true, Ordering::Relaxed);
            #[cfg(not(feature = "physics-jolt"))]
            anyhow::bail!("Jolt requires the physics-jolt cargo feature");
        }
    }
    Ok(())
}

pub(crate) fn authored_backend() -> Backend {
    if AUTHORED_JOLT.load(Ordering::Relaxed) {
        Backend::Jolt
    } else {
        Backend::Rapier
    }
}

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
    /// Initial angular velocity in radians per second.
    pub angular_velocity: Vec3,
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
    /// World-space angular velocity in radians per second.
    pub angular_velocity: [f32; 3],
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
        let angular = if shape.dynamic {
            shape.angular_velocity
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
                    .linvel(velocity)
                    .angvel(angular);
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
                    .angular_velocity(JoltVec::new(angular.x, angular.y, angular.z))
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
                    angular_velocity: body.angvel().to_array(),
                }
            }
            #[cfg(feature = "physics-jolt")]
            Solver::Jolt(world, ids) => {
                let body = world.body(*ids.get(id.0).context("Unknown 3D body")?)?;
                let c = body.position();
                let q = body.rotation();
                let v = body.linear_velocity();
                let a = body.angular_velocity();
                BodyState {
                    center: [c.x, c.y, c.z],
                    orientation: [q.x, q.y, q.z, q.w],
                    velocity: [v.x, v.y, v.z],
                    angular_velocity: [a.x, a.y, a.z],
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
            angular_velocity: Vec3::ZERO,
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
