//! Scene selection and pause state for authored game scenes.

mod script_scene;

use std::path::PathBuf;

use anyhow::{Context, Result};

pub(crate) use self::script_scene::ScriptClone;
use self::script_scene::ScriptScene;

use crate::{
    assets::Assets,
    ecs::EntityId,
    save::{Loaded, SceneFile, parent_dir},
    scene::{Lifecycle, Scene},
};

/// Owns the current scene, shared assets, and fixed-step pause state.
pub struct Director {
    paths: Vec<PathBuf>,
    current: usize,
    loaded: Loaded,
    physics: crate::physics2d::AuthoredPhysics,
    script_scene: ScriptScene,
    assets: Assets,
    paused: bool,
    ticks: u64,
}

impl Director {
    /// Opens the first registered scene and starts its autoplay clip.
    pub fn new(paths: Vec<PathBuf>, cache: Option<PathBuf>) -> Result<Self> {
        let first = paths.first().context("Director needs a scene")?;
        let mut assets = Assets::new(cache);
        let mut loaded = SceneFile::load(first)?.instantiate(
            parent_dir(first),
            &mut assets,
            Scene::default(),
        )?;
        loaded.scene.activate()?;
        Ok(Self {
            paths,
            current: 0,
            loaded,
            physics: Default::default(),
            script_scene: ScriptScene::default(),
            assets,
            paused: false,
            ticks: 0,
        })
    }

    /// Pauses authored scene simulation.
    pub fn pause(&mut self) {
        self.paused = true;
    }

    /// Resumes at the same animation phase.
    pub fn resume(&mut self) {
        self.paused = false;
    }

    /// Returns whether simulation is paused.
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Index of the active scene within the registered paths.
    pub fn current(&self) -> usize {
        self.current
    }

    /// Number of registered scene paths.
    pub fn scene_count(&self) -> usize {
        self.paths.len()
    }

    /// Number of simulation ticks run, excluding time spent paused.
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    /// Returns the current scene tree.
    pub fn scene(&self) -> &Scene {
        &self.loaded.scene
    }

    /// Provides mutable access to the current scene tree.
    pub fn scene_mut(&mut self) -> &mut Scene {
        &mut self.loaded.scene
    }

    pub(crate) fn set_text(&mut self, node: EntityId, text: String) -> Result<()> {
        let mut visual = self
            .loaded
            .scene
            .visual(node)?
            .context("Target node does not have a Text visual")?;
        let crate::save::visual::Visual::Text { spans, .. } = &mut visual else {
            anyhow::bail!("Target node does not have a Text visual");
        };
        let first = spans
            .first_mut()
            .context("Text visual has no styled runs")?;
        first.text = text;
        spans.truncate(1);
        visual.validate()?;
        let directory = parent_dir(self.scene_path()).to_path_buf();
        let sprite = visual.resolve(&directory, &mut self.assets)?;
        self.loaded.scene.set_visual(node, Some(visual))?;
        self.loaded.scene.set_sprite(node, sprite)
    }

    /// Returns the first authored node for the single-instance script bridge.
    pub fn primary(&self) -> Option<EntityId> {
        self.loaded.ids.first().copied()
    }

    /// Returns a scene node in authored document order.
    pub fn node(&self, index: usize) -> Option<EntityId> {
        self.loaded.ids.get(index).copied()
    }

    /// Display policy of the active 2D document.
    pub fn display(&self) -> crate::scene::Display2D {
        self.loaded.display
    }

    /// Returns textures shared by the current scene and renderer.
    pub fn assets(&self) -> &Assets {
        &self.assets
    }

    pub(crate) fn assets_mut(&mut self) -> &mut Assets {
        &mut self.assets
    }

    pub(crate) fn scene_path(&self) -> &std::path::Path {
        &self.paths[self.current]
    }

    /// Loads the requested scene before replacing the previous one.
    pub fn change_scene(&mut self, index: usize) -> Result<Vec<Lifecycle>> {
        let (next, entered) = self.prepare_scene_change(index)?;
        self.commit_scene_change(index, next, entered)
    }

    pub(crate) fn prepare_scene_change(
        &mut self,
        index: usize,
    ) -> Result<(Loaded, Vec<Lifecycle>)> {
        let path = self.paths.get(index).context("Unknown scene index")?;
        let mut next = SceneFile::load(path)?.instantiate(
            parent_dir(path),
            &mut self.assets,
            self.loaded.scene.successor(),
        )?;
        let entered = next.scene.activate()?;
        Ok((next, entered))
    }

