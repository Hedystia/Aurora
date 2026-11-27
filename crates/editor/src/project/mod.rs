//! Shared authoring document and index-preserving scene edits.

use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use engine_core::{
    assets::Assets,
    save::{
        CameraSelection, NodeFile, PrefabFile, PropertyValue, SceneFile, SceneKind, ScriptFile,
        SpriteFile, VERSION,
    },
    scene::spatial::{Body, JointFile, JointKind, Role},
};
use serde::{Deserialize, Serialize};

mod jumpy;

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProjectKind {
    #[default]
    TwoD,
    ThreeD,
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
        Self::open_document(path, true, None)
    }

    /// Opens a scene or prefab while keeping the current project root and FileSystem.
    pub(super) fn open_in_root(path: &Path, root: &Path) -> Result<Self> {
        Self::open_document(path, false, Some(root))
    }

    fn open_document(path: &Path, allow_new: bool, root_override: Option<&Path>) -> Result<Self> {
        let (root, path) = if path.is_dir() {
            let root = path.canonicalize()?;
            let file = startup_scene(&root);
            (root, file)
        } else if let Some(root) = root_override {
            let path = path.to_path_buf();
            ensure!(path.starts_with(root), "Document escapes the project");
            (root.to_path_buf(), path)
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
        Self::open_document(path, false, None)
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
                ProjectKind::TwoD | ProjectKind::Jumpy => SceneKind::TwoD,
            };
            if kind == ProjectKind::ThreeD {
                project.document.nodes = default_scene_nodes(SceneKind::ThreeD, "Node 3D");
                project.document.camera = default_camera(&project.document.nodes);
                project.selected = Some(1);
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
                joints: Vec::new(),
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
        if let Some(parent) = parent {
            self.apply_auto_layout(parent);
        }
        id
    }

    /// Adds a fixed joint between the first two spatial bodies in document order.
    pub fn add_joint(&mut self) -> Option<usize> {
        let endpoints: Vec<_> = self
            .document
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| {
                node.spatial
                    .as_ref()
                    .is_some_and(|spatial| spatial.body != Body::None)
            })
            .map(|(index, _)| index)
            .take(2)
            .collect();
        let [a, b] = endpoints.as_slice() else {
            return None;
        };
        let index = self.document.joints.len();
        self.document.joints.push(JointFile {
            a: *a,
            b: *b,
            kind: JointKind::Fixed,
            anchor_a: [0.; 3],
            anchor_b: [0.; 3],
            axis: [0., 1., 0.],
        });
        self.changed();
        Some(index)
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

    /// Authored size of a node in design pixels, used by design alignment tools.
    pub(super) fn node_size(node: &NodeFile) -> [f32; 2] {
        if let Some(sprite) = &node.sprite {
            return sprite.size;
        }
        match &node.visual {
            Some(engine_core::save::visual::Visual::Design(design)) => design.size,
            Some(engine_core::save::visual::Visual::NineSlice { size, .. }) => {
                [size[0] as f32, size[1] as f32]
            }
            Some(engine_core::save::visual::Visual::Atlas { cell, .. }) => {
                [cell[0] as f32, cell[1] as f32]
            }
            _ => [0.0, 0.0],
        }
    }

    /// Applies a frame's auto layout to its direct children.
    pub(super) fn apply_auto_layout(&mut self, index: usize) {
        let Some(layout) = self
            .document
            .nodes
            .get(index)
            .and_then(|node| match &node.visual {
                Some(engine_core::save::visual::Visual::Design(design)) => {
                    design.layout.enabled.then_some(design.layout)
                }
                _ => None,
            })
        else {
            return;
        };
        let parent_size = Self::node_size(&self.document.nodes[index]);
        let children: Vec<usize> = self
            .document
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.parent == Some(index))
            .map(|(child, _)| child)
            .collect();
        if children.is_empty() {
            return;
        }
        // A frame's origin is its centre, and each child sprite is centred on
        // its own origin. Positions are therefore in frame-local coordinates
        // starting at the frame's top-left corner, not at its centre.
        let left = -parent_size[0] * 0.5;
        let top = -parent_size[1] * 0.5;
        let mut cursor = if layout.horizontal {
            left + layout.padding[0]
        } else {
            top + layout.padding[1]
        };
        for child in children {
            let size = Self::node_size(&self.document.nodes[child]);
            let node = &mut self.document.nodes[child].transform;
            if layout.horizontal {
                let usable = parent_size[1] - layout.padding[1] - layout.padding[3] - size[1];
                let cross = top + layout.padding[1] + usable.max(0.0) * layout.align;
                node.position = [cursor + size[0] * 0.5, cross + size[1] * 0.5];
                cursor += size[0] + layout.gap;
            } else {
                let usable = parent_size[0] - layout.padding[0] - layout.padding[2] - size[0];
                let cross = left + layout.padding[0] + usable.max(0.0) * layout.align;
                node.position = [cross + size[0] * 0.5, cursor + size[1] * 0.5];
                cursor += size[1] + layout.gap;
            }
        }
        self.changed();
    }

    /// Centres the selected node in its parent, or on the design/3D origin.
    pub(super) fn center_selection(&mut self, in_parent: bool) {
        let Some(index) = self.selected else {
            return;
        };
        if in_parent && self.document.nodes[index].parent.is_none() {
            return;
        }
        self.document.nodes[index].transform.position = [0.0; 2];
        if let Some(spatial) = &mut self.document.nodes[index].spatial {
            spatial.position = [0.0; 3];
        }
        if !in_parent {
            self.apply_auto_layout(index);
        }
        self.changed();
    }

    /// Aligns the group selection on one axis.
    pub(super) fn align_selection(&mut self, horizontal: bool, align_end: bool, center: bool) {
        let indices: Vec<usize> = self.group_selection.iter().copied().collect();
        if indices.len() < 2 {
            return;
        }
        let axis = usize::from(!horizontal);
        let mut bounds: Vec<(usize, f32, f32)> = indices
            .iter()
            .map(|&index| {
                let position = self.document.nodes[index].transform.position[axis];
                let size = Self::node_size(&self.document.nodes[index])[axis];
                (index, position - size * 0.5, position + size * 0.5)
            })
            .collect();
        let target = if center {
            let low = bounds
                .iter()
                .map(|(_, low, _)| *low)
                .fold(f32::INFINITY, f32::min);
            let high = bounds
                .iter()
                .map(|(_, _, high)| *high)
                .fold(f32::NEG_INFINITY, f32::max);
            (low + high) * 0.5
        } else if align_end {
            bounds
                .iter()
                .map(|(_, _, high)| *high)
                .fold(f32::NEG_INFINITY, f32::max)
        } else {
            bounds
                .iter()
                .map(|(_, low, _)| *low)
                .fold(f32::INFINITY, f32::min)
        };
        for (index, low, high) in &mut bounds {
            let size = *high - *low;
            let position = if center || align_end {
                target - size * 0.5
            } else {
                target + size * 0.5
            };
            self.document.nodes[*index].transform.position[axis] = position;
        }
        self.changed();
    }

    /// Evenly distributes the group selection on one axis.
    pub(super) fn distribute_selection(&mut self, horizontal: bool) {
        let mut indices: Vec<usize> = self.group_selection.iter().copied().collect();
        if indices.len() < 3 {
            return;
        }
        let axis = usize::from(!horizontal);
        indices.sort_by(|a, b| {
            self.document.nodes[*a].transform.position[axis]
                .total_cmp(&self.document.nodes[*b].transform.position[axis])
        });
        let (Some(&first), Some(&last)) = (indices.first(), indices.last()) else {
            return;
        };
        let start = self.document.nodes[first].transform.position[axis];
        let end = self.document.nodes[last].transform.position[axis];
        let step = (end - start) / (indices.len() - 1) as f32;
        for (slot, index) in indices.iter().enumerate() {
            self.document.nodes[*index].transform.position[axis] = start + step * slot as f32;
        }
        self.changed();
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
        let new_count = self.document.nodes.len();
        let remap = |index: usize| {
            if index >= original {
                Some(new_count + index - original)
            } else {
                map[index]
            }
        };
        if let CameraSelection::Node(index) = self.document.camera {
            self.document.camera =
                remap(index).map_or(CameraSelection::Auto, CameraSelection::Node);
        }
        self.document.joints.retain_mut(|joint| {
            let (Some(a), Some(b)) = (remap(joint.a), remap(joint.b)) else {
                return false;
            };
            if a == b {
                return false;
            }
            joint.a = a;
            joint.b = b;
            true
        });
        if let Some(gameplay) = &mut self.document.gameplay {
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

/// Per-project editing state. `last_scene` is restored before `default_scene`.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct ProjectSession {
    pub version: u32,
    pub default_scene: String,
    pub last_scene: String,
}

fn session_path(root: &Path) -> PathBuf {
    root.join(".aurora/project-session.json")
}

pub(super) fn load_session(root: &Path) -> ProjectSession {
    let mut session: ProjectSession = fs::read(session_path(root))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    if session.version == 0 {
        session.version = 1;
    }
    session
}

pub(super) fn save_session(root: &Path, session: &ProjectSession) -> Result<()> {
    let directory = root.join(".aurora");
    fs::create_dir_all(&directory)?;
    let mut file = tempfile::NamedTempFile::new_in(&directory)?;
    file.write_all(&serde_json::to_vec_pretty(session)?)?;
    file.write_all(b"\n")?;
    file.persist(session_path(root))?;
    Ok(())
}

pub(super) fn valid_scene_ref(root: &Path, reference: &str) -> Option<PathBuf> {
    if reference.is_empty() || Path::new(reference).is_absolute() {
        return None;
    }
    let path = root.join(reference);
    if !is_scene(&path)
        || !path.starts_with(root)
        || reference.split(['/', '\\']).any(|part| part == "..")
        || !path.is_file()
    {
        return None;
    }
    Some(path)
}

fn startup_scene(root: &Path) -> PathBuf {
    let session = load_session(root);
    valid_scene_ref(root, &session.last_scene)
        .or_else(|| valid_scene_ref(root, &session.default_scene))
        .unwrap_or_else(|| main_scene(root))
}

/// Project-relative paths of every scene, sorted for stable play order.
pub(super) fn scene_references(root: &Path) -> Vec<String> {
    fn visit(directory: &Path, root: &Path, results: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if !path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with('.'))
                {
                    visit(&path, root, results);
                }
            } else if is_scene(&path)
                && let Ok(relative) = path.strip_prefix(root)
            {
                results.push(relative.to_string_lossy().into_owned());
            }
        }
    }
    let mut results = Vec::new();
    visit(root, root, &mut results);
    results.sort();
    results
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

