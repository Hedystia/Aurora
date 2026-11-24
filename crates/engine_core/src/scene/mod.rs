//! Ordered scene hierarchy with ECS-owned node and sprite components.

use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::{assets::AssetId, ecs::EntityId};
mod prefab;
pub mod spatial;
pub use prefab::{NodePool, Prefab, PrefabNode};

/// Local 2D pose using pixels, radians and unitless scale.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub struct Transform2D {
    /// Translation in the parent's coordinate space, with positive Y up.
    pub position: [f32; 2],
    /// Counterclockwise rotation in radians.
    pub rotation: f32,
    /// Local axis scale.
    pub scale: [f32; 2],
}

impl Default for Transform2D {
    fn default() -> Self {
        Self {
            position: [0.0; 2],
            rotation: 0.0,
            scale: [1.0; 2],
        }
    }
}

/// Affine transform columns, retaining shear from nested rotation and scale.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub struct Affine2D([[f32; 2]; 3]);

impl Affine2D {
    pub(crate) const IDENTITY: Self = Self([[1.0, 0.0], [0.0, 1.0], [0.0, 0.0]]);

    /// Maps a local point to world space.
    pub fn point(self, [x, y]: [f32; 2]) -> [f32; 2] {
        let [u, v, p] = self.0;
        [u[0] * x + v[0] * y + p[0], u[1] * x + v[1] * y + p[1]]
    }

    /// Maps a world point to local space, or returns `None` for a degenerate basis.
    pub fn inverse_point(self, [px, py]: [f32; 2]) -> Option<[f32; 2]> {
        let [u, v, p] = self.0;
        let determinant = u[0] * v[1] - u[1] * v[0];
        if determinant.abs() <= f32::EPSILON {
            return None;
        }
        let [dx, dy] = [px - p[0], py - p[1]];
        Some([
            (v[1] * dx - v[0] * dy) / determinant,
            (u[0] * dy - u[1] * dx) / determinant,
        ])
    }

    pub(crate) fn compose(self, local: Transform2D) -> Self {
        let (sin, cos) = local.rotation.sin_cos();
        let [x, y] = local.scale;
        let [u, v, _] = self.0;
        let basis = Self([u, v, [0.0; 2]]);
        Self([
            basis.point([cos * x, sin * x]),
            basis.point([-sin * y, cos * y]),
            self.point(local.position),
        ])
    }
}

/// A centered textured quad. Sibling insertion order determines compositing.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Sprite {
    /// Deterministic particle system carried by an emitter sprite.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emitter: Option<crate::save::visual::Emitter>,
    /// Imported texture handle.
    pub texture: AssetId,
    /// Quad width and height in local pixels.
    pub size: [f32; 2],
}

/// Optional visual properties for an authored node.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SpriteAppearance {
    /// Whether this node and its descendants are visible.
    pub active: bool,
    /// Quad origin as normalized left-to-right and bottom-to-top coordinates.
    pub anchor: [f32; 2],
    /// Horizontal and vertical shear coefficients.
    pub skew: [f32; 2],
    /// sRGBA tint channels.
    pub color: [u8; 4],
    /// Additional alpha multiplier.
    pub opacity: u8,
    /// Painter's order. Equal layers retain traversal order.
    #[serde(skip_serializing_if = "zero_layer")]
    pub layer: i32,
}

impl Default for SpriteAppearance {
    fn default() -> Self {
        Self {
            active: true,
            anchor: [0.5; 2],
            skew: [0.0; 2],
            color: [255; 4],
            opacity: 255,
            layer: 0,
        }
    }
}

fn zero_layer(layer: &i32) -> bool {
    *layer == 0
}

impl SpriteAppearance {
    /// The four local quad corners ordered top-left, bottom-left, top-right, bottom-right.
    pub fn corners(self, [width, height]: [f32; 2]) -> [[f32; 2]; 4] {
        let [x, y] = self.anchor;
        let left = -width * x;
        let right = width * (1.0 - x);
        let bottom = -height * y;
        let top = height * (1.0 - y);
        [[left, top], [left, bottom], [right, top], [right, bottom]]
            .map(|[x, y]| [x + self.skew[0] * y, y + self.skew[1] * x])
    }
}

