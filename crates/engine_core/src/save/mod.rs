//! Versioned, human-readable scene and prefab documents.

use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{
    animation::{Clip, Playback},
    assets::{AssetId, Assets},
    ecs::EntityId,
    scene::{NodeState, Prefab, PrefabNode, Scene, Sprite, SpriteAppearance, Transform2D},
};

/// Current scene and prefab document format version.
pub const VERSION: u32 = 1;
const MAX_DOCUMENT: u64 = 8 * 1024 * 1024;
const MAX_NODES: usize = 100_000;

/// Texture path and local dimensions for a serialized sprite.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpriteFile {
    /// PNG path, resolved relative to the containing document.
    pub png: PathBuf,
    /// Local size in pixels.
    pub size: [f32; 2],
}

impl SpriteFile {
    fn resolve(&self, directory: &Path, assets: &mut Assets) -> Result<Sprite> {
        ensure!(
            self.size.iter().all(|size| size.is_finite() && *size > 0.0),
            "Invalid sprite size"
        );
        Ok(Sprite {
            texture: assets.import_file(&directory.join(&self.png))?,
            size: self.size,
        })
    }
}

/// Saved data for an attached script component.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptFile {
    /// TypeScript asset path, relative to this document.
    pub path: PathBuf,
    /// Name of the exported component class.
    pub component: String,
    /// Inspector values keyed by their declared property name.
    #[serde(default)]
    pub values: BTreeMap<String, PropertyValue>,
}

/// One authored script property override.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum PropertyValue {
    /// Numeric value.
    Number(f64),
    /// Boolean value.
    Boolean(bool),
    /// Text value.
    String(String),
    /// Inline scene node reference.
    Node(usize),
    /// Prefab asset path.
    Prefab(PathBuf),
}

/// Parent-first serialized node description.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NodeFile {
    /// Node name.
    pub name: String,
    /// Parent index in this document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<usize>,
    /// Authored local transform.
    #[serde(default)]
    pub transform: Transform2D,
    /// Optional sprite asset binding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sprite: Option<SpriteFile>,
    /// Optional visual overrides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<SpriteAppearance>,
    /// Attached TypeScript components.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<ScriptFile>,
}

/// Reference to a prefab to add after the scene's inline nodes.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceFile {
    /// Prefab path relative to this scene.
    pub prefab: PathBuf,
    /// Inline scene node to attach the instance to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<usize>,
}

/// Versioned .aurora-scene document.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SceneFile {
    /// Format version.
    pub version: u32,
    /// Inline nodes in parent-first order.
    pub nodes: Vec<NodeFile>,
    /// Additional prefab instances in document order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instances: Vec<InstanceFile>,
    /// Clips available for the loaded scene.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub animations: Vec<Clip>,
    /// Clip started automatically on scene entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autoplay: Option<usize>,
}

/// Versioned .aurora-prefab document describing one subtree.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PrefabFile {
    /// Format version.
    pub version: u32,
    /// Template nodes in parent-first order.
    pub nodes: Vec<NodeFile>,
}

impl SceneFile {
    /// Captures the active tree as inline nodes, resolving sprite handles to document paths.
    ///
    /// Returns a scene without clips or prefab references. Use the authored document
    /// directly when saving those assets and their editable bindings.
    pub fn capture(scene: &Scene, path_for: impl Fn(AssetId) -> Option<PathBuf>) -> Result<Self> {
        let mut indices = BTreeMap::new();
        let mut nodes = Vec::new();
        for state in scene.snapshot()? {
            let parent = state.parent.and_then(|id| indices.get(&id).copied());
            indices.insert(state.id, nodes.len());
            nodes.push(NodeFile {
                name: state.name,
                parent,
                transform: state.local,
                sprite: capture_sprite(state.sprite, &path_for)?,
                appearance: state.appearance,
                scripts: Vec::new(),
            });
        }
        let document = Self {
            version: VERSION,
            nodes,
            instances: Vec::new(),
            animations: Vec::new(),
            autoplay: None,
        };
        document.validate()?;
        Ok(document)
    }

    /// Reads and validates a scene document before resource resolution.
    pub fn load(path: &Path) -> Result<Self> {
        let file: Self = read(path)?;
        file.validate()
            .with_context(|| format!("Validating {}", path.display()))?;
        Ok(file)
    }

