//! Fixed-step Rapier world with node-owned bodies and ordered contacts.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};
use rapier2d::prelude::*;

use crate::ecs::EntityId;

/// Duration of one 60 Hz physics step in seconds.
pub const STEP: f32 = 1.0 / 60.0;

/// A change in contact between two node-owned colliders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Contact {
    /// Node handles in ascending order.
    pub nodes: [EntityId; 2],
    /// True for contact entry and false for contact exit.
    pub entered: bool,
}

/// One body and collider per registered scene node.
pub struct Physics2D {
    world: PhysicsWorld,
    handles: BTreeMap<EntityId, RigidBodyHandle>,
    touching: BTreeSet<[EntityId; 2]>,
}

impl Default for Physics2D {
    fn default() -> Self {
        let mut world = PhysicsWorld::default();
        world.integration_parameters.dt = STEP;
        Self {
            world,
            handles: BTreeMap::new(),
            touching: BTreeSet::new(),
        }
    }
}

impl Physics2D {
    /// Gives a node a Rapier body and collider expressed in meters.
    pub fn insert(
        &mut self,
        node: EntityId,
        body: RigidBodyBuilder,
        collider: ColliderBuilder,
    ) -> Result<()> {
        ensure!(
            !self.handles.contains_key(&node),
            "Node already owns a body"
        );
        let (body, _) = self
            .world
            .insert(body, collider.user_data(u128::from(node.0)));
        self.handles.insert(node, body);
        Ok(())
    }

    /// Gets the body associated with a scene node.
    pub fn body(&self, node: EntityId) -> Option<&RigidBody> {
        self.world.bodies.get(*self.handles.get(&node)?)
    }

    /// Mutably accesses a node's body for simulation commands.
    pub fn body_mut(&mut self, node: EntityId) -> Option<&mut RigidBody> {
        self.world.bodies.get_mut(*self.handles.get(&node)?)
    }

    /// Removes a body and its collider. The next step reports any contact exits.
    pub fn remove(&mut self, node: EntityId) -> bool {
        self.handles
            .remove(&node)
            .and_then(|handle| self.world.remove_body(handle))
            .is_some()
    }

    /// Advances one step and returns exit events followed by entry events in handle order.
    pub fn step(&mut self) -> Vec<Contact> {
        self.world.step();
        let touching: BTreeSet<_> = self
            .world
            .narrow_phase
            .contact_pairs()
            .filter(|pair| pair.has_any_active_contact())
            .map(|pair| {
                let mut nodes = [
                    EntityId(self.world.colliders[pair.collider1].user_data as u64),
                    EntityId(self.world.colliders[pair.collider2].user_data as u64),
                ];
                nodes.sort_unstable();
                nodes
            })
            .collect();
        let events = self
            .touching
            .difference(&touching)
            .map(|&nodes| Contact {
                nodes,
                entered: false,
            })
            .chain(touching.difference(&self.touching).map(|&nodes| Contact {
                nodes,
                entered: true,
            }))
            .collect();
        self.touching = touching;
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context;

    #[test]
    fn dynamic_circle_contacts_static_box_and_polygon_kinematic_body() -> Result<()> {
        for kinematic in [false, true] {
            let mut world = Physics2D::default();
            let floor = EntityId(1);
            let actor = EntityId(2);
            let (body, shape) = if kinematic {
                let shape = ColliderBuilder::convex_hull(&[
                    Vector::new(-2.0, -0.25),
                    Vector::new(2.0, -0.25),
                    Vector::new(2.0, 0.0),
                    Vector::new(-2.0, 0.0),
                ])
                .context("Invalid test polygon")?;
                (
                    RigidBodyBuilder::kinematic_velocity_based().linvel(Vector::new(0.05, 0.0)),
                    shape,
                )
            } else {
                (RigidBodyBuilder::fixed(), ColliderBuilder::cuboid(2.0, 0.1))
            };
            world.insert(floor, body, shape)?;
            world.insert(
                actor,
                RigidBodyBuilder::dynamic().translation(Vector::new(0.0, 1.0)),
                ColliderBuilder::ball(0.25),
            )?;
            let events: Vec<_> = (0..60).flat_map(|_| world.step()).collect();
            assert_eq!(
                events,
                [Contact {
                    nodes: [floor, actor],
                    entered: true
                }]
            );
            assert!(world.remove(floor));
            assert_eq!(
                world.step(),
                [Contact {
                    nodes: [floor, actor],
                    entered: false
                }]
            );
            assert!(world.step().is_empty());
        }
        Ok(())
    }
}