fn is_true(value: &bool) -> bool {
    *value
}

/// A lifecycle event emitted by a scene operation or frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifecycle {
    /// Parent enters before children.
    EnterTree(EntityId),
    /// Children become ready before their parent.
    Ready(EntityId),
    /// Processing follows depth-first scene order.
    Process(EntityId),
    /// Children exit before their parent.
    ExitTree(EntityId),
}

struct Node {
    name: String,
    parent: Option<EntityId>,
    spatial: Option<spatial::SpatialNode>,
    children: Vec<EntityId>,
    local: Transform2D,
    appearance: Option<SpriteAppearance>,
}

/// Ordered scene graph whose component storage is backed by hecs.
#[derive(Default)]
pub struct Scene {
    world: World,
    entities: BTreeMap<EntityId, Entity>,
    roots: Vec<EntityId>,
    next_id: u64,
    active: bool,
}

/// A node's resolved state in scene traversal order.
#[derive(Debug, Serialize)]
pub struct NodeState {
    /// Fixed tick sampled by procedural draw geometry.
    #[serde(skip)]
    pub draw_tick: u64,
    /// Stable node handle.
    pub id: EntityId,
    /// Parent, if any.
    pub parent: Option<EntityId>,
    /// Display name.
    pub name: String,
    /// Parent-relative pose.
    pub local: Transform2D,
    /// Optional parent-relative spatial components.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spatial: Option<spatial::SpatialNode>,
    /// Composed world pose.
    pub world: Affine2D,
    /// Sprite component, when present.
    pub sprite: Option<Sprite>,
    /// Optional authored visual appearance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub appearance: Option<SpriteAppearance>,
    /// Visibility inherited through the node tree.
    #[serde(skip_serializing_if = "is_true")]
    pub visible: bool,
}

impl Scene {
    pub(crate) fn successor(&self) -> Self {
        Self {
            next_id: self.next_id,
            ..Self::default()
        }
    }

    pub(crate) fn deactivate(&mut self) -> Result<Vec<Lifecycle>> {
        let mut events = Vec::new();
        if self.active {
            self.walk(
                &self.roots,
                |_, _, _| Ok(()),
                |id| events.push(Lifecycle::ExitTree(id)),
            )?;
            self.active = false;
        }
        Ok(events)
    }
    fn entity(&self, id: EntityId) -> Result<Entity> {
        self.entities
            .get(&id)
            .copied()
            .with_context(|| format!("Unknown node {id:?}"))
    }

