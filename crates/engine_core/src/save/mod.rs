//! Versioned, human-readable scene and prefab documents.

pub mod visual;

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
    scene::{
        Display2D, NodeState, Prefab, PrefabNode, Scene, Sprite, SpriteAppearance, Transform2D,
    },
};

/// Current scene and prefab document format version.
pub const VERSION: u32 = 1;
const MAX_DOCUMENT: u64 = 8 * 1024 * 1024;
/// Maximum number of nodes in a loaded scene, including authored prefab instances.
pub const MAX_NODES: usize = 100_000;

/// Intended 2D/3D authoring mode for a scene.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SceneKind {
    /// Two-dimensional scene.
    #[default]
    TwoD,
    /// Three-dimensional scene.
    ThreeD,
    /// Scene intended to combine two- and three-dimensional content.
    TwoDThreeD,
}

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
            emitter: None,
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
    /// Integer value.
    Integer(i64),
    /// Boolean value.
    Boolean(bool),
    /// Text value.
    String(String),
    /// Multiline text value.
    Text(String),
    /// Two-dimensional numeric value.
    Vector2([f64; 2]),
    /// Three-dimensional numeric value.
    Vector3([f64; 3]),
    /// Width and height.
    Size([f64; 2]),
    /// Rectangle position and size as X, Y, width and height.
    Rect([f64; 4]),
    /// Straight-alpha sRGBA value.
    Color([u8; 4]),
    /// Selected string enum value.
    Enum(String),
    /// Inline scene node reference.
    Node(usize),
    /// Prefab asset path.
    Prefab(PathBuf),
    /// Audio clip path, registered in the scene's sound bank.
    AudioClip(PathBuf),
    /// Sprite frame PNG path, registered in the scene's frame banks.
    SpriteFrame(PathBuf),
    /// Numeric array.
    Numbers(Vec<f64>),
    /// Boolean array.
    Booleans(Vec<bool>),
    /// Text array.
    Strings(Vec<String>),
    /// Inline scene node reference array.
    Nodes(Vec<usize>),
    /// Prefab asset path array.
    Prefabs(Vec<PathBuf>),
    /// Audio clip path array, registered in the scene's sound bank.
    AudioClips(Vec<PathBuf>),
    /// Sprite frame PNG path array, registered in the scene's frame banks.
    SpriteFrames(Vec<PathBuf>),
    /// Reference to a script component on an inline scene node.
    Component {
        /// Node index in this document.
        node: usize,
        /// Component name as declared by `@ccclass`.
        component: String,
    },
    /// Unsigned 32-bit layer or flag mask.
    Bitmask(u32),
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
    /// Optional independent 3D pose and components.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spatial: Option<crate::scene::spatial::SpatialNode>,
    /// Optional sprite asset binding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sprite: Option<SpriteFile>,
    /// Authored visual generated from a text, atlas, tile map or emitter definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual: Option<visual::Visual>,
    /// Optional visual overrides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<SpriteAppearance>,
    /// Optional solid or trigger collider with its owning body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physics2d: Option<crate::physics2d::Body2D>,
    /// Attached TypeScript components.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<ScriptFile>,
}

impl NodeFile {
    fn resolve_visual(&self, directory: &Path, assets: &mut Assets) -> Result<Option<Sprite>> {
        match &self.visual {
            Some(visual) => Ok(Some(visual.resolve(directory, assets)?)),
            None => self
                .sprite
                .as_ref()
                .map(|sprite| sprite.resolve(directory, assets))
                .transpose(),
        }
    }
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

/// A non-solid overlap sensor centered on an authored node.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SensorFile {
    /// Node index, including expanded prefab nodes.
    pub node: usize,
    /// Local dimensions in pixels.
    pub size: [f32; 2],
}

/// Sprite frame alternatives for a node.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FrameFile {
    /// Target node index.
    pub node: usize,
    /// Frame PNGs relative to the scene document.
    pub png: Vec<PathBuf>,
}

/// Optional authored resources used by gameplay scripts.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct GameplayFile {
    /// Non-solid contact areas.
    pub sensors: Vec<SensorFile>,
    /// Registered frame textures by target node.
    pub frames: Vec<FrameFile>,
    /// Registered one-shot audio asset paths.
    pub sounds: Vec<PathBuf>,
}

impl GameplayFile {
    fn validate(&self, nodes: usize) -> Result<()> {
        let mut sensors = std::collections::BTreeSet::new();
        for sensor in &self.sensors {
            ensure!(
                sensor.node < nodes
                    && sensors.insert(sensor.node)
                    && sensor
                        .size
                        .iter()
                        .all(|side| side.is_finite() && *side > 0.0),
                "Invalid or duplicate gameplay sensor"
            );
        }
        let mut frames = std::collections::BTreeSet::new();
        for frame in &self.frames {
            ensure!(
                frame.node < nodes
                    && frames.insert(frame.node)
                    && (1..=256).contains(&frame.png.len())
                    && frame.png.iter().all(|path| !path.as_os_str().is_empty()),
                "Invalid or duplicate frame bank"
            );
        }
        ensure!(
            self.sounds.len() <= 128 && self.sounds.iter().all(|path| !path.as_os_str().is_empty()),
            "Invalid scene sound bank"
        );
        Ok(())
    }
}

