//! Shared authoring document and index-preserving scene edits.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use engine_core::{
    assets::Assets,
    save::{NodeFile, PrefabFile, PropertyValue, SceneFile, ScriptFile, SpriteFile, VERSION},
};

#[derive(Clone)]
pub(super) struct Project {
    pub path: PathBuf,
    pub root: PathBuf,
    pub document: SceneFile,
    pub selected: Option<usize>,
    pub dirty: bool,
    pub revision: u64,
    pub prefab: bool,
}

impl Project {
    pub fn open(path: &Path) -> Result<Self> {
        let (root, path) = if path.is_dir() {
            let root = path.canonicalize()?;
            let file = root.join("main.aurora-scene");
            (root, file)
        } else {
            let root = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
                .canonicalize()?;
            let file = root.join(path.file_name().context("Scene filename is missing")?);
            (root, file)
        };
        let prefab = extension(&path) == "aurora-prefab";
        ensure!(
            prefab || extension(&path) == "aurora-scene",
            "Choose a project directory or an Aurora document"
        );
        let document = if prefab {
            SceneFile {
                nodes: PrefabFile::load(&path)?.nodes,
                ..empty()
            }
        } else if path.exists() {
            SceneFile::load(&path)?
        } else {
            SceneFile {
                nodes: vec![node(crate::strings::CANVAS, None)],
                ..empty()
            }
        };
        let selected = (!document.nodes.is_empty()).then_some(0);
        Ok(Self {
            path,
            root,
            document,
            selected,
            dirty: false,
            revision: 1,
            prefab,
        })
    }