    fn node(&self, id: EntityId) -> Result<hecs::Ref<'_, Node>> {
        Ok(self.world.get::<&Node>(self.entity(id)?)?)
    }

    fn siblings(&mut self, parent: Option<EntityId>) -> Result<&mut Vec<EntityId>> {
        match parent {
            Some(parent) => {
                let entity = self.entity(parent)?;
                Ok(&mut self.world.query_one_mut::<&mut Node>(entity)?.children)
            }
            None => Ok(&mut self.roots),
        }
    }

    pub(crate) fn has_bodies2d(&self) -> bool {
        self.world
            .query::<&crate::physics2d::Body2D>()
            .iter()
            .next()
            .is_some()
    }

    pub(crate) fn body2d(
        &self,
        id: EntityId,
    ) -> Result<Option<hecs::Ref<'_, crate::physics2d::Body2D>>> {
        Ok(self
            .world
            .get::<&crate::physics2d::Body2D>(self.entity(id)?)
            .ok())
    }

    pub(crate) fn set_body2d(
        &mut self,
        id: EntityId,
        body: Option<crate::physics2d::Body2D>,
    ) -> Result<()> {
        let entity = self.entity(id)?;
        if let Some(body) = body {
            self.world.insert_one(entity, body)?;
        } else if self.world.satisfies::<&crate::physics2d::Body2D>(entity) {
            self.world.remove_one::<crate::physics2d::Body2D>(entity)?;
        }
        Ok(())
    }

    /// Adds a node, emitting entry and readiness if the scene is already active.
    pub fn spawn(
        &mut self,
        name: impl Into<String>,
        parent: Option<EntityId>,
        local: Transform2D,
    ) -> Result<(EntityId, Vec<Lifecycle>)> {
        let id = EntityId(self.next_id);
        let next = self
            .next_id
            .checked_add(1)
            .context("Scene handles exhausted")?;
        let name = name.into();
        self.siblings(parent)?.push(id);
        let entity = self.world.spawn((Node {
            name,
            parent,
            local,
            appearance: None,
            spatial: None,
            children: Vec::new(),
        },));
        self.entities.insert(id, entity);
        self.next_id = next;
        let events = if self.active {
            vec![Lifecycle::EnterTree(id), Lifecycle::Ready(id)]
        } else {
            Vec::new()
        };
        Ok((id, events))
    }

    /// Returns a node's sprite if registered.
    pub fn sprite(&self, id: EntityId) -> Result<Option<Sprite>> {
        Ok(self
            .world
            .get::<&Sprite>(self.entity(id)?)
            .ok()
            .as_deref()
            .copied())
    }

    /// Changes visibility while preserving the other visual overrides.
    pub fn set_visible(&mut self, id: EntityId, visible: bool) -> Result<()> {
        let entity = self.entity(id)?;
        let mut node = self.world.get::<&mut Node>(entity)?;
        let mut appearance = node.appearance.unwrap_or_default();
        appearance.active = visible;
        node.appearance = Some(appearance);
        Ok(())
    }

    pub(crate) fn has_spatial_bodies(&self) -> bool {
        self.world.query::<&Node>().iter().any(|n| {
            n.spatial
                .as_ref()
                .is_some_and(|s| s.body != spatial::Body::None)
        })
    }

    pub(crate) fn node_count(&self) -> usize {
        self.entities.len()
    }

    pub(crate) fn active_in_hierarchy(&self, mut id: EntityId) -> Result<bool> {
        if !self.active {
            return Ok(false);
        }
        loop {
            let node = self.node(id)?;
            if !node.appearance.is_none_or(|appearance| appearance.active) {
                return Ok(false);
            }
            let Some(parent) = node.parent else {
                return Ok(true);
            };
            id = parent;
        }
    }

    pub(crate) fn visual(&self, id: EntityId) -> Result<Option<crate::save::visual::Visual>> {
        Ok(self
            .world
            .get::<&crate::save::visual::Visual>(self.entity(id)?)
            .ok()
            .as_deref()
            .cloned())
    }

    pub(crate) fn set_visual(
        &mut self,
        id: EntityId,
        visual: Option<crate::save::visual::Visual>,
    ) -> Result<()> {
        let entity = self.entity(id)?;
        if let Some(visual) = visual {
            self.world.insert_one(entity, visual)?;
        } else if self
            .world
            .get::<&crate::save::visual::Visual>(entity)
            .is_ok()
        {
            self.world
                .remove_one::<crate::save::visual::Visual>(entity)?;
        }
        Ok(())
    }

    /// Sets or replaces a node's sprite.
    pub fn set_sprite(&mut self, id: EntityId, sprite: Sprite) -> Result<()> {
        Ok(self.world.insert_one(self.entity(id)?, sprite)?)
    }

    /// Replaces the node's optional visual overrides.
    pub fn set_appearance(
        &mut self,
        id: EntityId,
        appearance: Option<SpriteAppearance>,
    ) -> Result<()> {
        self.world.get::<&mut Node>(self.entity(id)?)?.appearance = appearance;
        Ok(())
    }

    /// Replaces the local spatial component, retaining all 2D components.
    pub fn set_spatial(
        &mut self,
        id: EntityId,
        spatial: Option<spatial::SpatialNode>,
    ) -> Result<()> {
        if let Some(component) = &spatial {
            component.validate()?;
        }
        self.world.get::<&mut Node>(self.entity(id)?)?.spatial = spatial;
        Ok(())
    }

    /// Reads a node's local pose.
    pub fn transform(&self, id: EntityId) -> Result<Transform2D> {
        Ok(self.node(id)?.local)
    }

    /// Updates the local transform.
    pub fn set_transform(&mut self, id: EntityId, local: Transform2D) -> Result<()> {
        self.world.get::<&mut Node>(self.entity(id)?)?.local = local;
        Ok(())
    }

    /// Moves a subtree, retaining its local pose and readiness.
    ///
    /// In an active scene, emits child-first exit followed by parent-first entry.
    pub fn reparent(&mut self, id: EntityId, parent: Option<EntityId>) -> Result<Vec<Lifecycle>> {
        let old = self.node(id)?.parent;
        if old == parent {
            return Ok(Vec::new());
        }
        let mut ancestor = parent;
        while let Some(current) = ancestor {
            ensure!(current != id, "Reparenting would create a cycle");
            ancestor = self.node(current)?.parent;
        }

        let (mut exit, mut enter) = (Vec::new(), Vec::new());
        if self.active {
            self.walk(
                &[id],
                |node, _, _| {
                    enter.push(Lifecycle::EnterTree(node));
                    Ok(())
                },
                |node| exit.push(Lifecycle::ExitTree(node)),
            )?;
        }
        self.siblings(old)?.retain(|node| *node != id);
        self.siblings(parent)?.push(id);
        self.world.get::<&mut Node>(self.entity(id)?)?.parent = parent;
        exit.extend(enter);
        Ok(exit)
    }

    // Iterative traversal avoids exhausting the stack for nested hierarchies.
    fn walk(
        &self,
        roots: &[EntityId],
        mut enter: impl FnMut(EntityId, &Node, Affine2D) -> Result<()>,
        mut exit: impl FnMut(EntityId),
    ) -> Result<()> {
        let mut pending: Vec<_> = roots
            .iter()
            .rev()
            .map(|&id| (id, Some(Affine2D::IDENTITY)))
            .collect();
        while let Some((id, parent)) = pending.pop() {
            if let Some(parent) = parent {
                let node = self.node(id)?;
                let world = parent.compose(node.local);
                enter(id, &node, world)?;
                pending.push((id, None));
                pending.extend(
                    node.children
                        .iter()
                        .rev()
                        .map(|&child| (child, Some(world))),
                );
            } else {
                exit(id);
            }
        }
        Ok(())
    }

    /// Starts the scene once. All entry notifications precede the ready notifications.
    pub fn activate(&mut self) -> Result<Vec<Lifecycle>> {
        if self.active {
            return Ok(Vec::new());
        }
        let (mut enter, mut ready) = (Vec::new(), Vec::new());
        self.walk(
            &self.roots,
            |id, _, _| {
                enter.push(Lifecycle::EnterTree(id));
                Ok(())
            },
            |id| ready.push(Lifecycle::Ready(id)),
        )?;
        self.active = true;
        enter.extend(ready);
        Ok(enter)
    }

    pub(crate) fn subtree(&self, root: EntityId) -> Result<Vec<EntityId>> {
        let mut nodes = Vec::new();
        self.walk(&[root], |_, _, _| Ok(()), |id| nodes.push(id))?;
        Ok(nodes)
    }

    /// Removes a subtree, invalidates its handles and returns exit events.
    pub fn remove(&mut self, id: EntityId) -> Result<Vec<Lifecycle>> {
        let parent = self.node(id)?.parent;
        let mut removed = Vec::new();
        self.walk(&[id], |_, _, _| Ok(()), |id| removed.push(id))?;
        self.siblings(parent)?.retain(|node| *node != id);
        for &id in &removed {
            let entity = self.entity(id)?;
            self.world.despawn(entity)?;
            self.entities.remove(&id);
        }
        Ok(if self.active {
            removed.into_iter().map(Lifecycle::ExitTree).collect()
        } else {
            Vec::new()
        })
    }

    /// Emits process events for one simulation tick in the active scene.
    pub fn process(&self) -> Result<Vec<Lifecycle>> {
        let mut events = Vec::new();
        if self.active {
            self.walk(
                &self.roots,
                |id, _, _| {
                    events.push(Lifecycle::Process(id));
                    Ok(())
                },
                |_| {},
            )?;
        }
        Ok(events)
    }

    /// Resolves transforms and sprite state in deterministic scene order.
    pub fn snapshot(&self) -> Result<Vec<NodeState>> {
        let mut states = Vec::new();
        let mut visibility = BTreeMap::new();
        self.walk(
            &self.roots,
            |id, node, world| {
                let sprite = self
                    .world
                    .get::<&Sprite>(self.entity(id)?)
                    .ok()
                    .as_deref()
                    .copied();
                let visible = node.appearance.is_none_or(|style| style.active)
                    && node
                        .parent
                        .is_none_or(|parent| visibility.get(&parent).copied().unwrap_or(true));
                visibility.insert(id, visible);
                states.push(NodeState {
                    draw_tick: 0,
                    id,
                    name: node.name.clone(),
                    parent: node.parent,
                    local: node.local,
                    spatial: node.spatial.clone(),
                    world,
                    sprite,
                    appearance: node.appearance,
                    visible,
                });
                Ok(())
            },
            |_| {},
        )?;
        Ok(states)
    }
}

