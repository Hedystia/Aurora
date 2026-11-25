//! Scene-local ownership of authored 3D bodies and colliders.

use crate::{
    ecs::EntityId,
    physics3d::Backend,
    scene::{
        Scene,
        spatial::{Body, Collider, Geometry, JointKind, matrices},
    },
};
use anyhow::{Context, Result, bail, ensure};
#[cfg(feature = "physics-jolt")]
use glam::Quat;
use glam::{EulerRot, Mat4, Vec3};
use rapier3d::prelude::{
    ActiveCollisionTypes, ColliderBuilder, ColliderHandle, FixedJointBuilder, ImpulseJointHandle,
    PhysicsWorld, Pose, PrismaticJointBuilder, QueryFilter, Ray, RevoluteJointBuilder,
    RigidBodyBuilder, RigidBodyHandle, SphericalJointBuilder,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
};

#[cfg(feature = "physics-jolt")]
use oxijolt::{
    Activation, BodyId as JoltBodyId, BodySettings as JoltBodySettings, ContactEvent,
    EventSettings as JoltEventSettings, Quat as JoltQuat, RVec3 as JoltRVec, Shape as JoltShape,
    Vec3 as JoltVec, WorldEvents as JoltWorldEvents,
};

/// Solver owning the authored bodies of one scene.
enum Solver {
    Rapier(Box<PhysicsWorld>),
    #[cfg(feature = "physics-jolt")]
    Jolt(Box<oxijolt::PhysicsWorld>),
}

/// One authored body in whichever solver owns the scene.
#[derive(Clone, Copy)]
enum BodyHandle {
    Rapier {
        body: RigidBodyHandle,
        collider: ColliderHandle,
    },
    #[cfg(feature = "physics-jolt")]
    Jolt(JoltBodyId),
}

impl BodyHandle {
    fn rapier_body(self) -> Option<RigidBodyHandle> {
        match self {
            Self::Rapier { body, .. } => Some(body),
            #[cfg(feature = "physics-jolt")]
            Self::Jolt(_) => None,
        }
    }

    fn rapier_collider(self) -> Option<ColliderHandle> {
        match self {
            Self::Rapier { collider, .. } => Some(collider),
            #[cfg(feature = "physics-jolt")]
            Self::Jolt(_) => None,
        }
    }
}

/// Everything needed to build one Jolt body from an authored node.
#[cfg(feature = "physics-jolt")]
struct JoltBodyDesc<'a> {
    shape: Collider,
    scale: Vec3,
    geometry: Option<&'a Geometry>,
    trigger: bool,
    friction: f32,
    restitution: f32,
    kind: Body,
    position: Vec3,
    rotation: Quat,
}

#[cfg(feature = "physics-jolt")]
fn jolt_vec(value: Vec3) -> JoltVec {
    JoltVec::new(value.x, value.y, value.z)
}

#[cfg(feature = "physics-jolt")]
fn jolt_rvec(value: Vec3) -> JoltRVec {
    JoltRVec::new(value.x, value.y, value.z)
}

#[cfg(feature = "physics-jolt")]
fn jolt_quat(value: Quat) -> JoltQuat {
    JoltQuat::from_xyzw(value.x, value.y, value.z, value.w)
}

#[cfg(feature = "physics-jolt")]
fn jolt_position(value: JoltRVec) -> Vec3 {
    Vec3::new(value.x, value.y, value.z)
}

#[cfg(feature = "physics-jolt")]
fn jolt_rotation(value: JoltQuat) -> Quat {
    Quat::from_xyzw(value.x, value.y, value.z, value.w)
}

/// A contact or trigger transition for one scene tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Contact {
    pub nodes: [EntityId; 2],
    pub entered: bool,
    /// True when at least one collider in the pair is a trigger.
    pub trigger: bool,
}

/// Query shape in metres, independent of the solver backend.
#[derive(Clone, Copy, Debug)]
pub(crate) enum QueryShape3D {
    Box([f32; 3]),
    /// Available to host callers that need non-box proximity queries.
    #[allow(dead_code)]
    Sphere(f32),
    #[allow(dead_code)]
    Capsule {
        radius: f32,
        half_height: f32,
    },
}

impl QueryShape3D {
    fn rapier(&self) -> rapier3d::prelude::ColliderBuilder {
        match *self {
            Self::Box(half) => ColliderBuilder::cuboid(half[0], half[1], half[2]),
            Self::Sphere(radius) => ColliderBuilder::ball(radius),
            Self::Capsule {
                radius,
                half_height,
            } => ColliderBuilder::capsule_y(half_height, radius),
        }
    }
}

/// Authored constraint between two scene bodies.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Joint {
    pub a: EntityId,
    pub b: EntityId,
    pub kind: JointKind,
    pub anchor_a: [f32; 3],
    pub anchor_b: [f32; 3],
    pub axis: [f32; 3],
}

/// Result of a world-space raycast against authored colliders.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RayHit3D {
    /// Collider owner retained for diagnostics and future script delivery.
    #[allow(dead_code)]
    pub node: EntityId,
    pub distance: f32,
    pub normal: [f32; 3],
}

#[derive(Clone, Copy)]
struct AuthoredBody {
    handle: BodyHandle,
    kind: Body,
    shape: Collider,
    trigger: bool,
    friction: f32,
    restitution: f32,
    scale: Vec3,
    geometry: Option<usize>,
}

pub(crate) struct AuthoredWorld {
    backend: Backend,
    world: Option<Solver>,
    handles: BTreeMap<EntityId, AuthoredBody>,
    contacts: BTreeSet<(EntityId, EntityId)>,
    transitions: Vec<Contact>,
    joints: Vec<Joint>,
    joint_handles: Vec<ImpulseJointHandle>,
    pending: BTreeMap<EntityId, [f32; 3]>,
    pending_angular: BTreeMap<EntityId, [f32; 3]>,
    kinematic: BTreeMap<EntityId, [f32; 3]>,
    kinematic_spin: BTreeMap<EntityId, [f32; 3]>,
}

impl Default for AuthoredWorld {
    fn default() -> Self {
        Self::new(super::authored_backend())
    }
}

impl AuthoredWorld {
    /// Opens an authored world with the selected solver, built on first step.
    pub(crate) fn new(backend: Backend) -> Self {
        Self {
            backend,
            world: None,
            handles: BTreeMap::new(),
            contacts: BTreeSet::new(),
            transitions: Vec::new(),
            joints: Vec::new(),
            joint_handles: Vec::new(),
            pending: BTreeMap::new(),
            pending_angular: BTreeMap::new(),
            kinematic: BTreeMap::new(),
            kinematic_spin: BTreeMap::new(),
        }
    }