/// Selects the first camera in a starter scene, or automatic selection when there is none.
pub(super) fn default_camera(nodes: &[NodeFile]) -> CameraSelection {
    nodes
        .iter()
        .position(|node| {
            node.spatial
                .as_ref()
                .is_some_and(|spatial| spatial.role == Role::Camera)
        })
        .map_or(CameraSelection::Auto, CameraSelection::Node)
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
    }
}

fn empty() -> SceneFile {
    SceneFile {
        version: VERSION,
        camera: CameraSelection::Auto,
        scene_kind: SceneKind::TwoD,
        nodes: Vec::new(),
        instances: Vec::new(),
        script_prefabs: Vec::new(),
        animations: Vec::new(),
        autoplay: None,
        gameplay: None,
        display: None,
        joints: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_core::animation::{Clip, Interpolation, Key, Property, Track, Wrap};

    #[test]
    fn scene_references_list_project_scenes_in_stable_order() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let root = scratch.path().canonicalize()?;
        let levels = root.join("levels");
        fs::create_dir(&levels)?;
        fs::write(root.join("main.scene"), r#"{"version":1,"nodes":[]}"#)?;
        fs::write(levels.join("Two.scene"), r#"{"version":1,"nodes":[]}"#)?;
        fs::write(root.join("notes.txt"), "ignored")?;
        assert_eq!(
            scene_references(&root),
            vec!["levels/Two.scene".to_owned(), "main.scene".to_owned()]
        );
        Ok(())
    }

    #[test]
    fn starter_scenes_select_their_camera() {
        let two_d = default_scene_nodes(SceneKind::TwoD, "Canvas");
        assert_eq!(default_camera(&two_d), CameraSelection::Auto);
        let three_d = default_scene_nodes(SceneKind::ThreeD, "Node 3D");
        assert_eq!(default_camera(&three_d), CameraSelection::Node(2));
    }

    #[test]
    fn auto_layout_and_alignment_arrange_design_children() {
        let mut project = Project {
            path: PathBuf::new(),
            root: PathBuf::new(),
            document: empty(),
            selected: None,
            group_selection: BTreeSet::new(),
            dirty: false,
            revision: 0,
            prefab: false,
        };
        let frame = project.document.nodes.len();
        project.document.nodes.push(NodeFile {
            name: "Frame".into(),
            parent: None,
            transform: Default::default(),
            spatial: None,
            visual: Some(engine_core::save::visual::Visual::Design(
                engine_core::save::visual::design::Design {
                    size: [200.0, 100.0],
                    layout: engine_core::save::visual::design::AutoLayout {
                        enabled: true,
                        horizontal: true,
                        gap: 10.0,
                        padding: [8.0, 8.0, 8.0, 8.0],
                        align: 0.0,
                    },
                    ..Default::default()
                },
            )),
            sprite: None,
            appearance: None,
            physics2d: None,
            scripts: Vec::new(),
        });
        for index in 0..2 {
            project.document.nodes.push(NodeFile {
                name: format!("Child {index}"),
                parent: Some(frame),
                transform: Default::default(),
                spatial: None,
                visual: Some(engine_core::save::visual::Visual::Design(
                    engine_core::save::visual::design::Design {
                        size: [40.0, 20.0],
                        ..Default::default()
                    },
                )),
                sprite: None,
                appearance: None,
                physics2d: None,
                scripts: Vec::new(),
            });
        }
        project.apply_auto_layout(frame);
        assert_eq!(
            project.document.nodes[frame + 1].transform.position[0],
            -72.0
        );
        assert_eq!(
            project.document.nodes[frame + 2].transform.position[0],
            -22.0
        );

        project.group_selection.insert(frame + 1);
        project.group_selection.insert(frame + 2);
        project.align_selection(true, false, true);
        assert_eq!(
            project.document.nodes[frame + 1].transform.position[0],
            project.document.nodes[frame + 2].transform.position[0]
        );
    }

    #[test]
    fn scenes_opened_inside_the_project_keep_the_project_root() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let root = scratch.path().canonicalize()?;
        let folder = root.join("levels");
        fs::create_dir(&folder)?;
        fs::write(folder.join("Two.scene"), r#"{"version":1,"nodes":[]}"#)?;
        let project = Project::open_in_root(&folder.join("Two.scene"), &root)?;
        assert_eq!(project.root, root);
        assert_eq!(project.path, folder.join("Two.scene"));
        assert!(Project::open_in_root(&folder.join("Missing.scene"), &root).is_err());
        Ok(())
    }

    #[test]
    fn session_scene_overrides_default_and_falls_back_to_main() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let root = scratch.path().canonicalize()?;
        for name in ["main.scene", "level.scene", "other.scene"] {
            fs::write(root.join(name), r#"{"version":1,"nodes":[]}"#)?;
        }
        let mut session = ProjectSession {
            version: 1,
            default_scene: "level.scene".into(),
            last_scene: String::new(),
        };
        save_session(&root, &session)?;
        assert_eq!(Project::open(&root)?.path, root.join("level.scene"));

        session.last_scene = "other.scene".into();
        save_session(&root, &session)?;
        assert_eq!(Project::open(&root)?.path, root.join("other.scene"));

        session.last_scene = "../escape.scene".into();
        save_session(&root, &session)?;
        assert_eq!(Project::open(&root)?.path, root.join("level.scene"));
        Ok(())
    }

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
    fn scene_save_round_trips_joints_and_node_removal_rebases_them() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let mut project = Project::open(directory.path())?;
        let mut bodies = Vec::new();
        for _ in 0..3 {
            let node = project.add("Cube 3D", Some(0));
            project.document.nodes[node]
                .spatial
                .as_mut()
                .context("cube has a spatial component")?
                .body = Body::Fixed;
            bodies.push(node);
        }
        assert_eq!(project.add_joint(), Some(0));
        project.document.joints[0].kind = JointKind::Hinge;
        project.document.joints.push(JointFile {
            a: bodies[1],
            b: bodies[2],
            kind: JointKind::Slider,
            anchor_a: [0.; 3],
            anchor_b: [1., 0., 0.],
            axis: [1., 0., 0.],
        });
        project.save()?;
        assert!(!project.dirty);

        let mut reopened = Project::open(&project.path)?;
        assert_eq!(reopened.document.joints.len(), 2);
        assert_eq!(reopened.document.joints[0].kind, JointKind::Hinge);
        reopened.remove(bodies[0])?;
        assert_eq!(reopened.document.joints.len(), 1);
        assert_eq!(reopened.document.joints[0].a, bodies[1] - 1);
        assert_eq!(reopened.document.joints[0].b, bodies[2] - 1);
        assert_eq!(reopened.document.joints[0].kind, JointKind::Slider);
        reopened.save()?;
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