/// How the design area maps into a differently shaped window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    /// Show the whole area and preserve aspect ratio.
    #[default]
    Contain,
    /// Fill the window and crop the edges.
    Cover,
    /// Fill each axis independently.
    Stretch,
}

/// Authored 2D design size and fit policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Display2D {
    /// Width and height in design pixels.
    pub resolution: [u32; 2],
    /// Fit policy for the player window.
    pub fit: Fit,
}

impl Default for Display2D {
    fn default() -> Self {
        Self {
            resolution: [640, 384],
            fit: Fit::Contain,
        }
    }
}

impl Display2D {
    /// Maximum size of either authored dimension.
    pub const MAX_DIMENSION: u32 = 2048;

    /// Returns whether both dimensions are supported.
    pub fn is_valid(self) -> bool {
        self.resolution
            .into_iter()
            .all(|v| (1..=Self::MAX_DIMENSION).contains(&v))
    }

    /// Returns an aspect-preserving frame size for the editor preview stream.
    pub fn preview_resolution(self) -> [u32; 2] {
        const PREVIEW_MAX: u32 = 960;
        let [width, height] = self.resolution;
        let largest = width.max(height);
        if largest <= PREVIEW_MAX {
            return self.resolution;
        }
        self.resolution.map(|value| {
            ((u64::from(value) * u64::from(PREVIEW_MAX) / u64::from(largest)) as u32).max(1)
        })
    }