    fn create_solver(backend: Backend) -> Result<Solver> {
        match backend {
            Backend::Rapier => {
                let mut world = PhysicsWorld::default();
                world.integration_parameters.dt = super::STEP;
                Ok(Solver::Rapier(Box::new(world)))
            }
            Backend::Jolt => {
                #[cfg(feature = "physics-jolt")]
                {
                    let mut world = oxijolt::PhysicsWorld::new(oxijolt::WorldSettings::default())?;
                    world.set_event_settings(JoltEventSettings::default().contacts(true));
                    Ok(Solver::Jolt(Box::new(world)))
                }
                #[cfg(not(feature = "physics-jolt"))]
                bail!("Jolt requires the physics-jolt cargo feature")
            }
        }
    }

    /// Queues a script velocity for a dynamic or kinematic cuboid.
    ///
    /// The authored world is built lazily on the first step, so a velocity set
    /// from a scene-start or fixed-update callback is applied when the body is
    /// created rather than rejected. Kinematic bodies integrate the velocity
    /// into their authored transform; fixed bodies ignore it.
    pub(crate) fn set_velocity(&mut self, node: EntityId, velocity: [f32; 3]) -> Result<()> {
        ensure!(
            velocity.iter().all(|value| value.is_finite()),
            "3D velocity must be finite"
        );
        self.pending.insert(node, velocity);
        Ok(())
    }

    /// Queues angular velocity in degrees per second for a dynamic or kinematic body.
    pub(crate) fn set_angular_velocity(
        &mut self,
        node: EntityId,
        velocity: [f32; 3],
    ) -> Result<()> {
        ensure!(
            velocity.iter().all(|value| value.is_finite()),
            "3D angular velocity must be finite"
        );
        self.pending_angular.insert(node, velocity);
        Ok(())
    }

    /// Replaces the authored constraints applied on the next step.
    pub(crate) fn set_joints(&mut self, joints: Vec<Joint>) {
        self.joints = joints;
    }

    /// Borrows the Rapier solver for host-side queries, or reports that Jolt
    /// does not implement them yet.
    fn rapier_world(&self, query: &str) -> Result<Option<&PhysicsWorld>> {
        #[cfg(not(feature = "physics-jolt"))]
        let _ = query;
        #[cfg(feature = "physics-jolt")]
        if matches!(&self.world, Some(Solver::Jolt(_))) {
            bail!("3D {query} is not supported by the Jolt backend");
        }
        if let Some(Solver::Rapier(world)) = &self.world {
            Ok(Some(world))
        } else {
            Ok(None)
        }
    }

    /// Casts a world-space ray against the authored colliders.
    pub(crate) fn raycast(
        &self,
        origin: [f32; 3],
        direction: [f32; 3],
        max_distance: f32,
    ) -> Result<Option<RayHit3D>> {
        let Some(world) = self.rapier_world("raycast")? else {
            return Ok(None);
        };
        let direction = Vec3::from_array(direction);
        ensure!(
            origin.iter().all(|v| v.is_finite())
                && direction.to_array().iter().all(|v| v.is_finite())
                && direction.length_squared() > 1e-8
                && max_distance.is_finite()
                && max_distance > 0.0,
            "Invalid raycast parameters"
        );
        let direction = direction.normalize();
        let ray = Ray::new(Vec3::from_array(origin), direction);
        let Some((collider, hit)) =
            world.cast_ray_and_get_normal(&ray, max_distance, true, QueryFilter::default())
        else {
            return Ok(None);
        };
        let Some(node) = self
            .handles
            .iter()
            .find(|(_, body)| matches!(body.handle, BodyHandle::Rapier { collider: h, .. } if h == collider))
            .map(|(&node, _)| node)
        else {
            return Ok(None);
        };
        Ok(Some(RayHit3D {
            node,
            distance: hit.time_of_impact,
            normal: hit.normal.to_array(),
        }))
    }

    /// Returns authored nodes whose colliders intersect the query shape.
    pub(crate) fn overlaps_shape(
        &self,
        shape: QueryShape3D,
        position: [f32; 3],
    ) -> Result<Vec<EntityId>> {
        let Some(world) = self.rapier_world("overlap queries")? else {
            return Ok(Vec::new());
        };
        ensure!(
            position.iter().all(|v| v.is_finite()),
            "Invalid overlap position"
        );
        let collider = shape.rapier().build();
        let pose = Pose::translation(position[0], position[1], position[2]);
        let mut hits: Vec<EntityId> = world
            .intersect_shape(pose, collider.shape(), QueryFilter::default())
            .filter_map(|(handle, _)| {
                self.handles.iter().find(|(_, body)| {
                    matches!(body.handle, BodyHandle::Rapier { collider: h, .. } if h == handle)
                })
                .map(|(&node, _)| node)
            })
            .collect();
        hits.sort_unstable();
        hits.dedup();
        Ok(hits)
    }

    /// Sweeps a query shape and reports the first authored body it touches.
    pub(crate) fn shape_cast(
        &self,
        shape: QueryShape3D,
        position: [f32; 3],
        direction: [f32; 3],
        max_distance: f32,
    ) -> Result<Option<RayHit3D>> {
        let Some(world) = self.rapier_world("shape casts")? else {
            return Ok(None);
        };
        let direction = Vec3::from_array(direction);
        ensure!(
            position.iter().all(|v| v.is_finite())
                && direction.to_array().iter().all(|v| v.is_finite())
                && direction.length_squared() > 1e-8
                && max_distance.is_finite()
                && max_distance > 0.0,
            "Invalid shape cast parameters"
        );
        let velocity = direction.normalize() * max_distance;
        let collider = shape.rapier().build();
        let pose = Pose::translation(position[0], position[1], position[2]);
        let options = rapier3d::parry::query::ShapeCastOptions::with_max_time_of_impact(1.0);
        let Some((handle, hit)) = world.cast_shape(
            &pose,
            velocity,
            collider.shape(),
            options,
            QueryFilter::default(),
        ) else {
            return Ok(None);
        };
        let Some(node) = self
            .handles
            .iter()
            .find(|(_, body)| matches!(body.handle, BodyHandle::Rapier { collider: h, .. } if h == handle))
            .map(|(&node, _)| node)
        else {
            return Ok(None);
        };
        Ok(Some(RayHit3D {
            node,
            distance: hit.time_of_impact * max_distance,
            normal: hit.normal1.to_array(),
        }))
    }

    /// Contact and trigger transitions produced by the most recent step.
    pub(crate) fn contacts(&self) -> &[Contact] {
        &self.transitions
    }

