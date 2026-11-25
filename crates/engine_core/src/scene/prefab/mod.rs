//! Subtree templates and reusable scene-local node allocations.

use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};

use super::{Entity, EntityId, Lifecycle, Node, Scene, Sprite, SpriteAppearance, Transform2D};

/// One node in a prefab, indexed in parent-first order.
#[derive(Clone, Debug)]
pub struct PrefabNode {
    /// Display name.
    pub name: String,
    /// Parent index, or none for the root.
    pub parent: Option<usize>,
    /// Local pose restored on checkout.
    pub pose: Transform2D,
    /// Optional spatial components.
    pub spatial: Option<super::spatial::SpatialNode>,
    /// Editable source for generated sprites.
    pub visual: Option<crate::save::visual::Visual>,
    /// Resolved sprite, if present.
    pub sprite: Option<Sprite>,
    /// Optional authored 2D physics component.
    pub physics2d: Option<crate::physics2d::Body2D>,
    /// Optional authored visual appearance.
    pub appearance: Option<SpriteAppearance>,
}

/// Immutable, resolved template for one scene subtree.
pub struct Prefab {
    nodes: Vec<PrefabNode>,
    clips: Vec<crate::animation::Clip>,
}

impl Prefab {
    /// Validates that the template has one root and all parents precede children.
    pub fn new(nodes: Vec<PrefabNode>, clips: Vec<crate::animation::Clip>) -> Result<Self> {
        ensure!(
            !nodes.is_empty() && nodes[0].parent.is_none(),
            "Prefab needs a root"
        );
        for (index, node) in nodes.iter().enumerate().skip(1) {
            ensure!(
                node.parent.is_some_and(|parent| parent < index),
                "Invalid prefab parent at node {index}"
            );
        }
        for clip in &clips {
            clip.validate(nodes.len())?;
        }
        Ok(Self { nodes, clips })
    }

    /// Exposes template nodes in their authored order.
    pub fn nodes(&self) -> &[PrefabNode] {
        &self.nodes
    }

    /// Exposes clips bound to the template's authored node order.
    pub fn clips(&self) -> &[crate::animation::Clip] {
        &self.clips
    }

    /// Copies an existing scene subtree into an independent template.
    pub fn capture(scene: &Scene, root: EntityId) -> Result<Self> {
        let mut indices = BTreeMap::new();
        let mut nodes = Vec::new();
        scene.walk(
            &[root],
            |id, node, _| {
                let parent = node.parent.and_then(|id| indices.get(&id).copied());
                indices.insert(id, nodes.len());
                let sprite = scene
                    .world
                    .get::<&Sprite>(scene.entity(id)?)
                    .ok()
                    .as_deref()
                    .copied();
                nodes.push(PrefabNode {
                    name: node.name.clone(),
                    parent,
                    pose: node.local,
                    physics2d: scene.body2d(id)?.as_deref().cloned(),
                    spatial: node.spatial.clone(),
                    visual: scene.visual(id)?,
                    sprite,
                    appearance: node.appearance,
                });
                Ok(())
            },
            |_| {},
        )?;
        Self::new(nodes, Vec::new())
    }

    /// Instantiates a fresh subtree, returning handles and ordered lifecycle events.
    pub fn instantiate(
        &self,
        scene: &mut Scene,
        parent: Option<EntityId>,
    ) -> Result<(Vec<EntityId>, Vec<Lifecycle>)> {
        self.attach(scene, parent, None)
    }

    fn attach(
        &self,
        scene: &mut Scene,
        parent: Option<EntityId>,
        spare: Option<&[Entity]>,
    ) -> Result<(Vec<EntityId>, Vec<Lifecycle>)> {
        if let Some(parent) = parent {
            scene.entity(parent)?;
        }
        let end = scene
            .next_id
            .checked_add(self.nodes.len() as u64)
            .context("Scene handles exhausted")?;
        let ids: Vec<_> = (scene.next_id..end).map(EntityId).collect();
        scene.next_id = end;
        for (index, template) in self.nodes.iter().enumerate() {
            let parent = template.parent.map(|i| ids[i]).or(parent);
            let node = Node {
                name: template.name.clone(),
                parent,
                local: template.pose,
                spatial: template.spatial.clone(),
                appearance: template.appearance,
                children: Vec::new(),
            };
            let entity = if let Some(spare) = spare {
                let entity = spare[index];
                *scene.world.get::<&mut Node>(entity)? = node;
                entity
            } else {
                scene.world.spawn((node,))
            };
            scene.entities.insert(ids[index], entity);
            scene.siblings(parent)?.push(ids[index]);
            scene.set_visual(ids[index], template.visual.clone())?;
            scene.set_body2d(ids[index], template.physics2d.clone())?;
            if let Some(sprite) = template.sprite {
                scene.set_sprite(ids[index], sprite)?;
            } else if scene.world.get::<&Sprite>(entity).is_ok() {
                scene.world.remove_one::<Sprite>(entity)?;
            }
        }
        let mut enter = Vec::new();
        let mut ready = Vec::new();
        if scene.active {
            scene.walk(
                &ids[..1],
                |id, _, _| {
                    enter.push(Lifecycle::EnterTree(id));
                    Ok(())
                },
                |id| ready.push(Lifecycle::Ready(id)),
            )?;
            enter.extend(ready);
        }
        Ok((ids, enter))
    }
}