    pub(crate) fn commit_scene_change(
        &mut self,
        index: usize,
        next: Loaded,
        entered: Vec<Lifecycle>,
    ) -> Result<Vec<Lifecycle>> {
        let mut events = self.loaded.scene.deactivate()?;
        events.extend(entered);
        self.loaded = next;
        self.physics = Default::default();
        self.script_scene = self.script_scene.successor();
        self.current = index;
        Ok(events)
    }

    pub(crate) fn set_linear_velocity(&mut self, node: EntityId, velocity: [f32; 2]) -> Result<()> {
        self.physics
            .set_velocity(&self.loaded.scene, node, velocity)
    }

    pub(crate) fn physics_overlaps(&self, target: EntityId) -> impl Iterator<Item = EntityId> + '_ {
        self.physics.overlaps(target)
    }

    pub(crate) fn physics_contacts(&self) -> &[crate::physics2d::Contact] {
        &self.physics.contacts
    }

    pub(crate) fn set_linear_velocity_3d(
        &mut self,
        node: EntityId,
        velocity: [f32; 3],
    ) -> Result<()> {
        self.loaded.spatial_physics.set_velocity(node, velocity)
    }

    pub(crate) fn set_angular_velocity_3d(
        &mut self,
        node: EntityId,
        velocity: [f32; 3],
    ) -> Result<()> {
        self.loaded
            .spatial_physics
            .set_angular_velocity(node, velocity)
    }

    pub(crate) fn physics3d_contacts(&self) -> &[crate::physics3d::authored::Contact] {
        self.loaded.spatial_physics.contacts()
    }

    pub(crate) fn physics3d_raycast(
        &self,
        origin: [f32; 3],
        direction: [f32; 3],
        max_distance: f32,
    ) -> Result<Option<crate::physics3d::authored::RayHit3D>> {
        self.loaded
            .spatial_physics
            .raycast(origin, direction, max_distance)
    }

    /// Starts an authored clip from its first sample.
    pub fn play(&mut self, index: usize) -> Result<()> {
        self.loaded.play(index)
    }

    /// Advances the active scene by one tick and returns animation events.
    pub fn step(&mut self) -> Result<Vec<u32>> {
        if self.paused {
            return Ok(Vec::new());
        }
        let mut events = std::mem::take(&mut self.loaded.entry_markers);
        if let Some(playback) = &mut self.loaded.playback {
            events.extend(playback.step(&mut self.loaded.scene)?);
        }
        self.physics.step(&mut self.loaded.scene)?;
        self.loaded.spatial_physics.step(&mut self.loaded.scene)?;
        self.ticks = self
            .ticks
            .checked_add(1)
            .context("Director clock exhausted")?;
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        animation::{Clip, Interpolation, Key, Marker, Property, Track, Wrap},
        save::{
            InstanceFile, NodeFile, PrefabFile, VERSION,
            visual::{Span, Visual},
        },
        scene::Transform2D,
    };

    #[test]
    fn runtime_text_replaces_rich_runs_and_rasterizes_updated_content() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let path = scratch.path().join("text.scene");
        SceneFile {
            version: VERSION,
            joints: Vec::new(),
            scene_kind: crate::save::SceneKind::TwoD,
            nodes: vec![NodeFile {
                name: "Label".into(),
                parent: None,
                transform: Transform2D::default(),
                spatial: None,
                sprite: None,
                visual: Some(Visual::Text {
                    font: None,
                    spans: vec![
                        Span {
                            text: "Old".into(),
                            size: 24,
                            color: [255; 4],
                        },
                        Span {
                            text: " Rich".into(),
                            size: 32,
                            color: [255; 4],
                        },
                    ],
                }),
                appearance: None,
                physics2d: None,
                scripts: Vec::new(),
            }],
            instances: Vec::new(),
            script_prefabs: Vec::new(),
            animations: Vec::new(),
            autoplay: None,
            gameplay: None,
            display: None,
        }
        .save(&path)?;
        let mut director = Director::new(vec![path], None)?;
        let node = director.primary().context("Missing text node")?;
        let previous = director
            .scene()
            .sprite(node)?
            .context("Missing text sprite")?;
        director.set_text(node, "Replacement text".into())?;
        let visual = director
            .scene()
            .visual(node)?
            .context("Missing updated Text visual")?;
        let Visual::Text { spans, .. } = visual else {
            anyhow::bail!("Updated visual is not Text");
        };
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "Replacement text");
        let current = director
            .scene()
            .sprite(node)?
            .context("Missing updated sprite")?;
        assert_ne!(current.texture, previous.texture);
        Ok(())
    }

    #[test]
    fn persists_scenes_and_prefabs_and_switches_transactionally() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let base = scratch.path();
        let mut scene = Scene::default();
        let (root, _) = scene.spawn("Gallery", None, Transform2D::default())?;
        let (child, _) = scene.spawn("Template", Some(root), Transform2D::default())?;
        scene.spawn(
            "Leaf",
            Some(child),
            Transform2D {
                position: [3.0, 4.0],
                ..Default::default()
            },
        )?;
        PrefabFile::capture(&scene, child, |_| None)?.save(&base.join("sample.prefab"))?;
        scene.remove(child)?;
        let mut document = SceneFile::capture(&scene, |_| None)?;
        document.instances.push(InstanceFile {
            prefab: "sample.prefab".into(),
            parent: Some(0),
        });
        document.animations.push(Clip {
            name: "Test".into(),
            sample_rate: 60,
            duration: 2,
            speed: 1.0,
            wrap: Wrap::Once,
            tracks: vec![Track {
                node: 1,
                property: Property::X,
                interpolation: Interpolation::Linear,
                keys: vec![
                    Key {
                        frame: 0,
                        value: 0.0,
                        tangents: None,
                    },
                    Key {
                        frame: 2,
                        value: 4.0,
                        tangents: None,
                    },
                ],
            }],
            events: vec![Marker { frame: 1, code: 42 }],
        });
        document.autoplay = Some(0);
        let first = base.join("first.scene");
        let second = base.join("second.scene");
        let broken = base.join("broken.scene");
        document.save(&first)?;
        let mut reloaded = SceneFile::load(&first)?;
        reloaded.nodes[0].name = "Lookout".into();
        reloaded.save(&second)?;
        reloaded.instances[0].prefab = "absent.prefab".into();
        reloaded.save(&broken)?;

        let mut director = Director::new(vec![first, second, broken], None)?;
        let old = director.primary().context("Missing root")?;
        assert_eq!(director.step()?, [42]);
        director.pause();
        assert!(director.step()?.is_empty());
        assert_eq!(director.ticks(), 1);
        assert_eq!(
            director.scene().snapshot()?[2].world.point([0.0; 2]),
            [5.0, 4.0]
        );
        director.resume();
        assert!(director.step()?.is_empty());
        assert_eq!(
            director.scene().snapshot()?[2].world.point([0.0; 2]),
            [7.0, 4.0]
        );
        assert!(director.change_scene(2).is_err());
        assert_eq!(director.current(), 0);
        let events = director.change_scene(1)?;
        assert_eq!(events.len(), 9);
        assert_ne!(director.primary(), Some(old));
        assert!(director.scene().transform(old).is_err());
        assert_eq!(director.scene().snapshot()?[0].name, "Lookout");
        assert_eq!(director.step()?, [42]);
        Ok(())
    }

    #[test]
    fn autoplay_clip_drives_an_authored_spatial_node() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let path = scratch.path().join("spatial.scene");
        SceneFile {
            version: VERSION,
            joints: Vec::new(),
            scene_kind: crate::save::SceneKind::ThreeD,
            nodes: vec![NodeFile {
                name: "Cube".into(),
                parent: None,
                transform: Transform2D::default(),
                spatial: Some(crate::scene::spatial::SpatialNode::default()),
                sprite: None,
                visual: None,
                appearance: None,
                physics2d: None,
                scripts: Vec::new(),
            }],
            instances: Vec::new(),
            script_prefabs: Vec::new(),
            animations: vec![Clip {
                name: "Lift".into(),
                sample_rate: 60,
                duration: 2,
                speed: 1.0,
                wrap: Wrap::Once,
                tracks: vec![
                    Track {
                        node: 0,
                        property: Property::TranslationY,
                        interpolation: Interpolation::Linear,
                        keys: vec![
                            Key {
                                frame: 0,
                                value: 0.0,
                                tangents: None,
                            },
                            Key {
                                frame: 2,
                                value: 2.0,
                                tangents: None,
                            },
                        ],
                    },
                    Track {
                        node: 0,
                        property: Property::RotationX,
                        interpolation: Interpolation::Linear,
                        keys: vec![
                            Key {
                                frame: 0,
                                value: 0.0,
                                tangents: None,
                            },
                            Key {
                                frame: 2,
                                value: 90.0,
                                tangents: None,
                            },
                        ],
                    },
                ],
                events: Vec::new(),
            }],
            autoplay: Some(0),
            gameplay: None,
            display: None,
        }
        .save(&path)?;

        let mut director = Director::new(vec![path], None)?;
        let node = director.primary().context("Missing root")?;
        assert_eq!(
            director
                .scene()
                .spatial(node)?
                .context("Missing spatial")?
                .position[1],
            0.0
        );
        assert!(director.step()?.is_empty());
        let spatial = director.scene().spatial(node)?.context("Missing spatial")?;
        assert_eq!(spatial.position[1], 1.0);
        assert_eq!(spatial.rotation[0], 45.0);
        Ok(())
    }
}
