//! Scene-local ownership of authored Rapier cuboids.

use crate::{
    ecs::EntityId,
    scene::{
        Scene,
        spatial::{Body, matrices},
    },
};
use anyhow::{Result, ensure};
use glam::{EulerRot, Mat4, Vec3};
use rapier3d::prelude::{
    ColliderBuilder, ColliderHandle, PhysicsWorld, Pose, RigidBodyBuilder, RigidBodyHandle,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// A cuboid contact transition for one scene tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Contact {
    pub nodes: [EntityId; 2],
    pub entered: bool,
}

#[derive(Default)]
pub(crate) struct AuthoredWorld {
    world: Option<PhysicsWorld>,
    handles: BTreeMap<EntityId, (RigidBodyHandle, ColliderHandle, Body, Vec3)>,
    contacts: BTreeSet<(EntityId, EntityId)>,
    transitions: Vec<Contact>,
    pending: BTreeMap<EntityId, [f32; 3]>,
}

impl AuthoredWorld {
    /// Queues a script velocity for a dynamic cuboid.
    ///
    /// The authored world is built lazily on the first step, so a velocity set
    /// from a scene-start or fixed-update callback is applied when the body is
    /// created rather than rejected. A node that is not a dynamic cuboid leaves
    /// the queued value unused.
    pub(crate) fn set_velocity(&mut self, node: EntityId, velocity: [f32; 3]) -> Result<()> {
        ensure!(
            velocity.iter().all(|value| value.is_finite()),
            "3D velocity must be finite"
        );
        self.pending.insert(node, velocity);
        Ok(())
    }

    /// Contact transitions produced by the most recent step.
    pub(crate) fn contacts(&self) -> &[Contact] {
        &self.transitions
    }

    pub(crate) fn step(&mut self, scene: &mut Scene) -> Result<()> {
        self.transitions.clear();
        if self.world.is_none() && !scene.has_spatial_bodies() {
            return Ok(());
        }
        let nodes = scene.snapshot()?;
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
            if let Some(&(handle, _, kind, extent)) = self.handles.get(&node.id) {
                if kind == s.body && extent.abs_diff_eq(scale, 0.0001) {
                    if kind == Body::Fixed {
                        world.bodies[handle].set_position(pose, true);
                    }
                    continue;
                }
                world.remove_body(handle);
            }
            let builder = if s.body == Body::Dynamic {
                RigidBodyBuilder::dynamic()
            } else {
                RigidBodyBuilder::fixed()
            };
            let half = scale * 0.5;
            let (handle, collider) = world.insert(
                builder.pose(pose),
                ColliderBuilder::cuboid(half.x, half.y, half.z),
            );
            self.handles
                .insert(node.id, (handle, collider, s.body, scale));
        }
        self.handles.retain(|id, (handle, _, _, _)| {
            if active.contains(id) {
                return true;
            }
            world.remove_body(*handle);
            false
        });
        for (&node, &velocity) in &self.pending {
            if let Some(&(handle, _, Body::Dynamic, _)) = self.handles.get(&node) {
                world.bodies[handle].set_linvel(Vec3::from_array(velocity), true);
            }
        }
        self.pending.clear();
        world.step();
        let owners: HashMap<ColliderHandle, EntityId> = self
            .handles
            .iter()
            .map(|(&entity, &(_, collider, _, _))| (collider, entity))
            .collect();
        let mut current = BTreeSet::new();
        for pair in world.contact_pairs() {
            if !pair.has_any_active_contact() {
                continue;
            }
            let (Some(&first), Some(&second)) =
                (owners.get(&pair.collider1), owners.get(&pair.collider2))
            else {
                continue;
            };
            let edge = if first < second {
                (first, second)
            } else {
                (second, first)
            };
            if current.insert(edge) && !self.contacts.contains(&edge) {
                self.transitions.push(Contact {
                    nodes: [edge.0, edge.1],
                    entered: true,
                });
            }
        }
        for edge in &self.contacts {
            if !current.contains(edge) {
                self.transitions.push(Contact {
                    nodes: [edge.0, edge.1],
                    entered: false,
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
            if let Some(&(handle, _, Body::Dynamic, scale)) = self.handles.get(&node.id) {
                let body = &world.bodies[handle];
                let local = parent.inverse()
                    * Mat4::from_scale_rotation_translation(
                        scale,
                        *body.rotation(),
                        body.translation(),
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