    pub fn changed(&mut self) {
        self.dirty = true;
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn save(&mut self) -> Result<()> {
        if self.prefab {
            PrefabFile {
                version: VERSION,
                nodes: self.document.nodes.clone(),
            }
            .save(&self.path)?;
        } else {
            self.document.save(&self.path)?;
        }
        self.dirty = false;
        Ok(())
    }

    pub fn add(&mut self, name: &str, parent: Option<usize>) -> usize {
        let id = self.document.nodes.len();
        let parent = if self.prefab {
            parent.or(Some(0))
        } else {
            parent
        };
        self.document.nodes.push(node(name, parent));
        for track in self
            .document
            .animations
            .iter_mut()
            .flat_map(|clip| &mut clip.tracks)
        {
            if track.node >= id {
                track.node += 1;
            }
        }
        self.selected = Some(id);
        self.changed();
        id
    }

    pub fn reparent(&mut self, id: usize, parent: Option<usize>) -> Result<()> {
        ensure!(id < self.document.nodes.len(), "Unknown node");
        ensure!(!(id == 0 && self.prefab), "Cannot reparent the prefab root");
        let parent = if self.prefab {
            parent.or(Some(0))
        } else {
            parent
        };
        let mut cursor = parent;
        while let Some(index) = cursor {
            ensure!(index != id, "A node cannot be parented to its own subtree");
            cursor = self
                .document
                .nodes
                .get(index)
                .context("Unknown parent")?
                .parent;
        }
        if self.document.nodes[id].parent != parent {
            self.document.nodes[id].parent = parent;
            self.reorder(&BTreeSet::new());
        }
        Ok(())
    }

    pub fn duplicate(&mut self, id: usize) -> Result<()> {
        ensure!(id < self.document.nodes.len(), "Unknown node");
        ensure!(
            !(self.prefab && id == 0),
            "Cannot duplicate the prefab root"
        );
        let members = self.subtree(id);
        let original_len = self.document.nodes.len();
        let mut remap = vec![None; original_len];
        for (offset, &index) in members.iter().enumerate() {
            remap[index] = Some(original_len + offset);
        }
        for index in members {
            let mut copy = self.document.nodes[index].clone();
            if index == id {
                copy.name.push_str(crate::strings::COPY_SUFFIX);
            }
            if let Some(parent) = copy.parent {
                copy.parent = remap[parent].or(Some(parent));
            }
            for script in &mut copy.scripts {
                for value in script.values.values_mut() {
                    if let PropertyValue::Node(target) = value
                        && let Some(mapped) = remap[*target]
                    {
                        *target = mapped;
                    }
                }
            }
            self.document.nodes.push(copy);
        }
        // Animation references to resolved prefab instances live after the inline nodes.
        let added = self.document.nodes.len() - original_len;
        for clip in &mut self.document.animations {
            for track in &mut clip.tracks {
                if track.node >= original_len {
                    track.node += added;
                }
            }
        }
        self.selected = Some(original_len);
        self.changed();
        Ok(())
    }

    pub fn remove(&mut self, id: usize) -> Result<()> {
        ensure!(id < self.document.nodes.len(), "Unknown node");
        ensure!(!(self.prefab && id == 0), "Cannot remove the prefab root");
        self.reorder(&self.subtree(id));
        Ok(())
    }

    fn subtree(&self, id: usize) -> BTreeSet<usize> {
        let mut set = BTreeSet::from([id]);
        for (index, node) in self.document.nodes.iter().enumerate().skip(id + 1) {
            if node.parent.is_some_and(|parent| set.contains(&parent)) {
                set.insert(index);
            }
        }
        set
    }

    fn reorder(&mut self, removed: &BTreeSet<usize>) {
        let original = self.document.nodes.len();
        let mut children = vec![Vec::new(); original + 1];
        for (index, node) in self.document.nodes.iter().enumerate() {
            if !removed.contains(&index) {
                children[node.parent.unwrap_or(original)].push(index);
            }
        }
        let mut pending: Vec<_> = children[original].iter().rev().copied().collect();
        let mut order = Vec::new();
        let mut map = vec![None; original];
        while let Some(index) = pending.pop() {
            map[index] = Some(order.len());
            order.push(index);
            pending.extend(children[index].iter().rev().copied());
        }
        let mut old: Vec<_> = std::mem::take(&mut self.document.nodes)
            .into_iter()
            .map(Some)
            .collect();
        self.document.nodes = order
            .into_iter()
            .filter_map(|index| old[index].take())
            .map(|mut node| {
                node.parent = node.parent.and_then(|parent| map[parent]);
                for script in &mut node.scripts {
                    script.values.retain(|_, value| {
                        if let PropertyValue::Node(index) = value {
                            if let Some(target) = map[*index] {
                                *index = target;
                            } else {
                                return false;
                            }
                        }
                        true
                    });
                }
                node
            })
            .collect();
        // Keep asset instances rooted if their original parent is removed.
        for instance in &mut self.document.instances {
            instance.parent = instance.parent.and_then(|index| map[index]);
        }
        for clip in &mut self.document.animations {
            clip.tracks.retain_mut(|track| {
                if track.node >= original {
                    track.node = self.document.nodes.len() + (track.node - original);
                } else if let Some(index) = map[track.node] {
                    track.node = index;
                } else {
                    return false;
                }
                true
            });
        }
        self.selected = self.selected.and_then(|index| map[index]);
        self.changed();
    }

    pub fn drop_sprite(&mut self, node: Option<usize>, path: &Path) -> Result<()> {
        let path = self.relative(path);
        let mut assets = Assets::new(None);
        let id = assets.import_file(&self.root.join(&path))?;
        let texture = assets.texture(id).context("Texture unavailable")?;
        let size = [texture.width() as f32, texture.height() as f32];
        let index = node.unwrap_or_else(|| self.add(crate::strings::SPRITE_NODE, None));
        self.document
            .nodes
            .get_mut(index)
            .context("Unknown node")?
            .sprite = Some(SpriteFile { png: path, size });
        self.changed();
        Ok(())
    }

    pub fn attach(&mut self, index: usize, path: &Path, component: &str) -> Result<()> {
        let path = self.relative(path);
        let node = self.document.nodes.get_mut(index).context("Unknown node")?;
        ensure!(
            !node
                .scripts
                .iter()
                .any(|script| script.path == path && script.component == component),
            "Component already attached"
        );
        node.scripts.push(ScriptFile {
            path,
            component: component.into(),
            values: Default::default(),
        });
        self.changed();
        Ok(())
    }

    pub fn relative(&self, asset: &Path) -> PathBuf {
        asset
            .strip_prefix(&self.root)
            .unwrap_or(asset)
            .to_path_buf()
    }

    pub fn assets(&self) -> Vec<PathBuf> {
        let mut results = Vec::new();
        for directory in [
            "assets",
            "scenes",
            "prefabs",
            "scripts",
            "animations",
            "audio",
        ] {
            let path = self.root.join(directory);
            if path.is_dir() {
                results.push(path.clone());
                scan(&path, &mut results);
            }
        }
        if let Ok(entries) = fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if entry.file_type().is_ok_and(|kind| kind.is_file()) && supported(&path) {
                    results.push(path);
                }
            }
        }
        results.sort();
        results.dedup();
        results
    }
}

