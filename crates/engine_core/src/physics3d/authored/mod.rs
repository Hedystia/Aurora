//! Scene-local ownership of authored Rapier bodies and colliders.

use crate::{
    ecs::EntityId,
    scene::{
        Scene,
        spatial::{Body, Collider, matrices},
    },
};
use anyhow::{Result, ensure};
use glam::{EulerRot, Mat4, Vec3};
use rapier3d::prelude::{
    ActiveCollisionTypes, ColliderBuilder, ColliderHandle, PhysicsWorld, Pose, RigidBodyBuilder,
    RigidBodyHandle,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// A contact or trigger transition for one scene tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Contact {
    pub nodes: [EntityId; 2],
    pub entered: bool,
    /// True when at least one collider in the pair is a trigger.
    pub trigger: bool,
}

#[derive(Clone, Copy)]
struct AuthoredBody {
    handle: RigidBodyHandle,
    collider: ColliderHandle,
    kind: Body,
    shape: Collider,
    trigger: bool,
    friction: f32,
    restitution: f32,
    scale: Vec3,
}

#[derive(Default)]
pub(crate) struct AuthoredWorld {
    world: Option<PhysicsWorld>,
    handles: BTreeMap<EntityId, AuthoredBody>,
    contacts: BTreeSet<(EntityId, EntityId)>,
    transitions: Vec<Contact>,
    pending: BTreeMap<EntityId, [f32; 3]>,
    kinematic: BTreeMap<EntityId, [f32; 3]>,
}

impl AuthoredWorld {
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

    /// Contact and trigger transitions produced by the most recent step.
    pub(crate) fn contacts(&self) -> &[Contact] {
        &self.transitions
    }

    fn collider(
        shape: Collider,
        scale: Vec3,
        trigger: bool,
        friction: f32,
        restitution: f32,
    ) -> ColliderBuilder {
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
        };
        builder
            .sensor(trigger)
            .active_collision_types(ActiveCollisionTypes::all())
            .friction(friction)
            .restitution(restitution)
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
        self.kinematic.retain(|node, _| {
            nodes.iter().any(|state| {
                state.id == *node
                    && state
                        .spatial
                        .as_ref()
                        .is_some_and(|s| s.body == Body::Kinematic)
            })
        });
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
            scene.set_spatial(node.id, Some(spatial.clone()))?;
            node.spatial = Some(spatial);
        }
        let mut worlds = matrices(&nodes);
        let world = self.world.get_or_insert_with(|| {
            let mut w = PhysicsWorld::default();
            w.integration_parameters.dt = super::STEP;
            w
        });
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
                    && body.scale.abs_diff_eq(scale, 0.0001);
                if unchanged {
                    match s.body {
                        Body::Fixed | Body::Kinematic => {
                            world.bodies[body.handle].set_position(pose, true)
                        }
                        Body::Dynamic | Body::None => {}
                    }
                    continue;
                }
                world.remove_body(body.handle);
            }
            let builder = match s.body {
                Body::Dynamic => RigidBodyBuilder::dynamic(),
                Body::Kinematic => RigidBodyBuilder::kinematic_position_based(),
                Body::Fixed | Body::None => RigidBodyBuilder::fixed(),
            };
            let collider = Self::collider(s.collider, scale, s.trigger, s.friction, s.restitution);
            let (handle, collider_handle) = world.insert(builder.pose(pose), collider);
            self.handles.insert(
                node.id,
                AuthoredBody {
                    handle,
                    collider: collider_handle,
                    kind: s.body,
                    shape: s.collider,
                    trigger: s.trigger,
                    friction: s.friction,
                    restitution: s.restitution,
                    scale,
                },
            );
        }
        self.handles.retain(|id, body| {
            if active.contains(id) {
                return true;
            }
            world.remove_body(body.handle);
            false
        });
        for (&node, &velocity) in &self.pending {
            if let Some(body) = self.handles.get(&node)
                && body.kind == Body::Dynamic
            {
                world.bodies[body.handle].set_linvel(Vec3::from_array(velocity), true);
            }
        }
        self.pending.clear();
        world.step();
        let owners: HashMap<ColliderHandle, EntityId> = self
            .handles
            .iter()
            .map(|(&entity, body)| (body.collider, entity))
            .collect();
        let triggers: HashMap<ColliderHandle, bool> = self
            .handles
            .values()
            .map(|body| (body.collider, body.trigger))
            .collect();
        let mut current = BTreeSet::new();
        let mut is_trigger = HashMap::new();
        let mut observe = |first: ColliderHandle, second: ColliderHandle, enabled: bool| {
            if !enabled {
                return;
            }
            let (Some(&first_node), Some(&second_node)) = (owners.get(&first), owners.get(&second))
            else {
                return;
            };
            let edge = if first_node < second_node {
                (first_node, second_node)
            } else {
                (second_node, first_node)
            };
            is_trigger.insert(
                edge,
                triggers.get(&first).copied().unwrap_or(false)
                    || triggers.get(&second).copied().unwrap_or(false),
            );
            current.insert(edge);
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
        // Match the 2D solver's exit-before-entry ordering.
        for edge in &self.contacts {
            if !current.contains(edge) {
                self.transitions.push(Contact {
                    nodes: [edge.0, edge.1],
                    entered: false,
                    trigger: is_trigger.get(edge).copied().unwrap_or(false),
                });
            }
        }
        for edge in &current {
            if !self.contacts.contains(edge) {
                self.transitions.push(Contact {
                    nodes: [edge.0, edge.1],
                    entered: true,
                    trigger: is_trigger.get(edge).copied().unwrap_or(false),
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
                let body_state = &world.bodies[body.handle];
                let local = parent.inverse()
                    * Mat4::from_scale_rotation_translation(
                        body.scale,
                        *body_state.rotation(),
                        body_state.translation(),
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
}