    /// Validates and atomically replaces a scene file.
    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        write(path, self)
    }

    fn validate(&self) -> Result<()> {
        validate_nodes(self.version, &self.nodes)?;
        ensure!(
            self.instances
                .iter()
                .all(|instance| instance.parent.is_none_or(|p| p < self.nodes.len())),
            "Invalid prefab instance parent"
        );
        ensure!(
            self.autoplay
                .is_none_or(|index| index < self.animations.len()),
            "Invalid autoplay clip"
        );
        for clip in &self.animations {
            clip.validate(if self.instances.is_empty() {
                self.nodes.len()
            } else {
                usize::MAX
            })?;
        }
        Ok(())
    }

    /// Resolves an authored scene for display. An optional clip and sample drives timeline scrubbing.
    pub fn preview(
        &self,
        directory: &Path,
        assets: &mut Assets,
        cursor: Option<(usize, f64)>,
    ) -> Result<Vec<NodeState>> {
        let mut document = self.clone();
        document.autoplay = None;
        let mut loaded = document.instantiate(directory, assets, Scene::default())?;
        if let Some((index, frame)) = cursor {
            let clip = loaded
                .clips
                .get(index)
                .context("Unknown preview clip")?
                .clone();
            Playback::new(clip, loaded.ids)?.seek(&mut loaded.scene, frame)?;
        }
        loaded.scene.snapshot()
    }

    pub(crate) fn instantiate(
        self,
        directory: &Path,
        assets: &mut Assets,
        mut scene: Scene,
    ) -> Result<Loaded> {
        self.validate()?;
        let mut ids = Vec::new();
        for node in &self.nodes {
            let parent = node.parent.map(|index| ids[index]);
            let (id, _) = scene.spawn(&node.name, parent, node.transform)?;
            if let Some(sprite) = &node.sprite {
                scene.set_sprite(id, sprite.resolve(directory, assets)?)?;
            }
            scene.set_appearance(id, node.appearance)?;
            ids.push(id);
        }
        let mut prefabs = BTreeMap::new();
        for instance in &self.instances {
            let path = directory.join(&instance.prefab);
            let prefab = match prefabs.entry(path) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => {
                    let path = entry.key();
                    let prefab = PrefabFile::load(path)?.resolve(parent_dir(path), assets)?;
                    entry.insert(prefab)
                }
            };
            ensure!(
                prefab.nodes().len() <= MAX_NODES - ids.len(),
                "Scene has too many instantiated nodes"
            );
            let (nodes, _) =
                prefab.instantiate(&mut scene, instance.parent.map(|index| ids[index]))?;
            ids.extend(nodes);
        }
        for clip in &self.animations {
            clip.validate(ids.len())?;
        }
        let mut loaded = Loaded {
            scene,
            ids,
            clips: self.animations,
            playback: None,
            entry_markers: Vec::new(),
        };
        if let Some(index) = self.autoplay {
            loaded.play(index)?;
        }
        Ok(loaded)
    }
}

impl PrefabFile {
    /// Captures a subtree and resolves its sprite handles to document paths.
    pub fn capture(
        scene: &Scene,
        root: EntityId,
        path_for: impl Fn(AssetId) -> Option<PathBuf>,
    ) -> Result<Self> {
        let prefab = Prefab::capture(scene, root)?;
        let nodes = prefab
            .nodes()
            .iter()
            .map(|node| {
                Ok(NodeFile {
                    name: node.name.clone(),
                    parent: node.parent,
                    transform: node.pose,
                    sprite: capture_sprite(node.sprite, &path_for)?,
                    appearance: node.appearance,
                    scripts: Vec::new(),
                })
            })
            .collect::<Result<_>>()?;
        let document = Self {
            version: VERSION,
            nodes,
        };
        document.validate()?;
        Ok(document)
    }

    /// Reads and validates one prefab document.
    pub fn load(path: &Path) -> Result<Self> {
        let file: Self = read(path)?;
        file.validate()
            .with_context(|| format!("Validating {}", path.display()))?;
        Ok(file)
    }