    fn rapier_collider(
        shape: Collider,
        scale: Vec3,
        geometry: Option<&Geometry>,
        trigger: bool,
        friction: f32,
        restitution: f32,
    ) -> Result<ColliderBuilder> {
        let builder = match shape {
            Collider::Cuboid => {
                let half = scale * 0.5;
                ColliderBuilder::cuboid(half.x, half.y, half.z)
            }
            Collider::Sphere => ColliderBuilder::ball(scale.max_element() * 0.5),
            Collider::Capsule => {
                let radius = scale.x.min(scale.z) * 0.5;
                let half_height = (scale.y * 0.5 - radius).max(0.0);
                ColliderBuilder::capsule_y(half_height, radius)
            }
            Collider::Mesh | Collider::ConvexHull => {
                let geometry = geometry.context("Mesh collider needs geometry")?;
                let points: Vec<Vec3> = geometry
                    .positions
                    .iter()
                    .map(|point| Vec3::from_array(*point) * scale)
                    .collect();
                if shape == Collider::ConvexHull {
                    ColliderBuilder::convex_hull(&points)
                        .context("Convex hull needs non-degenerate geometry")?
                } else {
                    let indices: Vec<[u32; 3]> = geometry
                        .indices
                        .as_chunks::<3>()
                        .0
                        .iter()
                        .map(|chunk| [chunk[0], chunk[1], chunk[2]])
                        .collect();
                    ColliderBuilder::trimesh(points, indices)
                        .context("Invalid triangle mesh collider")?
                }
            }
        };
        Ok(builder
            .sensor(trigger)
            .active_collision_types(ActiveCollisionTypes::all())
            .friction(friction)
            .restitution(restitution))
    }

    /// Builds a Jolt shape and body settings for one authored collider.
    ///
    /// Jolt meshes and convex hulls have no volume, so only fixed bodies may
    /// use them, and they cannot be sensors. Other shapes support every body
    /// kind; friction and restitution are per body in Jolt.
    #[cfg(feature = "physics-jolt")]
    fn jolt_body(desc: JoltBodyDesc<'_>) -> Result<(JoltShape, JoltBodySettings)> {
        let JoltBodyDesc {
            shape,
            scale,
            geometry,
            trigger,
            friction,
            restitution,
            kind,
            position,
            rotation,
        } = desc;
        let collider = match shape {
            Collider::Cuboid => JoltShape::new_box(jolt_vec(scale * 0.5))?,
            Collider::Sphere => JoltShape::new_sphere(scale.max_element() * 0.5)?,
            Collider::Capsule => {
                let radius = scale.x.min(scale.z) * 0.5;
                let half_height = (scale.y * 0.5 - radius).max(0.0);
                JoltShape::new_capsule(half_height, radius)?
            }
            Collider::Mesh | Collider::ConvexHull => {
                let geometry = geometry.context("Mesh collider needs geometry")?;
                ensure!(
                    kind == Body::Fixed,
                    "Jolt supports authored mesh and convex colliders only on fixed bodies"
                );
                ensure!(
                    !trigger,
                    "Jolt does not support mesh or convex trigger colliders"
                );
                let points: Vec<JoltVec> = geometry
                    .positions
                    .iter()
                    .map(|point| jolt_vec(Vec3::from_array(*point) * scale))
                    .collect();
                if shape == Collider::ConvexHull {
                    JoltShape::new_convex_hull(&points, 0.0)?
                } else {
                    let indices: Vec<[u32; 3]> = geometry
                        .indices
                        .as_chunks::<3>()
                        .0
                        .iter()
                        .map(|chunk| [chunk[0], chunk[1], chunk[2]])
                        .collect();
                    JoltShape::new_mesh(&points, &indices)
                        .context("Invalid triangle mesh collider")?
                        .0
                }
            }
        };
        let settings = match kind {
            Body::Dynamic => JoltBodySettings::new_dynamic(),
            Body::Kinematic => JoltBodySettings::new_kinematic(),
            Body::Fixed | Body::None => JoltBodySettings::new_static(),
        }
        .position(jolt_rvec(position))
        .rotation(jolt_quat(rotation))
        .friction(friction)
        .restitution(restitution)
        .sensor(trigger);
        Ok((collider, settings))
    }

    /// Normalizes one step's contact events into node pairs in node order.
    #[cfg(feature = "physics-jolt")]
    fn jolt_contacts(
        events: JoltWorldEvents,
        handles: &BTreeMap<EntityId, AuthoredBody>,
    ) -> BTreeSet<(EntityId, EntityId)> {
        let owners: HashMap<JoltBodyId, EntityId> = handles
            .iter()
            .filter_map(|(&node, body)| match body.handle {
                BodyHandle::Jolt(handle) => Some((handle, node)),
                BodyHandle::Rapier { .. } => None,
            })
            .collect();
        let mut current = BTreeSet::new();
        for event in events.contacts {
            // Jolt reports Added once and Persisted every later step. Removed
            // pairs are absent from this set, which is how Rapier's pair lists
            // are read too.
            let pair = match event {
                ContactEvent::Added { manifold, .. } | ContactEvent::Persisted { manifold, .. } => {
                    manifold.pair
                }
                ContactEvent::Removed(_) => continue,
            };
            let (Some(&first), Some(&second)) = (owners.get(&pair.body1), owners.get(&pair.body2))
            else {
                continue;
            };
            current.insert((first.min(second), first.max(second)));
        }
        current
    }

    /// Normalizes Rapier's contact and intersection pairs into node pairs.
    fn rapier_contacts(
        world: &PhysicsWorld,
        handles: &BTreeMap<EntityId, AuthoredBody>,
    ) -> BTreeSet<(EntityId, EntityId)> {
        let owners: HashMap<ColliderHandle, EntityId> = handles
            .iter()
            .filter_map(|(&node, body)| {
                body.handle
                    .rapier_collider()
                    .map(|collider| (collider, node))
            })
            .collect();
        let mut current = BTreeSet::new();
        let mut observe = |first: ColliderHandle, second: ColliderHandle, enabled: bool| {
            if !enabled {
                return;
            }
            let (Some(&first_node), Some(&second_node)) = (owners.get(&first), owners.get(&second))
            else {
                return;
            };
            current.insert((first_node.min(second_node), first_node.max(second_node)));
        };
        for pair in world.contact_pairs() {
            observe(
                pair.collider1,
                pair.collider2,
                pair.has_any_active_contact(),
            );
        }
        for (first, _, second, _, touching) in world.intersection_pairs() {
            observe(first, second, touching);
        }
        current
    }

