//! Validated runtime creation and removal of script-owned scene subtrees.

use std::{
    collections::{BTreeMap, BTreeSet, btree_map::Entry},
    path::Path,
};

use anyhow::{Context, Result, ensure};

use crate::{
    ecs::EntityId,
    save::{MAX_NODES, PrefabFile, parent_dir},
    scene::{Prefab, Transform2D},
};

use super::Director;

const FIRST_DYNAMIC_HANDLE: u32 = 0x8000_0000;
const FIRST_CHILD_HANDLE: u32 = 0xc000_0000;

pub(crate) struct ScriptNode {
    pub reference: u32,
    pub entity: EntityId,
    pub parent: Option<u32>,
}

pub(crate) struct ScriptClone {
    pub root: u32,
    pub asset: Option<u32>,
    pub nodes: Vec<ScriptNode>,
    pub components: Vec<(usize, EntityId)>,
}

struct ScriptPrefab {
    prefab: Prefab,
    owners: Vec<usize>,
}

#[derive(Default)]
pub(super) struct ScriptScene {
    nodes: BTreeMap<u32, EntityId>,
    newest: Option<u32>,
    next_child: Option<u32>,
    prefabs: BTreeMap<u32, ScriptPrefab>,
}

impl ScriptScene {
    pub(super) fn successor(&self) -> Self {
        Self {
            next_child: self.next_child,
            ..Self::default()
        }
    }
}

impl Director {
    pub(crate) fn script_reference(&self, id: EntityId) -> Option<u32> {
        self.loaded
            .ids
            .iter()
            .position(|&node| node == id)
            .and_then(|index| u32::try_from(index).ok())
            .or_else(|| {
                self.script_scene
                    .nodes
                    .iter()
                    .find_map(|(&reference, &node)| (node == id).then_some(reference))
            })
    }

    pub(crate) fn script_node(&self, reference: u32) -> Result<EntityId> {
        if reference < FIRST_DYNAMIC_HANDLE {
            self.node(reference as usize)
                .context("Unknown authored node")
        } else {
            self.dynamic_script_node(reference)
        }
    }

    /// Registers a disk-backed prefab in the scene's import catalog.
    pub(crate) fn register_script_prefab(&mut self, path: &Path) -> Result<u32> {
        let index = self.loaded.script_prefabs.len();
        ensure!(
            index < u32::MAX as usize && index < MAX_NODES,
            "Too many script prefabs"
        );
        self.loaded.script_prefabs.push(path.to_path_buf());
        Ok(index as u32)
    }

    pub(crate) fn dynamic_script_node(&self, reference: u32) -> Result<EntityId> {
        self.script_scene
            .nodes
            .get(&reference)
            .copied()
            .context("Unknown script-created node")
    }

    pub(crate) fn create_script_node(
        &mut self,
        reference: u32,
        parent: Option<u32>,
        asset: Option<u32>,
    ) -> Result<ScriptClone> {
        ensure!(
            (FIRST_DYNAMIC_HANDLE..FIRST_CHILD_HANDLE).contains(&reference)
                && self.script_scene.newest.is_none_or(|last| reference > last),
            "Script root handles must increase within their assigned range"
        );
        let parent_id = parent
            .map(|reference| self.script_node(reference))
            .transpose()?;
        let available = MAX_NODES.saturating_sub(self.loaded.scene.node_count());
        let (ids, parents, components) = if let Some(asset) = asset {
            let path = self
                .loaded
                .script_prefabs
                .get(asset as usize)
                .context("Unknown script prefab asset")?;
            let prefab = match self.script_scene.prefabs.entry(asset) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    let file = PrefabFile::load(path)?;
                    let owners = file
                        .nodes
                        .iter()
                        .enumerate()
                        .flat_map(|(index, node)| std::iter::repeat_n(index, node.scripts.len()))
                        .collect();
                    entry.insert(ScriptPrefab {
                        prefab: file.resolve(parent_dir(path), &mut self.assets)?,
                        owners,
                    })
                }
            };
            let template = prefab.prefab.nodes();
            ensure!(template.len() <= available, "Scene has too many nodes");
            let first_child = self.script_scene.next_child.unwrap_or(FIRST_CHILD_HANDLE);
            let end = first_child
                .checked_add(u32::try_from(template.len().saturating_sub(1))?)
                .context("Script descendant reference range exhausted")?;
            let parents = template.iter().map(|node| node.parent).collect::<Vec<_>>();
            let (ids, _) = prefab
                .prefab
                .instantiate(&mut self.loaded.scene, parent_id)?;
            let components = prefab
                .owners
                .iter()
                .map(|&owner| (owner, ids[owner]))
                .collect();
            self.script_scene.next_child = Some(end);
            (ids, parents, components)
        } else {
            ensure!(available > 0, "Scene has too many nodes");
            let (id, _) = self
                .loaded
                .scene
                .spawn("Node", parent_id, Transform2D::default())?;
            (vec![id], vec![None], Vec::new())
        };
        let next_child = self.script_scene.next_child.unwrap_or(FIRST_CHILD_HANDLE);
        let first_child = next_child - u32::try_from(ids.len().saturating_sub(1))?;
        let references: Vec<_> = std::iter::once(reference)
            .chain(first_child..next_child)
            .collect();
        let nodes = ids
            .into_iter()
            .zip(parents)
            .zip(references.iter().copied())
            .map(|((entity, relative_parent), reference)| {
                let parent = relative_parent.map(|index| references[index]).or(parent);
                self.script_scene.nodes.insert(reference, entity);
                ScriptNode {
                    reference,
                    entity,
                    parent,
                }
            })
            .collect();
        self.script_scene.newest = Some(reference);
        Ok(ScriptClone {
            root: reference,
            asset,
            nodes,
            components,
        })
    }

    pub(crate) fn reparent_script_node(
        &mut self,
        reference: u32,
        parent: Option<u32>,
    ) -> Result<EntityId> {
        let node = self.dynamic_script_node(reference)?;
        let parent = parent
            .map(|reference| self.script_node(reference))
            .transpose()?;
        self.loaded.scene.reparent(node, parent)?;
        Ok(node)
    }

    pub(crate) fn script_subtree(&self, root: EntityId) -> Result<Vec<(u32, EntityId)>> {
        let references: BTreeMap<_, _> = self
            .script_scene
            .nodes
            .iter()
            .map(|(&reference, &entity)| (entity, reference))
            .collect();
        Ok(self
            .loaded
            .scene
            .subtree(root)?
            .into_iter()
            .filter_map(|entity| {
                references
                    .get(&entity)
                    .map(|&reference| (reference, entity))
            })
            .collect())
    }

    pub(crate) fn destroy_script_node(&mut self, root: EntityId) -> Result<()> {
        let removed: BTreeSet<_> = self.loaded.scene.subtree(root)?.into_iter().collect();
        self.loaded.scene.remove(root)?;
        self.script_scene
            .nodes
            .retain(|_, entity| !removed.contains(entity));
        Ok(())
    }

    pub(crate) fn script_dynamic_nodes(&self) -> Vec<(u32, EntityId)> {
        self.script_scene
            .nodes
            .iter()
            .map(|(&reference, &entity)| (reference, entity))
            .collect()
    }
}
