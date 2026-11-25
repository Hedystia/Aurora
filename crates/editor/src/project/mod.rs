//! Shared authoring document and index-preserving scene edits.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use engine_core::{
    assets::Assets,
    save::{
        NodeFile, PrefabFile, PropertyValue, SceneFile, SceneKind, ScriptFile, SpriteFile, VERSION,
    },
};

mod jumpy;

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProjectKind {
    #[default]
    TwoD,
    ThreeD,
    TwoDThreeD,
    Jumpy,
}

#[derive(Clone)]
pub(super) struct Project {
    pub path: PathBuf,
    pub root: PathBuf,
    pub document: SceneFile,
    pub selected: Option<usize>,
    pub group_selection: BTreeSet<usize>,
    pub dirty: bool,
    pub revision: u64,
    pub prefab: bool,
}

impl Project {
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_document(path, true)
    }

    fn open_document(path: &Path, allow_new: bool) -> Result<Self> {
        let (root, path) = if path.is_dir() {
            let root = path.canonicalize()?;
            let file = main_scene(&root);
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
        let prefab = is_prefab(&path);
        ensure!(
            prefab || is_scene(&path),
            "Choose a project directory or an Aurora document"
        );
        let document = if prefab {
            let prefab = PrefabFile::load(&path)?;
            SceneFile {
                nodes: prefab.nodes,
                animations: prefab.animations,
                ..empty()
            }
        } else if !allow_new || path.exists() {
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
            group_selection: BTreeSet::new(),
            dirty: false,
            revision: 1,
            prefab,
        })
    }

    pub fn open_existing(path: &Path) -> Result<Self> {
        Self::open_document(path, false)
    }

    pub fn create(path: &Path, kind: ProjectKind) -> Result<Self> {
        ensure!(path.file_name().is_some(), "Choose a new project folder");
        // Claim the directory exclusively before saving any project files.
        fs::create_dir(path)
            .with_context(|| format!("Creating new project folder {}", path.display()))?;
        let result = (|| {
            let mut project = Self::open(path)?;
            project.document.scene_kind = match kind {
                ProjectKind::ThreeD => SceneKind::ThreeD,
                ProjectKind::TwoDThreeD => SceneKind::TwoDThreeD,
                ProjectKind::TwoD | ProjectKind::Jumpy => SceneKind::TwoD,
            };
            if kind == ProjectKind::ThreeD {
                project.document.nodes = default_scene_nodes(SceneKind::ThreeD, "Node 3D");
                project.selected = Some(1);
                project.changed();
            } else if kind == ProjectKind::TwoDThreeD {
                project.document.nodes =
                    default_scene_nodes(SceneKind::TwoDThreeD, crate::strings::CANVAS);
                project.selected = Some(2);
                project.changed();
            }
            let assets = project.root.join("assets");
            fs::create_dir(&assets)?;
            let mut initialize = || -> Result<()> {
                crate::project_types::refresh(&project.root, None)?;
                if kind == ProjectKind::Jumpy {
                    jumpy::create(&project.root)?;
                    project = Self::open_existing(path)?;
                } else {
                    project.save()?;
                }
                Ok(())
            };
            if let Err(error) = initialize() {
                crate::project_types::clean_incomplete_creation(&project.root);
                let _ = fs::remove_dir(assets);
                return Err(error);
            }
            Ok(project)
        })();
        if result.is_err() {
            let _ = fs::remove_dir(path);
        }
        result
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
                animations: self.document.animations.clone(),
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
        self.shift_gameplay(id, 1);
        self.selected = Some(id);
        self.changed();
        id
    }

    pub fn add_to_selection(&mut self, name: &str) -> usize {
        let parent = self
            .selected
            .filter(|&index| index < self.document.nodes.len())
            .or_else(|| (!self.document.nodes.is_empty()).then_some(0));
        self.add(name, parent)
    }

    pub fn group_siblings(&mut self) -> Result<()> {
        let mut selection = self.group_selection.iter().copied();
        let first = selection.next().context("Select sibling nodes to group")?;
        let parent = self.document.nodes[first].parent;
        ensure!(
            self.group_selection.len() >= 2
                && selection.all(|index| self.document.nodes[index].parent == parent),
            "Select at least two nodes with the same parent"
        );
        let group = self.add("Group", parent);
        for index in std::mem::take(&mut self.group_selection) {
            self.document.nodes[index].parent = Some(group);
        }
        self.reorder(&BTreeSet::new());
        Ok(())
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
        self.shift_gameplay(original_len, added);
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

    fn shift_gameplay(&mut self, inline: usize, added: usize) {
        if let Some(gameplay) = &mut self.document.gameplay {
            for index in gameplay
                .sensors
                .iter_mut()
                .map(|sensor| &mut sensor.node)
                .chain(gameplay.frames.iter_mut().map(|frame| &mut frame.node))
            {
                if *index >= inline {
                    *index += added;
                }
            }
        }
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
                        match value {
                            PropertyValue::Node(index)
                            | PropertyValue::Component { node: index, .. } => {
                                let Some(target) = map[*index] else {
                                    return false;
                                };
                                *index = target;
                            }
                            PropertyValue::Nodes(indices) => {
                                *indices = indices.iter().filter_map(|index| map[*index]).collect();
                            }
                            _ => {}
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
        if let Some(gameplay) = &mut self.document.gameplay {
            let new_count = self.document.nodes.len();
            let remap = |index: usize| {
                if index >= original {
                    Some(new_count + index - original)
                } else {
                    map[index]
                }
            };
            gameplay.sensors.retain_mut(|sensor| {
                let Some(node) = remap(sensor.node) else {
                    return false;
                };
                sensor.node = node;
                true
            });
            gameplay.frames.retain_mut(|frame| {
                let Some(node) = remap(frame.node) else {
                    return false;
                };
                frame.node = node;
                true
            });
        }
        self.group_selection = self
            .group_selection
            .iter()
            .filter_map(|index| map[*index])
            .collect();
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
        let node = self.document.nodes.get_mut(index).context("Unknown node")?;
        anyhow::ensure!(
            node.visual.is_none(),
            "Remove the existing visual before assigning a sprite"
        );
        node.sprite = Some(SpriteFile { png: path, size });
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
            "resources",
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

pub(super) fn is_scene(path: &Path) -> bool {
    matches!(extension(path), "scene" | "aurora-scene")
}

pub(super) fn is_prefab(path: &Path) -> bool {
    matches!(extension(path), "prefab" | "aurora-prefab")
}

pub(super) fn is_audio(path: &Path) -> bool {
    matches!(extension(path), "wav" | "mp3" | "ogg" | "flac")
}

pub(super) fn is_frame(path: &Path) -> bool {
    extension(path) == "png"
}

pub(super) fn main_scene(directory: &Path) -> PathBuf {
    let preferred = directory.join("main.scene");
    let legacy = directory.join("main.aurora-scene");
    if !preferred.exists() && legacy.is_file() {
        legacy
    } else {
        preferred
    }
}

pub(super) fn is_main_scene(path: &Path) -> bool {
    path.ends_with("main.scene") || path.ends_with("main.aurora-scene")
}

pub(super) fn supported(path: &Path) -> bool {
    is_scene(path)
        || is_prefab(path)
        || matches!(
            extension(path),
            "png" | "ts" | "wav" | "mp3" | "ttf" | "otf"
        )
}

fn node(name: &str, parent: Option<usize>) -> NodeFile {
    NodeFile {
        name: name.into(),
        parent,
        transform: Default::default(),
        spatial: crate::inspector::spatial::preset(name),
        visual: crate::inspector::visual::preset(name),
        sprite: None,
        appearance: None,
        physics2d: None,
        scripts: Vec::new(),
    }
}

pub(super) fn default_scene_nodes(kind: SceneKind, root_name: &str) -> Vec<NodeFile> {
    let mut root = node(root_name, None);
    match kind {
        SceneKind::TwoD => vec![root],
        SceneKind::ThreeD => {
            root.spatial = crate::inspector::spatial::preset("Node 3D");
            vec![
                root,
                node("Cube 3D", Some(0)),
                node("Camera 3D", Some(0)),
                node("Directional Light", Some(0)),
            ]
        }
        SceneKind::TwoDThreeD => vec![
            root,
            node("Node 3D", Some(0)),
            node("Cube 3D", Some(1)),
            node("Camera 3D", Some(1)),
            node("Directional Light", Some(1)),
        ],
    }
}

fn empty() -> SceneFile {
    SceneFile {
        version: VERSION,
        scene_kind: SceneKind::TwoD,
        nodes: Vec::new(),
        instances: Vec::new(),
        script_prefabs: Vec::new(),
        animations: Vec::new(),
        autoplay: None,
        gameplay: None,
        display: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_core::animation::{Clip, Interpolation, Key, Property, Track, Wrap};

    #[test]
    fn grouping_preserves_sibling_poses_and_references() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let mut project = Project::open(directory.path())?;
        let left = project.add("Left", Some(0));
        let right = project.add("Right", Some(0));
        let grandchild = project.add("Grandchild", Some(left));
        project.group_selection.extend([right, grandchild]);
        assert!(project.group_siblings().is_err());
        assert_eq!(project.document.nodes.len(), 4);
        project.document.nodes[left].transform.position = [-12.0, 3.0];
        project.attach(left, &directory.path().join("Mover.ts"), "Mover")?;
        project.document.nodes[left].scripts[0].values.insert(
            "targets".into(),
            PropertyValue::Nodes(vec![right, grandchild]),
        );
        project.group_selection = BTreeSet::from([left, right]);
        project.group_siblings()?;
        project.save()?;
        let reopened = Project::open(&project.path)?;
        let nodes = &reopened.document.nodes;
        assert_eq!(nodes[1].name, "Group");
        assert_eq!(nodes[2].parent, Some(1));
        assert_eq!(nodes[4].parent, Some(1));
        assert_eq!(nodes[2].transform.position, [-12.0, 3.0]);
        assert!(
            matches!(&nodes[2].scripts[0].values["targets"], PropertyValue::Nodes(targets) if targets == &[4,3])
        );
        Ok(())
    }

    #[test]
    fn adding_from_a_node_palette_uses_the_selected_node_or_scene_root() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let mut project = Project::open(directory.path())?;
        let root = project.add_to_selection("Sprite");
        assert_eq!(project.document.nodes[root].parent, Some(0));

        let child = project.add_to_selection("Text");
        assert_eq!(project.document.nodes[child].parent, Some(root));

        project.selected = None;
        let fallback = project.add_to_selection("Empty Node");
        assert_eq!(project.document.nodes[fallback].parent, Some(0));
        Ok(())
    }

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
                    tangents: None,
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
