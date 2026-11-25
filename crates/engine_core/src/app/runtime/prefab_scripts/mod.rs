//! Installs dynamically created prefab bindings after a script command batch.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use script_api::{BINDING_ENABLED, Kind};

use crate::{director::ScriptClone, ecs::EntityId};

use super::{Authored, Binding};

pub(super) struct RuntimeBinding {
    pub(super) node: EntityId,
    pub(super) lifecycle: Binding,
    dynamic: bool,
}

impl RuntimeBinding {
    pub(super) fn new(node: EntityId, dynamic: bool) -> Self {
        Self {
            node,
            lifecycle: Binding::default(),
            dynamic,
        }
    }
}

pub(super) enum Mutation {
    Attach(ScriptClone),
    AddComponent {
        node: EntityId,
        reference: u32,
        factory: u32,
    },
    RemoveComponent {
        binding: u32,
    },
    Reparent {
        reference: u32,
        node: EntityId,
        parent: Option<u32>,
    },
    Detach {
        reference: u32,
        node: EntityId,
    },
}

impl Authored {
    pub(super) fn control(
        &mut self,
        kind: Kind,
        binding: u32,
        node: EntityId,
        coordinates: [f64; 2],
        detail: u64,
    ) -> Result<bool> {
        let reply = self
            .script
            .as_mut()
            .context("Script disconnected")?
            .exchange_control(
                kind,
                binding,
                node.value(),
                [coordinates[0], coordinates[1], 0.0],
                detail,
            )?;
        ensure!(
            reply.command.is_none(),
            "Prefab control messages require an acknowledgement"
        );
        Ok(reply.enabled)
    }

    pub(super) fn settle_mutations(&mut self) -> Result<()> {
        // Teardown callbacks may queue more removals. Bound one batch even if
        // a script continually schedules another structural change.
        for _ in 0..4096 {
            match self.mutations.pop_front() {
                Some(Mutation::Attach(clone)) => self.attach_clone(clone)?,
                Some(Mutation::AddComponent {
                    node,
                    reference,
                    factory,
                }) => {
                    ensure!(
                        self.bindings.len() < 100_000,
                        "Scene has too many component bindings"
                    );
                    let binding = self.next_binding;
                    ensure!(
                        binding < BINDING_ENABLED,
                        "Component binding range exhausted"
                    );
                    self.next_binding += 1;
                    let enabled = self.control(
                        Kind::RegisterComponent,
                        binding,
                        node,
                        [f64::from(reference), f64::from(factory)],
                        0,
                    )?;
                    let mut entry = RuntimeBinding::new(node, true);
                    entry.lifecycle.enabled = enabled;
                    self.bindings.insert(binding, entry);
                }
                // Authored attachments and runtime factories share the same
                // teardown path; only dynamic bindings need a release message.
                Some(Mutation::RemoveComponent { binding }) => {
                    if self.bindings.contains_key(&binding) {
                        self.tearing_down = true;
                        let result = self.release_bindings(&[binding]);
                        self.tearing_down = false;
                        result?;
                    }
                }
                Some(Mutation::Reparent {
                    reference,
                    node,
                    parent,
                }) => {
                    if self.director.dynamic_script_node(reference).is_ok() {
                        self.control(
                            Kind::UpdatePrefabParent,
                            0,
                            node,
                            [wire_id(parent), 0.0],
                            u64::from(reference),
                        )?;
                    }
                }
                Some(Mutation::Detach { reference, node }) => {
                    if self.director.dynamic_script_node(reference).is_ok() {
                        self.detach_clone(node)?;
                    }
                }
                None => return Ok(()),
            }
        }
        ensure!(
            self.mutations.is_empty(),
            "Prefab mutation batch limit exceeded"
        );
        Ok(())
    }

    fn attach_clone(&mut self, clone: ScriptClone) -> Result<()> {
        for node in clone.nodes {
            let detail = (u64::from(clone.root) << 32) | u64::from(node.reference);
            self.control(
                Kind::RegisterPrefabNode,
                0,
                node.entity,
                [wire_id(clone.asset), wire_id(node.parent)],
                detail,
            )?;
        }

        let Some(asset) = clone.asset else {
            return Ok(());
        };
        for (ordinal, (owner, node)) in clone.components.into_iter().enumerate() {
            ensure!(
                self.bindings.len() < 100_000,
                "Scene has too many component bindings"
            );
            let binding = self.next_binding;
            ensure!(
                binding < BINDING_ENABLED,
                "Component binding range exhausted"
            );
            self.next_binding += 1;
            let detail = (u64::from(asset) << 32) | u64::from(clone.root);
            let enabled = self.control(
                Kind::RegisterPrefabComponent,
                binding,
                node,
                [
                    f64::from(u32::try_from(ordinal)?),
                    f64::from(u32::try_from(owner)?),
                ],
                detail,
            )?;
            let mut entry = RuntimeBinding::new(node, true);
            entry.lifecycle.enabled = enabled;
            self.bindings.insert(binding, entry);
        }
        Ok(())
    }

    fn detach_clone(&mut self, node: EntityId) -> Result<()> {
        let removed = self.director.script_subtree(node)?;
        let nodes: BTreeSet<_> = removed.iter().map(|&(_, node)| node).collect();
        let mut by_owner = BTreeMap::<EntityId, Vec<u32>>::new();
        for (&binding, entry) in self.bindings.iter().rev() {
            if nodes.contains(&entry.node) {
                by_owner.entry(entry.node).or_default().push(binding);
            }
        }
        let bindings: Vec<_> = removed
            .iter()
            .filter_map(|(_, node)| by_owner.remove(node))
            .flatten()
            .collect();

        self.tearing_down = true;
        let lifecycle = self.release_bindings(&bindings);
        self.tearing_down = false;
        lifecycle?;
        for (reference, node) in removed {
            self.control(
                Kind::ReleasePrefabNode,
                0,
                node,
                [0.0; 2],
                u64::from(reference),
            )?;
        }
        self.director.destroy_script_node(node)
    }

    pub(super) fn release_bindings(&mut self, bindings: &[u32]) -> Result<()> {
        for &binding in bindings {
            if let Some(entry) = self.bindings.get_mut(&binding)
                && let Some(kind) = entry.lifecycle.change_activation(false)
            {
                let owner = entry.node;
                self.dispatch_raw(binding, owner, kind, 0, None)?;
            }
        }
        for &binding in bindings {
            let Some(entry) = self.bindings.get_mut(&binding) else {
                continue;
            };
            let node = entry.node;
            let dynamic = entry.dynamic;
            if let Some(kind) = entry.lifecycle.destroy() {
                self.dispatch_raw(binding, node, kind, 0, None)?;
            }
            self.bindings.remove(&binding);
            if dynamic {
                self.control(Kind::ReleasePrefabComponent, binding, node, [0.0; 2], 0)?;
            }
        }
        Ok(())
    }
}

fn wire_id(reference: Option<u32>) -> f64 {
    reference.map_or(-1.0, f64::from)
}