/// Versioned .scene document.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SceneFile {
    /// Format version.
    pub version: u32,
    /// Intended authoring mode. Older documents without this field open as 2D scenes.
    #[serde(default)]
    pub scene_kind: SceneKind,
    /// Inline nodes in parent-first order.
    pub nodes: Vec<NodeFile>,
    /// Additional prefab instances in document order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instances: Vec<InstanceFile>,
    /// Prefab files available for scripts to instantiate. Their order defines the runtime IDs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub script_prefabs: Vec<PathBuf>,
    /// Clips available for the loaded scene.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub animations: Vec<Clip>,
    /// Clip started automatically on scene entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autoplay: Option<usize>,
    /// Optional scene-level gameplay resources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gameplay: Option<GameplayFile>,
    /// Optional authored design size and fit policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<Display2D>,
}

/// Versioned .prefab document describing one subtree.
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
            let visual = scene.visual(state.id)?;
            nodes.push(NodeFile {
                name: state.name,
                parent,
                transform: state.local,
                spatial: state.spatial,
                sprite: capture_sprite(state.sprite.filter(|_| visual.is_none()), &path_for)?,
                visual,
                appearance: state.appearance,
                physics2d: scene.body2d(state.id)?.as_deref().cloned(),
                scripts: Vec::new(),
            });
        }
        let document = Self {
            version: VERSION,
            scene_kind: match (
                nodes.iter().any(|node| node.spatial.is_some()),
                nodes.iter().any(|node| node.spatial.is_none()),
            ) {
                (true, true) => SceneKind::TwoDThreeD,
                (true, false) => SceneKind::ThreeD,
                _ => SceneKind::TwoD,
            },
            nodes,
            instances: Vec::new(),
            script_prefabs: Vec::new(),
            animations: Vec::new(),
            autoplay: None,
            gameplay: None,
            display: None,
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
            self.display.is_none_or(Display2D::is_valid),
            "Unsupported design resolution"
        );
        ensure!(
            self.instances
                .iter()
                .all(|instance| instance.parent.is_none_or(|p| p < self.nodes.len())),
            "Invalid prefab instance parent"
        );
        ensure!(
            self.script_prefabs.len() <= MAX_NODES
                && self.script_prefabs.iter().all(|path| matches!(
                    path.extension().and_then(|extension| extension.to_str()),
                    Some("prefab" | "aurora-prefab")
                )),
            "Invalid script prefab list"
        );
        ensure!(
            self.autoplay
                .is_none_or(|index| index < self.animations.len()),
            "Invalid autoplay clip"
        );
        let node_limit = if self.instances.is_empty() {
            self.nodes.len()
        } else {
            MAX_NODES
        };
        for clip in &self.animations {
            clip.validate(node_limit)?;
        }
        if let Some(gameplay) = &self.gameplay {
            gameplay.validate(node_limit)?;
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
        visual::draw_nodes(
            loaded.scene.snapshot()?,
            cursor.map_or(0, |(_, frame)| frame.max(0.0) as u64),
        )
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
            if let Some(sprite) = node.resolve_visual(directory, assets)? {
                scene.set_sprite(id, sprite)?;
            }
            scene.set_visual(id, node.visual.clone())?;
            scene.set_appearance(id, node.appearance)?;
            scene.set_body2d(id, node.physics2d.clone())?;
            scene.set_spatial(id, node.spatial.clone())?;
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
        if let Some(gameplay) = &self.gameplay {
            gameplay.validate(ids.len())?;
        }
        let mut loaded = Loaded {
            spatial_physics: Default::default(),
            display: self.display.unwrap_or_default(),
            scene,
            ids,
            script_prefabs: self
                .script_prefabs
                .into_iter()
                .map(|path| directory.join(path))
                .collect(),
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
                    physics2d: node.physics2d.clone(),
                    spatial: node.spatial.clone(),
                    visual: node.visual.clone(),
                    sprite: capture_sprite(
                        node.sprite.filter(|_| node.visual.is_none()),
                        &path_for,
                    )?,
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
                physics2d: node.physics2d.clone(),
                spatial: node.spatial.clone(),
                appearance: node.appearance,
                visual: node.visual.clone(),
                sprite: node.resolve_visual(directory, assets)?,
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
    pub spatial_physics: crate::physics3d::authored::AuthoredWorld,
    pub display: Display2D,
    pub scene: Scene,
    pub ids: Vec<EntityId>,
    pub script_prefabs: Vec<PathBuf>,
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
        if let Some(body) = &node.physics2d {
            body.validate()
                .with_context(|| format!("Physics on node {index}"))?;
        }
        ensure!(
            node.parent.is_none_or(|parent| parent < index),
            "Node {index} has an invalid parent"
        );
        ensure!(
            node.visual.is_none() || node.sprite.is_none(),
            "Choose one visual for node {index}"
        );
        if let Some(visual) = &node.visual {
            visual.validate()?;
        }
        if let Some(spatial) = &node.spatial {
            spatial.validate()?;
        }
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
                    PropertyValue::Vector2(value) => {
                        ensure!(
                            value.iter().all(|value| value.is_finite()),
                            "Nonfinite script property"
                        )
                    }
                    PropertyValue::Vector3(value) => {
                        ensure!(
                            value.iter().all(|value| value.is_finite()),
                            "Nonfinite script property"
                        )
                    }
                    PropertyValue::Size(value) => {
                        ensure!(
                            value.iter().all(|value| value.is_finite() && *value >= 0.0),
                            "Invalid script property size"
                        )
                    }
                    PropertyValue::Rect(value) => {
                        ensure!(
                            value.iter().all(|value| value.is_finite()),
                            "Nonfinite script property"
                        )
                    }
                    PropertyValue::Numbers(values) => {
                        ensure!(
                            values.iter().all(|value| value.is_finite()),
                            "Nonfinite script property"
                        );
                    }
                    PropertyValue::Nodes(values) => {
                        ensure!(
                            values.iter().all(|index| *index < nodes.len()),
                            "Invalid node reference"
                        );
                    }
                    PropertyValue::Component { node, component } => {
                        ensure!(*node < nodes.len(), "Invalid component reference node");
                        ensure!(
                            !component.trim().is_empty(),
                            "Invalid component reference name"
                        );
                    }
                    PropertyValue::AudioClip(path) | PropertyValue::SpriteFrame(path) => {
                        ensure!(!path.as_os_str().is_empty(), "Invalid asset property path")
                    }
                    PropertyValue::Prefabs(paths)
                    | PropertyValue::AudioClips(paths)
                    | PropertyValue::SpriteFrames(paths) => {
                        ensure!(
                            paths.iter().all(|path| !path.as_os_str().is_empty()),
                            "Invalid asset property path"
                        )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_kind_defaults_for_existing_documents_and_round_trips() -> Result<()> {
        let legacy: SceneFile = serde_json::from_str(r#"{"version":1,"nodes":[]}"#)?;
        assert_eq!(legacy.scene_kind, SceneKind::TwoD);
        assert_eq!(serde_json::to_value(&legacy)?["scene_kind"], "two_d");

        let mut mixed = legacy;
        mixed.scene_kind = SceneKind::TwoDThreeD;
        let encoded = serde_json::to_value(&mixed)?;
        assert_eq!(encoded["scene_kind"], "two_d_three_d");
        let decoded: SceneFile = serde_json::from_value(encoded)?;
        assert_eq!(decoded.scene_kind, SceneKind::TwoDThreeD);
        Ok(())
    }

    #[test]
    fn validates_extended_script_math_values() -> Result<()> {
        let mut values = BTreeMap::new();
        values.insert("position".into(), PropertyValue::Vector3([1.0, 2.0, 3.0]));
        values.insert("size".into(), PropertyValue::Size([640.0, 360.0]));
        values.insert(
            "bounds".into(),
            PropertyValue::Rect([0.0, 0.0, 640.0, 360.0]),
        );
        values.insert("layers".into(), PropertyValue::Bitmask(0x8000_0001));
        let document = NodeFile {
            name: "Root".into(),
            parent: None,
            transform: Transform2D::default(),
            spatial: None,
            sprite: None,
            visual: None,
            appearance: None,
            physics2d: None,
            scripts: vec![ScriptFile {
                path: "scripts/player.ts".into(),
                component: "Player".into(),
                values,
            }],
        };
        let encoded = serde_json::to_value(&document)?;
        let decoded: NodeFile = serde_json::from_value(encoded.clone())?;
        assert_eq!(serde_json::to_value(decoded)?, encoded);
        assert!(validate_nodes(VERSION, &[document]).is_ok());

        let mut invalid_size = NodeFile {
            name: "Root".into(),
            parent: None,
            transform: Transform2D::default(),
            spatial: None,
            sprite: None,
            visual: None,
            appearance: None,
            physics2d: None,
            scripts: vec![ScriptFile {
                path: "scripts/player.ts".into(),
                component: "Player".into(),
                values: BTreeMap::from([("size".into(), PropertyValue::Size([-1.0, 2.0]))]),
            }],
        };
        assert!(validate_nodes(VERSION, &[invalid_size.clone()]).is_err());
        invalid_size.scripts[0]
            .values
            .insert("size".into(), PropertyValue::Size([1.0, f64::INFINITY]));
        assert!(validate_nodes(VERSION, &[invalid_size]).is_err());
        Ok(())
    }
}