fn scan(path: &Path, results: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_dir() && !entry.file_name().to_string_lossy().starts_with('.') {
            results.push(path.clone());
            scan(&path, results);
        }
        if kind.is_file() && supported(&path) {
            results.push(path);
        }
    }
}

pub(super) fn extension(path: &Path) -> &str {
    path.extension()
        .and_then(|part| part.to_str())
        .unwrap_or("")
}

pub(super) fn supported(path: &Path) -> bool {
    matches!(
        extension(path),
        "png" | "ts" | "wav" | "mp3" | "ttf" | "aurora-scene" | "aurora-prefab"
    )
}

fn node(name: &str, parent: Option<usize>) -> NodeFile {
    NodeFile {
        name: name.into(),
        parent,
        transform: Default::default(),
        sprite: None,
        appearance: None,
        scripts: Vec::new(),
    }
}

fn empty() -> SceneFile {
    SceneFile {
        version: VERSION,
        nodes: Vec::new(),
        instances: Vec::new(),
        animations: Vec::new(),
        autoplay: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_core::animation::{Clip, Interpolation, Key, Property, Track, Wrap};

    #[test]
    fn hierarchy_edits_preserve_attachments_and_animation_bindings() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let mut project = Project::open(directory.path())?;
        let parent = project.add("Parent", Some(0));
        let child = project.add("Child", Some(parent));
        let destination = project.add("Destination", Some(0));
        project.attach(parent, &directory.path().join("Mover.ts"), "Mover")?;
        project.document.nodes[parent].scripts[0]
            .values
            .insert("target".into(), PropertyValue::Node(child));
        project.document.animations.push(Clip {
            name: "Track".into(),
            sample_rate: 60,
            duration: 30,
            speed: 1.0,
            wrap: Wrap::Once,
            tracks: vec![Track {
                node: child,
                property: Property::X,
                interpolation: Interpolation::Linear,
                keys: vec![Key {
                    frame: 0,
                    value: 3.0,
                }],
            }],
            events: Vec::new(),
        });
        assert!(project.reparent(parent, Some(child)).is_err());
        project.reparent(parent, Some(destination))?;
        assert_eq!(project.document.nodes[2].parent, Some(1));
        assert_eq!(project.document.nodes[3].parent, Some(2));
        assert_eq!(project.document.animations[0].tracks[0].node, 3);
        assert!(matches!(
            project.document.nodes[2].scripts[0].values["target"],
            PropertyValue::Node(3)
        ));
        project.duplicate(2)?;
        assert!(matches!(
            project.document.nodes[4].scripts[0].values["target"],
            PropertyValue::Node(5)
        ));
        project.remove(3)?;
        assert!(project.document.nodes[2].scripts[0].values.is_empty());
        assert!(project.document.animations[0].tracks.is_empty());
        project.save()?;
        assert!(!project.dirty);
        assert_eq!(Project::open(&project.path)?.document.nodes.len(), 5);
        Ok(())
    }
}