    pub(crate) fn step(&mut self, scene: &mut Scene) -> Result<()> {
        self.transitions.clear();
        if self.world.is_none() && !scene.has_spatial_bodies() {
            return Ok(());
        }
        let mut nodes = scene.snapshot()?;
        // Kinematic bodies take their pose from the authored scene, so a queued
        // script velocity advances that pose before the solver reads it.
        for (&node, &velocity) in &self.pending {
            if nodes.iter().any(|state| {
                state.id == node
                    && state
                        .spatial
                        .as_ref()
                        .is_some_and(|s| s.body == Body::Kinematic)
            }) {
                self.kinematic.insert(node, velocity);
            }
        }
        for (&node, &spin) in &self.pending_angular {
            if nodes.iter().any(|state| {
                state.id == node
                    && state
                        .spatial
                        .as_ref()
                        .is_some_and(|s| s.body == Body::Kinematic)
            }) {
                self.kinematic_spin.insert(node, spin);
            }
        }
        let is_kinematic = |node: EntityId| {
            nodes.iter().any(|state| {
                state.id == node
                    && state
                        .spatial
                        .as_ref()
                        .is_some_and(|s| s.body == Body::Kinematic)
            })
        };
        self.kinematic.retain(|node, _| is_kinematic(*node));
        self.kinematic_spin.retain(|node, _| is_kinematic(*node));
        for node in &mut nodes {
            let Some(velocity) = self.kinematic.get(&node.id) else {
                continue;
            };
            let Some(mut spatial) = node.spatial.clone() else {
                continue;
            };
            for (value, delta) in spatial.position.iter_mut().zip(velocity) {
                *value += delta * super::STEP;
            }
            if let Some(spin) = self.kinematic_spin.get(&node.id) {
                for (value, delta) in spatial.rotation.iter_mut().zip(spin) {
                    *value += delta * super::STEP;
                }
            }
            scene.set_spatial(node.id, Some(spatial.clone()))?;
            node.spatial = Some(spatial);
        }
        let mut worlds = matrices(&nodes);
        if self.world.is_none() {
            self.world = Some(Self::create_solver(self.backend)?);
        }
        let world = self.world.as_mut().context("3D solver")?;
        let mut active = BTreeSet::new();
        for node in &nodes {
            let Some(s) = node
                .spatial
                .as_ref()
                .filter(|s| s.body != Body::None && node.visible)
            else {
                continue;
            };
            let matrix = worlds[&node.id];
            let (scale, rotation, position) = matrix.to_scale_rotation_translation();
            ensure!(
                scale.is_finite()
                    && position.is_finite()
                    && rotation.is_normalized()
                    && Mat4::from_scale_rotation_translation(scale, rotation, position)
                        .abs_diff_eq(matrix, 0.001),
                "Physics node {} has a sheared or invalid world transform",
                node.name
            );
            let pose = Pose::from_parts(position, rotation);
            active.insert(node.id);
            if let Some(body) = self.handles.get(&node.id).copied() {
                let unchanged = body.kind == s.body
                    && body.shape == s.collider
                    && body.trigger == s.trigger
                    && (body.friction - s.friction).abs() < 0.0001
                    && (body.restitution - s.restitution).abs() < 0.0001
                    && body.scale.abs_diff_eq(scale, 0.0001)
                    && body.geometry
                        == s.geometry
                            .as_ref()
                            .map(|geometry| Arc::as_ptr(geometry) as usize);
                if unchanged {
                    if matches!(s.body, Body::Fixed | Body::Kinematic) {
                        match (&mut *world, body.handle) {
                            (Solver::Rapier(world), BodyHandle::Rapier { body: handle, .. }) => {
                                world.bodies[handle].set_position(pose, true);
                            }
                            #[cfg(feature = "physics-jolt")]
                            (Solver::Jolt(world), BodyHandle::Jolt(handle))
                                if s.body == Body::Kinematic =>
                            {
                                world.body_mut(handle)?.move_kinematic(
                                    jolt_rvec(position),
                                    jolt_quat(rotation),
                                    super::STEP,
                                )?;
                            }
                            #[cfg(feature = "physics-jolt")]
                            (Solver::Jolt(world), BodyHandle::Jolt(handle)) => {
                                world.body_mut(handle)?.set_position_and_rotation(
                                    jolt_rvec(position),
                                    jolt_quat(rotation),
                                    Activation::DontActivate,
                                )?;
                            }
                            #[cfg(feature = "physics-jolt")]
                            _ => {}
                        }
                    }
                    continue;
                }
                match (&mut *world, body.handle) {
                    (Solver::Rapier(world), BodyHandle::Rapier { body: handle, .. }) => {
                        world.remove_body(handle);
                    }
                    #[cfg(feature = "physics-jolt")]
                    (Solver::Jolt(world), BodyHandle::Jolt(handle)) => {
                        world.remove_body(handle)?;
                    }
                    #[cfg(feature = "physics-jolt")]
                    _ => bail!("3D body handle does not match its solver"),
                }
            }
            let handle = match &mut *world {
                Solver::Rapier(world) => {
                    let builder = match s.body {
                        Body::Dynamic => RigidBodyBuilder::dynamic(),
                        Body::Kinematic => RigidBodyBuilder::kinematic_position_based(),
                        Body::Fixed | Body::None => RigidBodyBuilder::fixed(),
                    };
                    let collider = Self::rapier_collider(
                        s.collider,
                        scale,
                        s.geometry.as_deref(),
                        s.trigger,
                        s.friction,
                        s.restitution,
                    )?;
                    let (body, collider) = world.insert(builder.pose(pose), collider);
                    BodyHandle::Rapier { body, collider }
                }
                #[cfg(feature = "physics-jolt")]
                Solver::Jolt(jolt) => {
                    let (shape, settings) = Self::jolt_body(JoltBodyDesc {
                        shape: s.collider,
                        scale,
                        geometry: s.geometry.as_deref(),
                        trigger: s.trigger,
                        friction: s.friction,
                        restitution: s.restitution,
                        kind: s.body,
                        position,
                        rotation,
                    })?;
                    BodyHandle::Jolt(jolt.create_body(&shape, &settings)?)
                }
            };
            self.handles.insert(
                node.id,
                AuthoredBody {
                    handle,
                    kind: s.body,
                    shape: s.collider,
                    trigger: s.trigger,
                    friction: s.friction,
                    restitution: s.restitution,
                    scale,
                    geometry: s
                        .geometry
                        .as_ref()
                        .map(|geometry| Arc::as_ptr(geometry) as usize),
                },
            );
        }
        let stale: Vec<_> = self
            .handles
            .iter()
            .filter(|(id, _)| !active.contains(id))
            .map(|(&id, body)| (id, body.handle))
            .collect();
        for (id, handle) in stale {
            match (&mut *world, handle) {
                (Solver::Rapier(world), BodyHandle::Rapier { body: handle, .. }) => {
                    world.remove_body(handle);
                }
                #[cfg(feature = "physics-jolt")]
                (Solver::Jolt(world), BodyHandle::Jolt(handle)) => {
                    world.remove_body(handle)?;
                }
                #[cfg(feature = "physics-jolt")]
                _ => bail!("3D body handle does not match its solver"),
            }
            self.handles.remove(&id);
        }
        match &mut *world {
            Solver::Rapier(world) => {
                for handle in self.joint_handles.drain(..) {
                    world.impulse_joints.remove(handle, true);
                }
                for joint in &self.joints {
                    let (Some(first), Some(second)) =
                        (self.handles.get(&joint.a), self.handles.get(&joint.b))
                    else {
                        continue;
                    };
                    let (Some(first_body), Some(second_body)) =
                        (first.handle.rapier_body(), second.handle.rapier_body())
                    else {
                        bail!("3D joint body does not belong to its solver");
                    };
                    let anchor_a = Vec3::from_array(joint.anchor_a);
                    let anchor_b = Vec3::from_array(joint.anchor_b);
                    let data: rapier3d::prelude::GenericJoint = match joint.kind {
                        JointKind::Fixed => FixedJointBuilder::new()
                            .local_anchor1(anchor_a)
                            .local_anchor2(anchor_b)
                            .build()
                            .into(),
                        JointKind::Ball => SphericalJointBuilder::new()
                            .local_anchor1(anchor_a)
                            .local_anchor2(anchor_b)
                            .build()
                            .into(),
                        JointKind::Hinge => RevoluteJointBuilder::new(Vec3::from_array(joint.axis))
                            .local_anchor1(anchor_a)
                            .local_anchor2(anchor_b)
                            .build()
                            .into(),
                        JointKind::Slider => {
                            PrismaticJointBuilder::new(Vec3::from_array(joint.axis))
                                .local_anchor1(anchor_a)
                                .local_anchor2(anchor_b)
                                .build()
                                .into()
                        }
                    };
                    let handle = world
                        .impulse_joints
                        .insert(first_body, second_body, data, true);
                    self.joint_handles.push(handle);
                }
            }
            #[cfg(feature = "physics-jolt")]
            Solver::Jolt(_) => ensure!(
                self.joints.is_empty(),
                "Jolt backend does not support authored 3D joints yet"
            ),
        }
        for (&node, &velocity) in &self.pending {
            let Some(body) = self.handles.get(&node).copied() else {
                continue;
            };
            if body.kind != Body::Dynamic {
                continue;
            }
            match (&mut *world, body.handle) {
                (Solver::Rapier(world), BodyHandle::Rapier { body: handle, .. }) => {
                    world.bodies[handle].set_linvel(Vec3::from_array(velocity), true);
                }
                #[cfg(feature = "physics-jolt")]
                (Solver::Jolt(world), BodyHandle::Jolt(handle)) => {
                    world
                        .body_mut(handle)?
                        .set_linear_velocity(jolt_vec(Vec3::from_array(velocity)))?;
                }
                #[cfg(feature = "physics-jolt")]
                _ => bail!("3D body handle does not match its solver"),
            }
        }
        for (&node, &spin) in &self.pending_angular {
            let Some(body) = self.handles.get(&node).copied() else {
                continue;
            };
            if body.kind != Body::Dynamic {
                continue;
            }
            let radians = spin.map(f32::to_radians);
            match (&mut *world, body.handle) {
                (Solver::Rapier(world), BodyHandle::Rapier { body: handle, .. }) => {
                    world.bodies[handle].set_angvel(Vec3::from_array(radians), true);
                }
                #[cfg(feature = "physics-jolt")]
                (Solver::Jolt(world), BodyHandle::Jolt(handle)) => {
                    world
                        .body_mut(handle)?
                        .set_angular_velocity(jolt_vec(Vec3::from_array(radians)))?;
                }
                #[cfg(feature = "physics-jolt")]
                _ => bail!("3D body handle does not match its solver"),
            }
        }
        self.pending.clear();
        self.pending_angular.clear();
        let current = match &mut *world {
            Solver::Rapier(world) => {
                world.step();
                Self::rapier_contacts(world, &self.handles)
            }
            #[cfg(feature = "physics-jolt")]
            Solver::Jolt(jolt) => {
                let report = jolt.step(super::STEP)?;
                ensure!(
                    report.is_complete(),
                    "Jolt dropped contacts during step: {report:?}"
                );
                Self::jolt_contacts(jolt.take_events(), &self.handles)
            }
        };
        let is_trigger = |edge: &(EntityId, EntityId)| {
            self.handles.get(&edge.0).is_some_and(|body| body.trigger)
                || self.handles.get(&edge.1).is_some_and(|body| body.trigger)
        };
        // Match the 2D solver's exit-before-entry ordering.
        for edge in &self.contacts {
            if !current.contains(edge) {
                self.transitions.push(Contact {
                    nodes: [edge.0, edge.1],
                    entered: false,
                    trigger: is_trigger(edge),
                });
            }
        }
        for edge in &current {
            if !self.contacts.contains(edge) {
                self.transitions.push(Contact {
                    nodes: [edge.0, edge.1],
                    entered: true,
                    trigger: is_trigger(edge),
                });
            }
        }
        self.contacts = current;
        // Recompose in tree order so a dynamic child uses its parent's new pose.
        for node in nodes {
            let parent = node
                .parent
                .and_then(|id| worlds.get(&id).copied())
                .unwrap_or(Mat4::IDENTITY);
            let Some(mut s) = node.spatial else {
                worlds.insert(node.id, parent);
                continue;
            };
            if let Some(body) = self.handles.get(&node.id)
                && body.kind == Body::Dynamic
            {
                let (body_rotation, body_position) = match (&mut *world, body.handle) {
                    (Solver::Rapier(world), BodyHandle::Rapier { body: handle, .. }) => {
                        let state = &world.bodies[handle];
                        (*state.rotation(), state.translation())
                    }
                    #[cfg(feature = "physics-jolt")]
                    (Solver::Jolt(jolt), BodyHandle::Jolt(handle)) => {
                        let state = jolt.body(handle)?;
                        (
                            jolt_rotation(state.rotation()),
                            jolt_position(state.position()),
                        )
                    }
                    #[cfg(feature = "physics-jolt")]
                    _ => bail!("3D body handle does not match its solver"),
                };
                let local = parent.inverse()
                    * Mat4::from_scale_rotation_translation(
                        body.scale,
                        body_rotation,
                        body_position,
                    );
                let (_, rotation, position) = local.to_scale_rotation_translation();
                ensure!(
                    rotation.is_normalized(),
                    "Dynamic body {} requires an unsheared parent transform",
                    node.name
                );
                s.position = position.to_array();
                let (x, y, z) = rotation.to_euler(EulerRot::XYZ);
                s.rotation = [x, y, z].map(f32::to_degrees);
                scene.set_spatial(node.id, Some(s.clone()))?;
            }
            worlds.insert(node.id, parent * s.matrix());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{director::Director, save::SceneFile};
    use anyhow::Context;

    #[test]
    fn script_velocity_moves_an_authored_dynamic_cuboid() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let path = tmp.path().join("velocity.scene");
        std::fs::write(
            &path,
            r#"{"version":1,"nodes":[
          {"name":"Floor","spatial":{"role":"cube","body":"fixed","scale":[8,1,8]}},
          {"name":"Block","spatial":{"role":"cube","body":"dynamic","position":[0,1,0],"scale":[1,1,1]}}
        ]}"#,
        )?;
        let mut director = Director::new(vec![path], None)?;
        let block = director.script_node(1)?;
        director.set_linear_velocity_3d(block, [2.0, 0.0, 0.0])?;
        for _ in 0..30 {
            director.step()?;
        }
        let nodes = director.scene().snapshot()?;
        let moved = nodes[1]
            .spatial
            .as_ref()
            .is_some_and(|spatial| spatial.position[0] > 0.1);
        assert!(moved, "script velocity did not move the dynamic cuboid");
        Ok(())
    }

    #[test]
    fn authored_cuboids_report_enter_and_exit_contacts() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let path = tmp.path().join("contacts.scene");
        std::fs::write(
            &path,
            r#"{"version":1,"nodes":[
          {"name":"Floor","spatial":{"role":"cube","body":"fixed","scale":[8,1,8]}},
          {"name":"Block","spatial":{"role":"cube","body":"dynamic","position":[0,3,0],"scale":[1,1,1]}}
        ]}"#,
        )?;
        let mut director = Director::new(vec![path], None)?;
        let mut entered = false;
        for _ in 0..180 {
            director.step()?;
            if director
                .physics3d_contacts()
                .iter()
                .any(|contact| contact.entered)
            {
                entered = true;
            }
        }
        assert!(entered, "landing cuboids did not report a contact");
        Ok(())
    }

    #[test]
    fn kinematic_bodies_follow_the_scene_and_script_velocity() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let path = tmp.path().join("kinematic.scene");
        std::fs::write(
            &path,
            r#"{"version":1,"nodes":[
          {"name":"Mover","spatial":{"role":"cube","body":"kinematic","position":[0,2,0],"scale":[1,1,1]}}
        ]}"#,
        )?;
        let mut director = Director::new(vec![path], None)?;
        let mover = director.script_node(0)?;
        director.set_linear_velocity_3d(mover, [1.0, 0.0, 0.0])?;
        for _ in 0..30 {
            director.step()?;
        }
        let nodes = director.scene().snapshot()?;
        assert!(
            nodes[0]
                .spatial
                .as_ref()
                .is_some_and(|s| s.position[0] > 0.1)
        );
        Ok(())
    }

    #[test]
    fn trigger_nodes_report_overlaps() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let path = tmp.path().join("trigger.scene");
        std::fs::write(
            &path,
            r#"{"version":1,"nodes":[
          {"name":"Zone","spatial":{"role":"cube","body":"fixed","trigger":true,"position":[0,1,0],"scale":[4,2,4]}},
          {"name":"Block","spatial":{"role":"cube","body":"dynamic","position":[0,4,0],"scale":[1,1,1]}}
        ]}"#,
        )?;
        let mut director = Director::new(vec![path], None)?;
        let mut seen = false;
        for _ in 0..180 {
            director.step()?;
            if director
                .physics3d_contacts()
                .iter()
                .any(|contact| contact.entered && contact.trigger)
            {
                seen = true;
            }
        }
        assert!(seen, "trigger overlap was not reported");
        Ok(())
    }

    #[test]
    fn sphere_and_capsule_colliders_land_on_a_cuboid_floor() -> Result<()> {
        for shape in ["sphere", "capsule"] {
            let tmp = tempfile::tempdir()?;
            let path = tmp.path().join(format!("{shape}.scene"));
            std::fs::write(
                &path,
                format!(
                    r#"{{"version":1,"nodes":[
                  {{"name":"Floor","spatial":{{"role":"cube","body":"fixed","scale":[8,1,8]}}}},
                  {{"name":"Ball","spatial":{{"role":"cube","body":"dynamic","collider":"{shape}","position":[0,3,0],"scale":[1,2,1]}}}}
                ]}}"#
                ),
            )?;
            let mut director = Director::new(vec![path], None)?;
            for _ in 0..180 {
                director.step()?;
            }
            let nodes = director.scene().snapshot()?;
            assert!(
                nodes[1]
                    .spatial
                    .as_ref()
                    .is_some_and(|s| s.position[1] < 2.0 && s.position[1].is_finite()),
                "{shape} collider did not settle"
            );
        }
        Ok(())
    }

    #[test]
    fn overlap_and_shape_cast_find_authored_bodies() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let path = tmp.path().join("query.scene");
        std::fs::write(
            &path,
            r#"{"version":1,"nodes":[
          {"name":"Floor","spatial":{"role":"cube","body":"fixed","position":[0,0,0],"scale":[8,1,8]}}
        ]}"#,
        )?;
        let mut director = Director::new(vec![path], None)?;
        director.step()?;
        let overlaps = director.physics3d_overlaps([0.1, 0.1, 0.1], [0.0, 0.0, 0.0])?;
        assert!(overlaps.contains(&director.script_node(0)?));
        let hit = director.physics3d_shape_cast(
            [0.2, 0.2, 0.2],
            [0.0, 4.0, 0.0],
            [0.0, -1.0, 0.0],
            10.0,
        )?;
        assert!(hit.is_some_and(|hit| hit.distance > 2.0));
        Ok(())
    }

    #[test]
    fn fixed_joint_keeps_two_bodies_together() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let path = tmp.path().join("joint.scene");
        std::fs::write(
            &path,
            r#"{"version":1,"nodes":[
          {"name":"Floor","spatial":{"role":"cube","body":"fixed","scale":[8,1,8]}},
          {"name":"First","spatial":{"role":"cube","body":"dynamic","position":[0,3,0],"scale":[1,1,1]}},
          {"name":"Second","spatial":{"role":"cube","body":"dynamic","position":[0,4,0],"scale":[1,1,1]}}
        ],"joints":[{"a":1,"b":2,"kind":"fixed"}]}"#,
        )?;
        let mut director = Director::new(vec![path], None)?;
        for _ in 0..180 {
            director.step()?;
        }
        let nodes = director.scene().snapshot()?;
        let first = nodes[1].spatial.as_ref().context("first body")?.position;
        let second = nodes[2].spatial.as_ref().context("second body")?.position;
        let distance = Vec3::from_array(first).distance(Vec3::from_array(second));
        assert!(
            distance < 2.0,
            "joint did not keep the bodies together: {distance}"
        );
        Ok(())
    }

    #[test]
    fn raycast_reports_the_hit_node_and_distance() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let path = tmp.path().join("ray.scene");
        std::fs::write(
            &path,
            r#"{"version":1,"nodes":[
          {"name":"Floor","spatial":{"role":"cube","body":"fixed","position":[0,0,0],"scale":[8,1,8]}}
        ]}"#,
        )?;
        let mut director = Director::new(vec![path], None)?;
        director.step()?;
        let hit = director
            .physics3d_raycast([0.0, 4.0, 0.0], [0.0, -1.0, 0.0], 10.0)?
            .context("ray did not hit the floor")?;
        assert_eq!(hit.node, director.script_node(0)?);
        assert!((hit.distance - 3.5).abs() < 0.05);
        assert!(
            director
                .physics3d_raycast([0.0, 4.0, 0.0], [1.0, 0.0, 0.0], 10.0)?
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn script_angular_velocity_spins_a_dynamic_cuboid() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let path = tmp.path().join("spin.scene");
        std::fs::write(
            &path,
            r#"{"version":1,"nodes":[
          {"name":"Floor","spatial":{"role":"cube","body":"fixed","scale":[8,1,8]}},
          {"name":"Top","spatial":{"role":"cube","body":"dynamic","position":[0,1,0],"scale":[1,1,1]}}
        ]}"#,
        )?;
        let mut director = Director::new(vec![path], None)?;
        let top = director.script_node(1)?;
        director.set_angular_velocity_3d(top, [0.0, 360.0, 0.0])?;
        for _ in 0..15 {
            director.step()?;
        }
        let nodes = director.scene().snapshot()?;
        assert!(
            nodes[1]
                .spatial
                .as_ref()
                .is_some_and(|s| s.rotation[1].abs() > 1.0),
            "angular velocity did not spin the body"
        );
        Ok(())
    }

    #[test]
    fn authored_mixed_scene_lands_and_restarts_with_its_parent_relative_pose() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let path = tmp.path().join("mixed.scene");
        std::fs::write(
            &path,
            r#"{"version":1,"nodes":[
          {"name":"Pivot","spatial":{"position":[1,2,3]}},
          {"name":"Floor","parent":0,"spatial":{"role":"cube","body":"fixed","scale":[6,1,6]}},
          {"name":"Block","parent":0,"spatial":{"role":"cube","body":"dynamic","position":[0,4,0]}},
          {"name":"HUD","transform":{"position":[15,20],"rotation":0,"scale":[1,1]}}
        ]}"#,
        )?;
        let mut director = Director::new(vec![path.clone()], None)?;
        for _ in 0..180 {
            director.step()?;
        }
        let nodes = director.scene().snapshot()?;
        assert!(
            nodes[2]
                .spatial
                .as_ref()
                .is_some_and(|spatial| (spatial.position[1] - 1.).abs() < 0.02)
        );
        assert_eq!(nodes[3].local.position, [15., 20.]);
        director.change_scene(0)?;
        let nodes = director.scene().snapshot()?;
        assert!(
            nodes[2]
                .spatial
                .as_ref()
                .is_some_and(|spatial| spatial.position[1] == 4.)
        );
        let captured = SceneFile::capture(director.scene(), |_| None)?;
        assert!(captured.nodes[2].spatial.is_some() && captured.nodes[3].spatial.is_none());
        Ok(())
    }

    /// The supported authored fixtures run through both solvers and compare
    /// their settled poses and event streams.
    #[cfg(feature = "physics-jolt")]
    mod parity {
        use super::*;
        use crate::{
            assets::Assets,
            save::{Loaded, SceneFile, parent_dir},
            scene::Scene,
        };
        use anyhow::Context;

        const FLOOR_BLOCK: &str = r#"{"version":1,"nodes":[
          {"name":"Floor","spatial":{"role":"cube","body":"fixed","scale":[8,1,8]}},
          {"name":"Block","spatial":{"role":"cube","body":"dynamic","position":[0,3,0],"scale":[1,1,1]}}
        ]}"#;

        const FLOOR_ROUNDS: &str = r#"{"version":1,"nodes":[
          {"name":"Floor","spatial":{"role":"cube","body":"fixed","scale":[8,1,8]}},
          {"name":"Sphere","spatial":{"role":"cube","body":"dynamic","collider":"sphere","position":[0,3,0],"scale":[1,2,1]}},
          {"name":"Capsule","spatial":{"role":"cube","body":"dynamic","collider":"capsule","position":[2,3,0],"scale":[1,2,1]}}
        ]}"#;

        const MOVER: &str = r#"{"version":1,"nodes":[
          {"name":"Mover","spatial":{"role":"cube","body":"kinematic","position":[0,2,0],"scale":[1,1,1]}}
        ]}"#;

        const TRIGGER: &str = r#"{"version":1,"nodes":[
          {"name":"Zone","spatial":{"role":"cube","body":"fixed","trigger":true,"position":[0,1,0],"scale":[4,2,4]}},
          {"name":"Block","spatial":{"role":"cube","body":"dynamic","position":[0,4,0],"scale":[1,1,1]}}
        ]}"#;

        fn fixture(json: &str, backend: Backend) -> Result<(Loaded, AuthoredWorld)> {
            let tmp = tempfile::tempdir()?;
            let path = tmp.path().join("parity.scene");
            std::fs::write(&path, json)?;
            let mut assets = Assets::new(None);
            let mut loaded = SceneFile::load(&path)?.instantiate(
                parent_dir(&path),
                &mut assets,
                Scene::default(),
            )?;
            loaded.scene.activate()?;
            let mut world = AuthoredWorld::new(backend);
            world.set_joints(loaded.joints.clone());
            Ok((loaded, world))
        }

        fn positions(json: &str, backend: Backend, ticks: usize) -> Result<Vec<[f32; 3]>> {
            let (mut loaded, mut world) = fixture(json, backend)?;
            for _ in 0..ticks {
                world.step(&mut loaded.scene)?;
            }
            Ok(loaded
                .scene
                .snapshot()?
                .into_iter()
                .map(|node| node.spatial.map_or([0.0; 3], |spatial| spatial.position))
                .collect())
        }

        fn assert_close(rapier: &[[f32; 3]], jolt: &[[f32; 3]], tolerance: f32) {
            assert_eq!(rapier.len(), jolt.len());
            for (first, second) in rapier.iter().zip(jolt) {
                for (a, b) in first.iter().zip(second) {
                    assert!(
                        (a - b).abs() <= tolerance,
                        "backend poses diverged: {rapier:?} vs {jolt:?}"
                    );
                }
            }
        }

        fn events(json: &str, backend: Backend, ticks: usize) -> Result<Vec<Contact>> {
            let (mut loaded, mut world) = fixture(json, backend)?;
            let mut events = Vec::new();
            for _ in 0..ticks {
                world.step(&mut loaded.scene)?;
                events.extend_from_slice(world.contacts());
            }
            Ok(events)
        }

        #[test]
        fn settled_cuboid_matches() -> Result<()> {
            let rapier = positions(FLOOR_BLOCK, Backend::Rapier, 180)?;
            let jolt = positions(FLOOR_BLOCK, Backend::Jolt, 180)?;
            assert_close(&rapier, &jolt, 0.05);
            Ok(())
        }

        #[test]
        fn settled_sphere_and_capsule_match() -> Result<()> {
            let rapier = positions(FLOOR_ROUNDS, Backend::Rapier, 180)?;
            let jolt = positions(FLOOR_ROUNDS, Backend::Jolt, 180)?;
            assert_close(&rapier, &jolt, 0.08);
            Ok(())
        }

        #[test]
        fn scripted_velocity_matches() -> Result<()> {
            let mut poses = Vec::new();
            for backend in [Backend::Rapier, Backend::Jolt] {
                let (mut loaded, mut world) = fixture(FLOOR_BLOCK, backend)?;
                let block = loaded.ids[1];
                world.set_velocity(block, [2.0, 0.0, 0.0])?;
                world.set_angular_velocity(block, [0.0, 360.0, 0.0])?;
                for _ in 0..15 {
                    world.step(&mut loaded.scene)?;
                }
                let node = loaded
                    .scene
                    .snapshot()?
                    .remove(1)
                    .spatial
                    .context("block")?;
                poses.push(node.position);
                assert!(
                    node.rotation[1].abs() > 1.0,
                    "angular velocity did not spin on {backend:?}"
                );
            }
            assert!((poses[0][0] - poses[1][0]).abs() < 0.1, "{poses:?}");
            Ok(())
        }

        #[test]
        fn kinematic_drive_matches() -> Result<()> {
            let mut poses = Vec::new();
            for backend in [Backend::Rapier, Backend::Jolt] {
                let (mut loaded, mut world) = fixture(MOVER, backend)?;
                world.set_velocity(loaded.ids[0], [1.0, 0.0, 0.0])?;
                for _ in 0..30 {
                    world.step(&mut loaded.scene)?;
                }
                poses.push(
                    loaded
                        .scene
                        .snapshot()?
                        .remove(0)
                        .spatial
                        .context("mover")?
                        .position,
                );
            }
            assert_close(&[poses[0]], &[poses[1]], 0.01);
            Ok(())
        }

        #[test]
        fn unsupported_jolt_operations_report_errors() -> Result<()> {
            let (mut loaded, mut world) = fixture(FLOOR_BLOCK, Backend::Jolt)?;
            world.set_joints(vec![Joint {
                a: loaded.ids[0],
                b: loaded.ids[1],
                kind: JointKind::Fixed,
                anchor_a: [0.0; 3],
                anchor_b: [0.0; 3],
                axis: [1.0, 0.0, 0.0],
            }]);
            let error = world.step(&mut loaded.scene).unwrap_err();
            assert!(error.to_string().contains("joints"), "{error:#}");

            let (mut loaded, mut world) = fixture(FLOOR_BLOCK, Backend::Jolt)?;
            world.step(&mut loaded.scene)?;
            assert!(
                world
                    .raycast([0.0, 4.0, 0.0], [0.0, -1.0, 0.0], 10.0)
                    .is_err()
            );
            assert!(
                world
                    .overlaps_shape(QueryShape3D::Box([0.1; 3]), [0.0, 0.0, 0.0])
                    .is_err()
            );
            assert!(
                world
                    .shape_cast(
                        QueryShape3D::Box([0.1; 3]),
                        [0.0, 4.0, 0.0],
                        [0.0, -1.0, 0.0],
                        10.0,
                    )
                    .is_err()
            );
            Ok(())
        }

        #[test]
        fn jolt_rejects_non_static_mesh_and_convex_shapes() -> Result<()> {
            let geometry = Geometry {
                positions: vec![
                    [0.0, 0.0, 0.0],
                    [1.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0],
                    [0.0, 0.0, 1.0],
                ],
                indices: vec![0, 1, 2, 0, 2, 3, 0, 3, 1, 1, 3, 2],
                uvs: None,
            };
            for shape in [Collider::Mesh, Collider::ConvexHull] {
                let desc = |kind| JoltBodyDesc {
                    shape,
                    scale: Vec3::ONE,
                    geometry: Some(&geometry),
                    trigger: false,
                    friction: 0.65,
                    restitution: 0.12,
                    kind,
                    position: Vec3::ZERO,
                    rotation: Quat::IDENTITY,
                };
                let error = match AuthoredWorld::jolt_body(desc(Body::Dynamic)) {
                    Err(error) => error,
                    Ok(_) => panic!("dynamic mesh collider must be rejected"),
                };
                assert!(error.to_string().contains("fixed"), "{error:#}");
                assert!(AuthoredWorld::jolt_body(desc(Body::Fixed)).is_ok());
            }
            Ok(())
        }

        #[test]
        fn contact_and_trigger_events_match() -> Result<()> {
            for json in [FLOOR_BLOCK, TRIGGER] {
                let rapier = events(json, Backend::Rapier, 180)?;
                let jolt = events(json, Backend::Jolt, 180)?;
                let rapier_pairs: BTreeSet<_> = rapier.iter().map(|event| event.nodes).collect();
                let jolt_pairs: BTreeSet<_> = jolt.iter().map(|event| event.nodes).collect();
                assert_eq!(rapier_pairs, jolt_pairs);
                for stream in [&rapier, &jolt] {
                    assert!(stream.iter().any(|event| event.entered));
                    assert_eq!(
                        stream.iter().any(|event| event.entered && event.trigger),
                        json == TRIGGER
                    );
                }
            }
            Ok(())
        }
    }
}