    /// Validates and atomically replaces a prefab file.
    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        write(path, self)
    }

    /// Resolves the template's PNG references into the asset store.
    pub fn resolve(&self, directory: &Path, assets: &mut Assets) -> Result<Prefab> {
        self.validate()?;
        let mut nodes = Vec::new();
        for node in &self.nodes {
            nodes.push(PrefabNode {
                name: node.name.clone(),
                parent: node.parent,
                pose: node.transform,
                appearance: node.appearance,
                sprite: node
                    .sprite
                    .as_ref()
                    .map(|sprite| sprite.resolve(directory, assets))
                    .transpose()?,
            });
        }
        Prefab::new(nodes)
    }

    fn validate(&self) -> Result<()> {
        validate_nodes(self.version, &self.nodes)?;
        ensure!(
            self.nodes.first().is_some_and(|node| node.parent.is_none())
                && self.nodes.iter().skip(1).all(|node| node.parent.is_some()),
            "Prefab needs exactly one root"
        );
        Ok(())
    }
}

pub(crate) struct Loaded {
    pub scene: Scene,
    pub ids: Vec<EntityId>,
    clips: Vec<Clip>,
    pub playback: Option<Playback>,
    pub entry_markers: Vec<u32>,
}

impl Loaded {
    pub fn play(&mut self, index: usize) -> Result<()> {
        let clip = self
            .clips
            .get(index)
            .context("Unknown animation clip")?
            .clone();
        let mut playback = Playback::new(clip, self.ids.clone())?;
        self.entry_markers = playback.start(&mut self.scene)?;
        self.playback = Some(playback);
        Ok(())
    }
}

fn capture_sprite(
    sprite: Option<Sprite>,
    path_for: &impl Fn(AssetId) -> Option<PathBuf>,
) -> Result<Option<SpriteFile>> {
    sprite
        .map(|sprite| {
            let png = path_for(sprite.texture)
                .with_context(|| format!("Missing source path for texture {}", sprite.texture))?;
            Ok(SpriteFile {
                png,
                size: sprite.size,
            })
        })
        .transpose()
}

fn validate_nodes(version: u32, nodes: &[NodeFile]) -> Result<()> {
    ensure!(
        version == VERSION,
        "Unsupported scene document version {version}"
    );
    ensure!(nodes.len() <= MAX_NODES, "Document has too many nodes");
    for (index, node) in nodes.iter().enumerate() {
        ensure!(
            node.parent.is_none_or(|parent| parent < index),
            "Node {index} has an invalid parent"
        );
        for script in &node.scripts {
            ensure!(
                !script.path.as_os_str().is_empty() && !script.component.is_empty(),
                "Invalid script attachment"
            );
            for value in script.values.values() {
                match value {
                    PropertyValue::Node(index) => {
                        ensure!(*index < nodes.len(), "Invalid node reference")
                    }
                    PropertyValue::Number(value) => {
                        ensure!(value.is_finite(), "Nonfinite script property")
                    }
                    _ => {}
                }
            }
        }
        if let Some(appearance) = node.appearance {
            ensure!(
                appearance
                    .anchor
                    .into_iter()
                    .chain(appearance.skew)
                    .all(f32::is_finite),
                "Invalid sprite appearance"
            );
        }
        let pose = node.transform;
        ensure!(
            pose.position
                .into_iter()
                .chain(pose.scale)
                .chain([pose.rotation])
                .all(f32::is_finite),
            "Node {index} has a nonfinite transform"
        );
        if let Some(sprite) = &node.sprite {
            ensure!(
                !sprite.png.as_os_str().is_empty()
                    && sprite
                        .size
                        .iter()
                        .all(|size| size.is_finite() && *size > 0.0),
                "Node {index} has an invalid sprite"
            );
        }
    }
    Ok(())
}

pub(crate) fn parent_dir(path: &Path) -> &Path {
    path.parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn read<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let mut bytes = Vec::new();
    File::open(path)
        .with_context(|| format!("Opening {}", path.display()))?
        .take(MAX_DOCUMENT + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_DOCUMENT,
        "Document {} is too large",
        path.display()
    );
    serde_json::from_slice(&bytes).with_context(|| format!("Parsing {}", path.display()))
}

fn write(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    ensure!((bytes.len() as u64) < MAX_DOCUMENT, "Document is too large");
    let mut staged = tempfile::NamedTempFile::new_in(parent_dir(path))?;
    staged.write_all(&bytes)?;
    staged.write_all(b"\n")?;
    staged.as_file().sync_all()?;
    staged
        .persist(path)
        .with_context(|| format!("Saving {}", path.display()))?;
    Ok(())
}
