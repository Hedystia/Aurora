//! Resources and overlap events for authored gameplay scenes.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use script_api::Kind;

use crate::{
    assets::AssetId,
    audio::{Audio, AudioClip, AudioSource},
    director::Director,
    ecs::EntityId,
    save::{SceneFile, parent_dir},
    scene::NodeState,
};

pub(super) struct Gameplay {
    sensors: BTreeMap<EntityId, [f32; 2]>,
    contacts: BTreeMap<EntityId, BTreeSet<EntityId>>,
    frames: BTreeMap<EntityId, Vec<AssetId>>,
    audio: Option<(Audio, AudioSource)>,
    silent: bool,
    profile: Option<PathBuf>,
    pub(super) best: u32,
}

impl Gameplay {
    /// Minimal resources for a scene with no authored gameplay document.
    pub(super) fn empty(silent: bool) -> Self {
        Self {
            sensors: BTreeMap::new(),
            contacts: BTreeMap::new(),
            frames: BTreeMap::new(),
            audio: None,
            silent,
            profile: None,
            best: 0,
        }
    }

    pub(super) fn load(
        director: &mut Director,
        silent: bool,
        profile: Option<&Path>,
    ) -> Result<Option<Self>> {
        let path = director.scene_path();
        let Some(document) = SceneFile::load(path)?.gameplay else {
            return Ok(None);
        };
        let directory = parent_dir(path).to_path_buf();
        let mut sensors = BTreeMap::new();
        for sensor in document.sensors {
            let node = director.node(sensor.node).context("Unknown sensor node")?;
            ensure!(
                sensor
                    .size
                    .into_iter()
                    .all(|side| side > 0.0 && side.is_finite()),
                "Invalid sensor dimensions"
            );
            ensure!(
                sensors.insert(node, sensor.size).is_none(),
                "Duplicate sensor node"
            );
        }
        let mut frames = BTreeMap::new();
        for bank in document.frames {
            let node = director.node(bank.node).context("Unknown frame target")?;
            ensure!(
                director.scene().sprite(node)?.is_some(),
                "Frame target needs a sprite"
            );
            ensure!(
                !bank.png.is_empty() && bank.png.len() <= 256,
                "Invalid frame bank"
            );
            let textures = bank
                .png
                .into_iter()
                .map(|path| director.assets_mut().import_file(&directory.join(path)))
                .collect::<Result<_>>()?;
            ensure!(
                frames.insert(node, textures).is_none(),
                "Duplicate frame target"
            );
        }
        let audio = if document.sounds.is_empty() {
            None
        } else {
            let mut clips = Vec::new();
            for path in document.sounds {
                let path = directory.join(path);
                let mut clip = Vec::new();
                File::open(&path)
                    .with_context(|| format!("Opening {}", path.display()))?
                    .take(16_777_217)
                    .read_to_end(&mut clip)?;
                ensure!(clip.len() <= 16_777_216, "Scene sound exceeds 16 MiB");
                clips.push(AudioClip::decode(clip)?);
            }
            Some((Audio::new(silent)?, AudioSource::new(clips)))
        };
        let best = profile.map(load_score).transpose()?.unwrap_or(0);
        Ok(Some(Self {
            sensors,
            contacts: BTreeMap::new(),
            frames,
            audio,
            silent,
            profile: profile.map(Path::to_path_buf),
            best,
        }))
    }

    pub(super) fn select_frame(
        &self,
        director: &mut Director,
        node: EntityId,
        index: usize,
    ) -> Result<()> {
        let texture = self
            .frames
            .get(&node)
            .and_then(|bank| bank.get(index))
            .context("Unknown sprite frame")?;
        let mut sprite = director.scene().sprite(node)?.context("Missing sprite")?;
        sprite.texture = *texture;
        director.scene_mut().set_sprite(node, sprite)
    }