/// Scene-bound pool that keeps detached ECS allocations and invalidates returned handles.
pub struct NodePool<'a> {
    scene: &'a mut Scene,
    prefab: &'a Prefab,
    active: BTreeMap<EntityId, Vec<EntityId>>,
    spare: Vec<Vec<Entity>>,
}

impl<'a> NodePool<'a> {
    /// Creates an empty pool for one template in one scene.
    pub fn new(scene: &'a mut Scene, prefab: &'a Prefab) -> Self {
        Self {
            scene,
            prefab,
            active: BTreeMap::new(),
            spare: Vec::new(),
        }
    }

    /// Gets a reset instance, reusing an allocation when available.
    pub fn get(&mut self, parent: Option<EntityId>) -> Result<(EntityId, Vec<Lifecycle>)> {
        let (ids, events) =
            self.prefab
                .attach(self.scene, parent, self.spare.last().map(Vec::as_slice))?;
        self.spare.pop();
        let root = ids[0];
        self.active.insert(root, ids);
        Ok((root, events))
    }

    /// Accesses the owning scene while the pool is in use.
    pub fn scene_mut(&mut self) -> &mut Scene {
        self.scene
    }

    fn checked(&self, root: EntityId) -> Result<&[EntityId]> {
        let ids = self
            .active
            .get(&root)
            .context("Node was not checked out from this pool")?;
        let expected: BTreeMap<_, _> = ids.iter().enumerate().map(|(i, &id)| (id, i)).collect();
        let mut seen = 0;
        self.scene.walk(
            &[root],
            |id, node, _| {
                let i = *expected
                    .get(&id)
                    .context("Pooled subtree topology changed")?;
                ensure!(
                    i == 0 || node.parent == self.prefab.nodes[i].parent.map(|p| ids[p]),
                    "Pooled subtree topology changed"
                );
                seen += 1;
                Ok(())
            },
            |_| {},
        )?;
        ensure!(seen == ids.len(), "Pooled subtree topology changed");
        Ok(ids)
    }

    /// Restores the checked-out instance's authored transforms and sprite components.
    pub fn reset(&mut self, root: EntityId) -> Result<()> {
        let ids = self.checked(root)?.to_vec();
        for (&id, template) in ids.iter().zip(&self.prefab.nodes) {
            let entity = self.scene.entity(id)?;
            {
                let mut node = self.scene.world.get::<&mut Node>(entity)?;
                node.local = template.pose;
                node.appearance = template.appearance;
                node.name.clone_from(&template.name);
            }
            if let Some(sprite) = template.sprite {
                self.scene.set_sprite(id, sprite)?;
            } else if self.scene.world.get::<&Sprite>(entity).is_ok() {
                self.scene.world.remove_one::<Sprite>(entity)?;
            }
        }
        Ok(())
    }

    /// Returns an instance, invalidating old handles and emitting child-first exits.
    pub fn put(&mut self, root: EntityId) -> Result<Vec<Lifecycle>> {
        let ids = self.checked(root)?.to_vec();
        let entities = ids
            .iter()
            .map(|&id| self.scene.entity(id))
            .collect::<Result<Vec<_>>>()?;
        let parent = self.scene.node(root)?.parent;
        let mut exits = Vec::new();
        if self.scene.active {
            self.scene.walk(
                &[root],
                |_, _, _| Ok(()),
                |id| exits.push(Lifecycle::ExitTree(id)),
            )?;
        }
        self.scene.siblings(parent)?.retain(|&id| id != root);
        for id in ids {
            self.scene.entities.remove(&id);
        }
        self.active.remove(&root);
        self.spare.push(entities);
        Ok(exits)
    }

    /// Returns the number of instances available without further ECS allocation.
    pub fn available(&self) -> usize {
        self.spare.len()
    }
}

impl Drop for NodePool<'_> {
    fn drop(&mut self) {
        for entity in self.spare.iter().flatten() {
            let _ = self.scene.world.despawn(*entity);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reuses_allocations_and_invalidates_returned_handles() -> Result<()> {
        let mut scene = Scene::default();
        let (root, _) = scene.spawn("Root", None, Transform2D::default())?;
        let (child, _) = scene.spawn("Child", Some(root), Transform2D::default())?;
        let prefab = Prefab::capture(&scene, root)?;
        scene.remove(root)?;
        scene.activate()?;
        let mut pool = NodePool::new(&mut scene, &prefab);
        let (first, enter) = pool.get(None)?;
        assert_eq!(enter.len(), 4);
        let original_allocation = pool.scene_mut().entity(first)?;
        pool.scene_mut().set_transform(
            first,
            Transform2D {
                rotation: 1.0,
                ..Default::default()
            },
        )?;
        pool.reset(first)?;
        assert_eq!(pool.scene_mut().snapshot()?[0].local.rotation, 0.0);
        assert_eq!(pool.put(first)?.len(), 2);
        assert_eq!(pool.available(), 1);
        assert!(pool.scene_mut().snapshot()?.is_empty());
        assert!(
            pool.scene_mut()
                .set_transform(first, Transform2D::default())
                .is_err()
        );
        let (second, _) = pool.get(None)?;
        assert_ne!(first, second);
        assert_ne!(second, child);
        assert_eq!(pool.scene_mut().entity(second)?, original_allocation);
        assert_eq!(pool.scene_mut().snapshot()?[1].parent, Some(second));
        assert!(pool.put(first).is_err());
        Ok(())
    }
}
