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
use rapier3d::prelude::{ColliderBuilder, PhysicsWorld, Pose, RigidBodyBuilder, RigidBodyHandle};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(crate) struct AuthoredWorld {
    world: Option<PhysicsWorld>,
    handles: BTreeMap<EntityId, (RigidBodyHandle, Body, Vec3)>,
}

impl AuthoredWorld {
    pub(crate) fn step(&mut self, scene: &mut Scene) -> Result<()> {
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
            if let Some(&(handle, kind, extent)) = self.handles.get(&node.id) {
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
            let (handle, _) = world.insert(
                builder.pose(pose),
                ColliderBuilder::cuboid(half.x, half.y, half.z),
            );
            self.handles.insert(node.id, (handle, s.body, scale));
        }
        self.handles.retain(|id, (handle, _, _)| {
            if active.contains(id) {
                return true;
            }
            world.remove_body(*handle);
            false
        });
        world.step();
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
            if let Some(&(handle, Body::Dynamic, scale)) = self.handles.get(&node.id) {
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