    /// Maps the authored area to an origin-centered camera.
    pub fn camera(self) -> Camera2D {
        Camera2D {
            center: [0.0; 2],
            design_size: self.resolution.map(|dimension| dimension as f32),
            fit: self.fit,
        }
    }
}

/// Orthographic camera framing a fixed design resolution at any window aspect ratio.
#[derive(Clone, Copy, Debug)]
pub struct Camera2D {
    /// World position at the center of the viewport.
    pub center: [f32; 2],
    /// Width and height of the area that must remain visible, in world pixels.
    pub design_size: [f32; 2],
    /// Fit policy for the viewport.
    pub fit: Fit,
}

impl Camera2D {
    /// Returns pixels per world unit along each axis.
    pub fn scale(self, [width, height]: [u32; 2]) -> [f32; 2] {
        let x = width as f32 / self.design_size[0];
        let y = height as f32 / self.design_size[1];
        match self.fit {
            Fit::Contain => [x.min(y); 2],
            Fit::Cover => [x.max(y); 2],
            Fit::Stretch => [x, y],
        }
    }

    /// Maps a world position into GPU clip coordinates.
    pub fn project(self, [x, y]: [f32; 2], [width, height]: [u32; 2]) -> [f32; 2] {
        let [sx, sy] = self.scale([width, height]);
        let width = width as f32;
        let height = height as f32;
        [
            2.0 * (x - self.center[0]) * sx / width,
            2.0 * (y - self.center[1]) * sy / height,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Lifecycle::{EnterTree, ExitTree, Process, Ready};

    #[test]
    fn lifecycle_reparenting_and_stale_handles() -> Result<()> {
        let mut scene = Scene::default();
        let local = Transform2D::default();
        let (root, _) = scene.spawn("Root", None, local)?;
        let (child, _) = scene.spawn("Child", Some(root), local)?;
        let (leaf, _) = scene.spawn("Leaf", Some(child), local)?;
        assert!(scene.process()?.is_empty());
        assert_eq!(
            scene.activate()?,
            [
                EnterTree(root),
                EnterTree(child),
                EnterTree(leaf),
                Ready(leaf),
                Ready(child),
                Ready(root)
            ]
        );
        assert!(scene.activate()?.is_empty());
        let (other, added) = scene.spawn("Other", None, local)?;
        assert_eq!(added, [EnterTree(other), Ready(other)]);
        assert!(scene.reparent(root, Some(leaf)).is_err());
        assert_eq!(
            scene.process()?,
            [Process(root), Process(child), Process(leaf), Process(other)]
        );
        assert_eq!(
            scene.reparent(child, Some(other))?,
            [
                ExitTree(leaf),
                ExitTree(child),
                EnterTree(child),
                EnterTree(leaf)
            ]
        );
        assert_eq!(
            scene.remove(other)?,
            [ExitTree(leaf), ExitTree(child), ExitTree(other)]
        );
        assert!(scene.set_transform(leaf, local).is_err());
        assert!(scene.spawn("Stale", Some(other), local).is_err());
        let (fresh, _) = scene.spawn("Fresh", Some(root), local)?;
        assert!(fresh > other);
        assert_eq!(scene.process()?, [Process(root), Process(fresh)]);
        Ok(())
    }

    #[test]
    fn nested_transforms_and_camera_preserve_world_scale() -> Result<()> {
        let mut scene = Scene::default();
        let (root, _) = scene.spawn(
            "Root",
            None,
            Transform2D {
                position: [10.0, -6.0],
                rotation: std::f32::consts::FRAC_PI_2,
                scale: [2.0, 3.0],
            },
        )?;
        scene.spawn(
            "Child",
            Some(root),
            Transform2D {
                position: [2.0, 1.0],
                rotation: std::f32::consts::FRAC_PI_2,
                scale: [4.0, 5.0],
            },
        )?;
        let nodes = scene.snapshot()?;
        let point = nodes[1].world.point([1.0, 1.0]);
        assert!((point[0] + 5.0).abs() < 0.0001);
        assert!((point[1] + 12.0).abs() < 0.0001);
        let camera = Camera2D {
            center: [10.0, -6.0],
            design_size: [100.0, 100.0],
            fit: Fit::Contain,
        };
        assert_eq!(camera.project([60.0, 44.0], [200, 100]), [0.5, 1.0]);
        assert_eq!(
            Camera2D {
                fit: Fit::Cover,
                ..camera
            }
            .project([60.0, 44.0], [200, 100]),
            [1.0, 2.0]
        );
        assert_eq!(
            Camera2D {
                fit: Fit::Stretch,
                ..camera
            }
            .project([60.0, 44.0], [200, 100]),
            [1.0, 1.0]
        );
        Ok(())
    }
}