    pub(super) fn sound(&mut self, id: u64, dynamic: Option<&AudioClip>) -> Result<()> {
        if let Some(clip) = dynamic {
            if self.audio.is_none() {
                self.audio = Some((Audio::new(self.silent)?, AudioSource::new(Vec::new())));
            }
            let Some((audio, _)) = self.audio.as_mut() else {
                anyhow::bail!("Audio mixer is unavailable");
            };
            clip.play(audio)?;
            return Ok(());
        }
        let (audio, source) = self.audio.as_mut().context("Scene has no audio clips")?;
        source.play_one_shot(audio, id as usize)?;
        Ok(())
    }

    pub(super) fn step_audio(&mut self) {
        if let Some((audio, _)) = &mut self.audio {
            audio.step();
        }
    }

    pub(super) fn record(&mut self, score: u32) -> Result<()> {
        if score > self.best {
            if let Some(path) = &self.profile {
                let directory = parent_dir(path);
                fs::create_dir_all(directory)?;
                let basename = path.file_name().context("Missing score profile filename")?;
                let mut lock_name = basename.to_os_string();
                lock_name.push(".lock");
                let lock = OpenOptions::new()
                    .create(true)
                    .truncate(false)
                    .write(true)
                    .open(directory.join(lock_name))?;
                lock.lock()?;
                let stored = load_score(path)?;
                if stored >= score {
                    self.best = stored;
                    return Ok(());
                }
                let mut file = tempfile::NamedTempFile::new_in(directory)?;
                writeln!(file, "{score}")?;
                file.as_file().sync_all()?;
                file.persist(path)?;
            }
            self.best = score;
        }
        Ok(())
    }

    pub(super) fn contacts(
        &mut self,
        states: &[NodeState],
        target: EntityId,
        physical: impl Iterator<Item = EntityId>,
    ) -> Vec<(Kind, EntityId)> {
        let actor_bounds = states
            .iter()
            .find(|node| node.id == target && node.visible)
            .and_then(|node| self.sensors.get(&target).map(|&size| bounds(node, size)));
        let mut next = actor_bounds.map_or_else(BTreeSet::new, |target_bounds| {
            states
                .iter()
                .filter_map(|node| {
                    if node.id == target || !node.visible {
                        return None;
                    }
                    let size = *self.sensors.get(&node.id)?;
                    target_bounds
                        .overlaps(bounds(node, size))
                        .then_some(node.id)
                })
                .collect()
        });
        next.extend(physical);
        let previous = self.contacts.entry(target).or_default();
        let events = previous
            .difference(&next)
            .map(|&id| (Kind::CollisionExit, id))
            .chain(
                next.difference(previous)
                    .map(|&id| (Kind::CollisionEnter, id)),
            )
            .collect();
        *previous = next;
        events
    }
}

fn load_score(path: &Path) -> Result<u32> {
    match File::open(path) {
        Ok(file) => {
            let mut text = String::new();
            file.take(64).read_to_string(&mut text)?;
            Ok(text.trim().parse().context("Invalid score profile")?)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error.into()),
    }
}

struct Bounds {
    min: [f32; 2],
    max: [f32; 2],
}

impl Bounds {
    fn overlaps(&self, other: Self) -> bool {
        (0..2).all(|axis| self.min[axis] < other.max[axis] && other.min[axis] < self.max[axis])
    }
}

fn bounds(node: &NodeState, [width, height]: [f32; 2]) -> Bounds {
    let half = [width / 2.0, height / 2.0];
    let mut result = Bounds {
        min: [f32::INFINITY; 2],
        max: [f32::NEG_INFINITY; 2],
    };
    for x in [-half[0], half[0]] {
        for y in [-half[1], half[1]] {
            let point = node.world.point([x, y]);
            for (axis, value) in point.into_iter().enumerate() {
                result.min[axis] = result.min[axis].min(value);
                result.max[axis] = result.max[axis].max(value);
            }
        }
    }
    result
}
