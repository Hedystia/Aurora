//! Shared player entry point for built-in demos and authored scenes.

mod gameplay;
mod prefab_scripts;
mod settings;
mod spatial;

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque, btree_map::Entry},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use script_api::Kind;
use winit::{
    event::{ElementState, WindowEvent},
    keyboard::{KeyCode, PhysicalKey},
};

use crate::{
    audio::AudioClip,
    director::Director,
    ecs::EntityId,
    input::InputEvent,
    physics3d::Backend,
    render::{Cube, Frame},
    save::parent_dir,
    scene::{Affine2D, NodeState, Sprite, SpriteAppearance, Transform2D},
    scripting::{Command, Script, Session},
};

use self::prefab_scripts::{Mutation, RuntimeBinding};

use super::{
    Scenario, SceneProgram,
    demo::{CAMERA, Demo},
    script_lifecycle::Binding,
};

pub(super) enum Runtime {
    Demo(Box<Demo>),
    Authored(Box<Authored>),
    Spatial(Box<spatial::Spatial>),
}

fn pack_entry((name, value): (&str, &str)) -> String {
    format!("{name}\0{value}")
}

impl Runtime {
    pub(super) fn new(
        sprite: Option<&Path>,
        cache: Option<PathBuf>,
        scenario: &Scenario<'_>,
        silent: bool,
    ) -> Result<Self> {
        anyhow::ensure!(
            scenario.scene_programs.is_empty()
                || scenario.scene_programs.len() == scenario.scenes.len(),
            "Packaged scene programs must match the registered scenes"
        );
        if let Some(backend) = scenario.spatial {
            return Ok(Self::Spatial(Box::new(spatial::Spatial::new(backend)?)));
        }
        if scenario.scenes.is_empty() {
            Ok(Self::Demo(Box::new(Demo::new(
                sprite,
                cache,
                scenario.bird,
                silent,
            )?)))
        } else {
            let mut director = Director::new(scenario.scenes.to_vec(), cache)?;
            let gameplay = gameplay::Gameplay::load(&mut director, silent, scenario.profile)?;
            Ok(Self::Authored(Box::new(Authored {
                director,
                gameplay,
                settings: settings::GameSettings::load(scenario.config, scenario.secrets)?,
                profile: scenario.profile.map(Path::to_path_buf),
                silent,
                loading: None,
                pending_config: BTreeMap::new(),
                script: None,
                pending_scene: None,
                bindings: BTreeMap::new(),
                pointers: BTreeMap::new(),
                mutations: VecDeque::new(),
                pending_text: BTreeMap::new(),
                pending_sprites: BTreeMap::new(),
                pending_audio: BTreeMap::new(),
                pending_prefabs: BTreeMap::new(),
                pending_raycasts: BTreeMap::new(),
                pending_overlaps: BTreeMap::new(),
                dynamic_frames: BTreeMap::new(),
                dynamic_audio: BTreeMap::new(),
                dynamic_prefabs: BTreeMap::new(),
                asset_refs: BTreeMap::new(),
                next_asset: 0x8000_0000,
                pending_asset_events: Vec::new(),
                pending_raycast_events: Vec::new(),
                pending_overlap_events: Vec::new(),
                next_binding: 0,
                tearing_down: false,
                transformed: BTreeSet::new(),
                script_nodes: if let Some(program) = scenario.scene_programs.first() {
                    program.bindings.clone()
                } else if scenario.script_nodes.is_empty() {
                    vec![0]
                } else {
                    scenario.script_nodes.to_vec()
                },
                scene_programs: scenario.scene_programs.to_vec(),
            })))
        }
    }

    pub(super) fn attach_script(&mut self, script: Option<&Script>) -> Result<()> {
        match self {
            Self::Demo(demo) => demo.attach_script(script),
            Self::Spatial(_) => {
                anyhow::ensure!(script.is_none(), "The 3D demo has no script bindings");
                Ok(())
            }
            Self::Authored(game) => {
                game.script = if let Some(program) = game.scene_programs.first() {
                    anyhow::ensure!(script.is_none(), "A native bundle owns its scene programs");
                    spawn_scene_program(program)?
                } else {
                    script.map(Session::spawn).transpose()?
                };
                game.start()
            }
        }
    }

    pub(super) fn disconnect_script(&mut self) {
        match self {
            Self::Demo(demo) => demo.disconnect_script(),
            Self::Authored(game) => game.script = None,
            Self::Spatial(_) => {}
        }
    }

    pub(super) fn stop_script(&mut self) -> Result<()> {
        match self {
            Self::Demo(demo) => demo.stop_script(),
            Self::Authored(game) => {
                if let Some(script) = game.script.as_mut() {
                    script.begin_shutdown();
                }
                let result = game.end_scene();
                game.script = None;
                result
            }
            Self::Spatial(_) => Ok(()),
        }
    }

    pub(super) fn tick(&mut self, input: &[InputEvent]) -> Result<()> {
        match self {
            Self::Demo(demo) => demo.tick(input),
            Self::Authored(game) => game.tick(input),
            Self::Spatial(game) => game.tick(),
        }
    }

    pub(super) fn node_states(&self) -> Result<Vec<NodeState>> {
        match self {
            Self::Demo(demo) => demo.scene.snapshot(),
            Self::Authored(game) => game.director.scene().snapshot(),
            Self::Spatial(_) => Ok(Vec::new()),
        }
    }

    pub(super) fn spatial_state(&self) -> Option<(Backend, &[Cube])> {
        match self {
            Self::Spatial(game) => Some((game.backend, &game.cubes)),
            _ => None,
        }
    }

    pub(super) fn frame(&self) -> Result<Frame<'_>> {
        Ok(match self {
            Self::Demo(demo) => Frame::Sprites {
                assets: &demo.assets,
                nodes: demo.scene.snapshot()?,
                camera: CAMERA,
            },
            Self::Authored(game) => {
                let mut nodes = crate::save::visual::draw_nodes(
                    game.director.scene().snapshot()?,
                    game.director.ticks(),
                )?;
                if let Some(sprite) = &game.loading {
                    nodes.push(NodeState {
                        draw_tick: game.director.ticks(),
                        id: EntityId(u64::MAX),
                        parent: None,
                        name: "Loading".into(),
                        local: Transform2D::default(),
                        spatial: None,
                        world: Affine2D::IDENTITY,
                        sprite: Some(*sprite),
                        appearance: Some(SpriteAppearance {
                            color: [0, 0, 0, 255],
                            opacity: 170,
                            layer: i32::MAX,
                            ..Default::default()
                        }),
                        visible: true,
                    });
                }
                Frame::Sprites {
                    assets: game.director.assets(),
                    nodes,
                    camera: game.director.display().camera(),
                }
            }
            Self::Spatial(game) => Frame::Cubes(&game.cubes),
        })
    }

    pub(super) fn preview_size(&self) -> [u32; 2] {
        match self {
            Self::Authored(game) => game.director.display().preview_resolution(),
            _ => crate::scene::Display2D::default().resolution,
        }
    }

    pub(super) fn ticks(&self) -> u64 {
        match self {
            Self::Demo(demo) => demo.ticks,
            Self::Authored(game) => game.director.ticks(),
            Self::Spatial(game) => game.ticks,
        }
    }

    pub(super) fn bird_stats(&self) -> Result<Option<serde_json::Value>> {
        match self {
            Self::Demo(demo) => demo.bird_stats(),
            Self::Authored(_) | Self::Spatial(_) => Ok(None),
        }
    }

    pub(super) fn title(&self) -> &'static str {
        match self {
            Self::Demo(demo) if demo.is_bird() => "Aurora Bird · Space, ↑ or touch to flap",
            Self::Authored(_) => "Aurora Player · P pause/resume · Tab switch scene",
            Self::Demo(_) => "Aurora Player",
            Self::Spatial(_) => "Aurora · 3D Drop Test",
        }
    }

    pub(super) fn controls(&mut self, event: &WindowEvent) {
        if let Self::Authored(game) = self
            && let WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } = event
            && event.state == ElementState::Pressed
            && !event.repeat
        {
            match event.physical_key {
                PhysicalKey::Code(KeyCode::KeyP) if game.director.is_paused() => {
                    game.director.resume()
                }
                PhysicalKey::Code(KeyCode::KeyP) => game.director.pause(),
                PhysicalKey::Code(KeyCode::Tab) if game.director.scene_count() > 1 => {
                    game.pending_scene =
                        Some((game.director.current() + 1) % game.director.scene_count());
                }
                _ => {}
            }
        }
    }
}

fn spawn_scene_program(program: &SceneProgram) -> Result<Option<Session>> {
    anyhow::ensure!(
        program.executable.is_none() == program.bindings.is_empty(),
        "A packaged scene program must have an executable and its bindings, or neither"
    );
    program
        .executable
        .as_ref()
        .map(|path| Session::spawn(&Script::Native(path.clone())))
        .transpose()
}

pub(super) struct Authored {
    director: Director,
    gameplay: Option<gameplay::Gameplay>,
    settings: settings::GameSettings,
    profile: Option<PathBuf>,
    /// White-pixel sprite drawn over the game frame while input is blocked.
    loading: Option<Sprite>,
    pending_config: BTreeMap<u32, Vec<u8>>,
    silent: bool,
    script: Option<Session>,
    pending_scene: Option<usize>,
    script_nodes: Vec<usize>,
    scene_programs: Vec<SceneProgram>,
    bindings: BTreeMap<u32, RuntimeBinding>,
    /// Hit node reference and last position captured for each active pointer identity.
    pointers: BTreeMap<u64, (u32, [f32; 2])>,
    mutations: VecDeque<Mutation>,
    pending_text: BTreeMap<u32, Vec<u8>>,
    pending_sprites: BTreeMap<u32, Vec<u8>>,
    pending_audio: BTreeMap<u32, Vec<u8>>,
    pending_prefabs: BTreeMap<u32, Vec<u8>>,
    pending_raycasts: BTreeMap<u32, Vec<u8>>,
    pending_overlaps: BTreeMap<u32, Vec<u8>>,
    dynamic_frames: BTreeMap<u64, crate::assets::AssetId>,
    dynamic_audio: BTreeMap<u64, AudioClip>,
    dynamic_prefabs: BTreeMap<u64, u32>,
    asset_refs: BTreeMap<u64, u32>,
    next_asset: u64,
    /// Binding, request, handle, failure flag and asset kind (1 sprite, 2 audio, 3 prefab).
    pending_asset_events: Vec<(u32, u64, u64, bool, u32)>,
    pending_raycast_events: Vec<(u32, u64, f32, [f32; 3], bool)>,
    pending_overlap_events: Vec<(u32, u64, u32, bool)>,
    next_binding: u32,
    tearing_down: bool,
    /// Script references whose local transform the script already knows.
    transformed: BTreeSet<u32>,
}

impl Authored {
    fn start(&mut self) -> Result<()> {
        if self.script.is_none() {
            return Ok(());
        }
        self.bindings.clear();
        self.pointers.clear();
        self.mutations.clear();
        self.pending_text.clear();
        self.pending_sprites.clear();
        self.pending_audio.clear();
        self.pending_prefabs.clear();
        self.pending_raycasts.clear();
        self.pending_overlaps.clear();
        self.dynamic_frames.clear();
        self.dynamic_audio.clear();
        self.dynamic_prefabs.clear();
        self.asset_refs.clear();
        self.pending_asset_events.clear();
        self.pending_raycast_events.clear();
        self.pending_overlap_events.clear();
        self.next_asset = 0x8000_0000;
        self.transformed.clear();
        for (index, &owner) in self.script_nodes.iter().enumerate() {
            let node = self
                .director
                .node(owner)
                .with_context(|| format!("Component binding {index} needs scene node {owner}"))?;
            self.bindings
                .insert(u32::try_from(index)?, RuntimeBinding::new(node, false));
        }
        self.next_binding = u32::try_from(self.bindings.len())?;
        if let Some((&binding, entry)) = self.bindings.iter().next() {
            let node = entry.node;
            self.dispatch_raw(binding, node, Kind::SceneStart, 0, None)?;
        }
        self.sync_node_transforms()?;
        if let Some(best) = self.gameplay.as_ref().map(|gameplay| gameplay.best) {
            for binding in self.bindings.keys().copied().collect::<Vec<_>>() {
                let node = self.bindings[&binding].node;
                self.dispatch_to(binding, node, Kind::RestoreScore, u64::from(best))?;
            }
        }
        let (config, secrets): (Vec<_>, Vec<_>) = (
            self.settings.config_entries().map(pack_entry).collect(),
            self.settings.secret_entries().map(pack_entry).collect(),
        );
        for (index, entry) in config.iter().enumerate() {
            self.dispatch_entry(Kind::ConfigValue, index as u32, entry)?;
        }
        for (index, entry) in secrets.iter().enumerate() {
            self.dispatch_entry(Kind::SecretValue, index as u32, entry)?;
        }
        self.sync_lifecycle()
    }

    /// Sends one `name\0value` entry to every running binding in 12-byte chunks.
    fn dispatch_entry(&mut self, kind: Kind, entry: u32, text: &str) -> Result<()> {
        let bytes = text.as_bytes();
        anyhow::ensure!(
            bytes.len() <= 1512,
            "Settings entry exceeds the 1512-byte limit"
        );
        for binding in self.bindings.keys().copied().collect::<Vec<_>>() {
            let node = self.bindings[&binding].node;
            self.dispatch_raw(binding, node, kind, u64::from(entry), Some([0.0; 2]))?;
            for chunk in bytes.chunks(12) {
                let mut packed = [0_u64; 2];
                for (index, byte) in chunk.iter().enumerate() {
                    packed[index / 6] |= u64::from(*byte) << ((index % 6) * 8);
                }
                let detail = (u64::from(chunk.len() as u32) << 32) | u64::from(entry);
                let coordinate = [
                    packed[0] as f64,
                    packed[1] as f64 + f64::from(chunk.len() as u32) * 2_f64.powi(48),
                ];
                self.dispatch_raw(binding, node, kind, detail, Some(coordinate))?;
            }
            self.dispatch_raw(
                binding,
                node,
                kind,
                13_u64 << 32 | u64::from(entry),
                Some([0.0; 2]),
            )?;
        }
        Ok(())
    }

    /// Sends the local transform of every scene node the script has not seen yet.
    ///
    /// Scripts read authored transforms for tweens and position accessors. The
    /// stream runs after SceneStart and picks up prefab or script-created nodes
    /// on a later tick without repeating known nodes.
    fn sync_node_transforms(&mut self) -> Result<()> {
        if self.script.is_none() {
            return Ok(());
        }
        let Some((binding, host)) = self.running_binding()? else {
            return Ok(());
        };
        let nodes = self.director.scene().snapshot()?;
        let worlds = crate::scene::spatial::matrices(&nodes);
        for state in nodes {
            let Some(reference) = self.director.script_reference(state.id) else {
                continue;
            };
            if !self.transformed.insert(reference) {
                continue;
            }
            self.dispatch_raw(
                binding,
                host,
                Kind::NodeTransform,
                u64::from(reference),
                Some(state.local.position.map(f64::from)),
            )?;
            let detail = u64::from(reference) | (u64::from(state.local.rotation.to_bits()) << 32);
            self.dispatch_raw(
                binding,
                host,
                Kind::NodeScaleRotation,
                detail,
                Some(state.local.scale.map(f64::from)),
            )?;
            if let Some(spatial) = state.spatial {
                self.dispatch_spatial(
                    binding,
                    host,
                    Kind::NodeTransform3D,
                    u64::from(reference),
                    spatial.position,
                )?;
                self.dispatch_spatial(
                    binding,
                    host,
                    Kind::NodeRotation3D,
                    u64::from(reference),
                    spatial.rotation,
                )?;
                self.dispatch_spatial(
                    binding,
                    host,
                    Kind::NodeScale3D,
                    u64::from(reference),
                    spatial.scale,
                )?;
            }
            if let Some(world) = worlds.get(&state.id) {
                let (_, rotation, position) = world.to_scale_rotation_translation();
                let (x, y, z) = rotation.to_euler(glam::EulerRot::XYZ);
                self.dispatch_spatial(
                    binding,
                    host,
                    Kind::NodeWorldTransform3D,
                    u64::from(reference),
                    position.to_array(),
                )?;
                self.dispatch_spatial(
                    binding,
                    host,
                    Kind::NodeWorldRotation3D,
                    u64::from(reference),
                    [x.to_degrees(), y.to_degrees(), z.to_degrees()],
                )?;
            }
        }
        Ok(())
    }

    /// Sends one spatial transform stream and requires an acknowledgement.
    fn dispatch_spatial(
        &mut self,
        binding: u32,
        node: EntityId,
        kind: Kind,
        detail: u64,
        vector: [f32; 3],
    ) -> Result<()> {
        let reply = self
            .script
            .as_mut()
            .context("Script disconnected")?
            .exchange_control(kind, binding, node.0, vector.map(f64::from), detail)?;
        ensure!(
            reply.command.is_none(),
            "Spatial transform streams require an acknowledgement"
        );
        self.bindings
            .get_mut(&binding)
            .context("Unknown component binding")?
            .lifecycle
            .enabled = reply.enabled;
        Ok(())
    }

    /// Advances script tweens once for this simulation tick.
    fn dispatch_tween(&mut self) -> Result<()> {
        if self.script.is_none() {
            return Ok(());
        }
        self.sync_node_transforms()?;
        let Some((binding, node)) = self.running_binding()? else {
            return Ok(());
        };
        self.dispatch_raw(binding, node, Kind::Tween, 0, None)?;
        self.settle_mutations()
    }

    /// Returns the first binding that is currently running.
    fn running_binding(&self) -> Result<Option<(u32, EntityId)>> {
        for (&binding, entry) in &self.bindings {
            if entry
                .lifecycle
                .runs(self.director.scene().active_in_hierarchy(entry.node)?)
            {
                return Ok(Some((binding, entry.node)));
            }
        }
        Ok(None)
    }

    fn sync_lifecycle(&mut self) -> Result<()> {
        if self.script.is_none() {
            return Ok(());
        }
        let phases: [fn(&mut Binding, bool) -> Option<Kind>; 3] =
            [Binding::load, Binding::change_activation, Binding::start];
        for phase in phases {
            for binding in self.bindings.keys().copied().collect::<Vec<_>>() {
                let Some(entry) = self.bindings.get_mut(&binding) else {
                    continue;
                };
                let node = entry.node;
                let active = self.director.scene().active_in_hierarchy(node)?;
                if let Some(kind) = phase(&mut entry.lifecycle, active) {
                    self.dispatch_to(binding, node, kind, 0)?;
                }
            }
        }
        Ok(())
    }

    fn end_scene(&mut self) -> Result<()> {
        if self.script.is_none() {
            return Ok(());
        }
        self.tearing_down = true;
        let bindings: Vec<_> = self.bindings.keys().rev().copied().collect();
        let result = self.release_bindings(&bindings);
        self.tearing_down = false;
        result?;
        for (reference, node) in self.director.script_dynamic_nodes() {
            self.control(
                Kind::ReleasePrefabNode,
                0,
                node,
                [0.0; 2],
                u64::from(reference),
            )?;
        }
        self.mutations.clear();
        self.pointers.clear();
        self.pending_config.clear();
        self.loading = None;
        Ok(())
    }

    fn poll_disabled(&mut self) -> Result<()> {
        if self.script.is_some() {
            for binding in self.bindings.keys().copied().collect::<Vec<_>>() {
                let Some(entry) = self.bindings.get(&binding) else {
                    continue;
                };
                let node = entry.node;
                if !entry.lifecycle.enabled && self.director.scene().active_in_hierarchy(node)? {
                    self.dispatch_to(binding, node, Kind::PollEnabled, 0)?;
                }
            }
        }
        Ok(())
    }

    fn dispatch(&mut self, kind: Kind, detail: u64) -> Result<()> {
        self.sync_lifecycle()?;
        if self.script.is_some() {
            for binding in self.bindings.keys().copied().collect::<Vec<_>>() {
                let Some(entry) = self.bindings.get(&binding) else {
                    continue;
                };
                let node = entry.node;
                if entry
                    .lifecycle
                    .runs(self.director.scene().active_in_hierarchy(node)?)
                {
                    self.dispatch_to(binding, node, kind, detail)?;
                }
            }
        }
        Ok(())
    }

    fn dispatch_to(&mut self, binding: u32, node: EntityId, kind: Kind, detail: u64) -> Result<()> {
        self.dispatch_raw(binding, node, kind, detail, None)?;
        self.settle_mutations()
    }

    /// Sends one pointer phase to every running binding with normalized coordinates.
    fn dispatch_pointer_phase(&mut self, kind: Kind, id: u64, position: [f32; 2]) -> Result<()> {
        self.sync_lifecycle()?;
        if self.script.is_some() {
            for binding in self.bindings.keys().copied().collect::<Vec<_>>() {
                let Some(entry) = self.bindings.get(&binding) else {
                    continue;
                };
                let node = entry.node;
                if entry
                    .lifecycle
                    .runs(self.director.scene().active_in_hierarchy(node)?)
                {
                    self.dispatch_raw(binding, node, kind, id, Some(position.map(f64::from)))?;
                    self.settle_mutations()?;
                }
            }
        }
        Ok(())
    }

    /// Chooses a running binding for node-targeted input.
    ///
    /// A binding on the hit node keeps `this.node` mutations on that node. When the
    /// node has no running component, any running binding provides the callback
    /// context for listeners that other components attached to the node.
    fn binding_for_reference(&self, reference: u32) -> Result<Option<(u32, EntityId)>> {
        let target = self.director.script_node(reference).ok();
        let mut fallback = None;
        for (&binding, entry) in &self.bindings {
            let active = self.director.scene().active_in_hierarchy(entry.node)?;
            if !entry.lifecycle.runs(active) {
                continue;
            }
            if Some(entry.node) == target {
                return Ok(Some((binding, entry.node)));
            }
            fallback.get_or_insert((binding, entry.node));
        }
        Ok(fallback)
    }

    /// Sends wheel deltas to one hit node, then retries after commands settle.
    fn dispatch_pointer_wheel(&mut self, reference: u32, delta: [f32; 2]) -> Result<()> {
        if self.script.is_none() {
            return Ok(());
        }
        let Some((binding, owner)) = self.binding_for_reference(reference)? else {
            return Ok(());
        };
        self.dispatch_raw(
            binding,
            owner,
            Kind::PointerNodeWheel,
            u64::from(reference),
            Some(delta.map(f64::from)),
        )?;
        self.settle_mutations()
    }

    fn dispatch_raw(
        &mut self,
        binding: u32,
        node: EntityId,
        kind: Kind,
        detail: u64,
        coordinates: Option<[f64; 2]>,
    ) -> Result<()> {
        self.dispatch_raw_z(binding, node, kind, detail, coordinates, None)
    }

    fn dispatch_raw_z(
        &mut self,
        binding: u32,
        node: EntityId,
        mut kind: Kind,
        mut detail: u64,
        mut coordinates: Option<[f64; 2]>,
        z: Option<f64>,
    ) -> Result<()> {
        for _ in 0..=128 {
            let transform = self.director.scene().transform(node)?;
            let position = coordinates.unwrap_or_else(|| transform.position.map(f64::from));
            let reply = self
                .script
                .as_mut()
                .context("Script disconnected")?
                .exchange_control(
                    kind,
                    binding,
                    node.0,
                    [position[0], position[1], z.unwrap_or(0.0)],
                    detail,
                )?;
            self.bindings
                .get_mut(&binding)
                .context("Unknown component binding")?
                .lifecycle
                .enabled = reply.enabled;
            let Some(command) = reply.command else {
                return Ok(());
            };
            if !self.apply(node, command)? {
                return Ok(());
            }
            // The remaining replies describe the scene after the applied command,
            // so they use the current transform rather than the event coordinates.
            kind = Kind::FlushCommands;
            detail = 0;
            coordinates = None;
        }
        anyhow::bail!("Script scene command limit exceeded")
    }

    fn target(&self, index: usize) -> Result<EntityId> {
        self.director.script_node(index.try_into()?)
    }

    fn require_gameplay(&mut self) -> Result<&mut gameplay::Gameplay> {
        if self.gameplay.is_none() {
            self.gameplay = Some(gameplay::Gameplay::empty(self.silent));
        }
        self.gameplay
            .as_mut()
            .context("Gameplay resources are unavailable")
    }

    fn append_asset_chunk(
        pending: &mut BTreeMap<u32, Vec<u8>>,
        request: u32,
        bytes: [u8; 12],
        length: u8,
    ) -> Result<()> {
        let path = pending
            .get_mut(&request)
            .context("Asset chunk has no matching start")?;
        let length = usize::from(length);
        anyhow::ensure!(
            (1..=bytes.len()).contains(&length) && path.len() + length <= 4096,
            "Asset path exceeds its size limit"
        );
        path.extend_from_slice(&bytes[..length]);
        Ok(())
    }

    fn finish_asset_path(pending: &mut BTreeMap<u32, Vec<u8>>, request: u32) -> Result<String> {
        let bytes = pending
            .remove(&request)
            .context("Asset load has no matching start")?;
        ensure!(!bytes.is_empty(), "Asset path is empty");
        String::from_utf8(bytes).context("Asset path is not UTF-8")
    }

    fn binding_for_node(&self, node: EntityId) -> Result<u32> {
        self.bindings
            .iter()
            .find(|(_, entry)| entry.node == node)
            .map(|(&binding, _)| binding)
            .context("Asset load has no owning component")
    }

    fn allocate_asset(&mut self) -> Result<u64> {
        let handle = self.next_asset;
        ensure!(handle < 0xffff_ffff, "Asset handle space exhausted");
        self.next_asset += 1;
        *self.asset_refs.entry(handle).or_insert(0) += 1;
        Ok(handle)
    }

    fn apply(&mut self, node: EntityId, command: Command) -> Result<bool> {
        match command {
            Command::SetPosition(position) => {
                self.move_node(node, position)?;
            }
            Command::Pause => self.director.pause(),
            Command::Resume => self.director.resume(),
            Command::ChangeScene(index) => {
                anyhow::ensure!(
                    index < self.director.scene_count(),
                    "Unknown scene index {index}"
                );
                self.pending_scene = Some(index);
            }
            Command::PlayOneShot(id) => {
                let clip = self.dynamic_audio.get(&id).cloned();
                self.require_gameplay()?.sound(id, clip.as_ref())?
            }
            Command::SetLinearVelocity(velocity) => {
                self.director.set_linear_velocity(node, velocity)?
            }
            Command::SetLinearVelocity3D(velocity) => {
                self.director.set_linear_velocity_3d(node, velocity)?
            }
            Command::SetAngularVelocity3D(velocity) => {
                self.director.set_angular_velocity_3d(node, velocity)?
            }
            Command::RaycastStart(request) => {
                anyhow::ensure!(
                    self.pending_raycasts.insert(request, Vec::new()).is_none(),
                    "Raycast is already in progress"
                );
                return Ok(true);
            }
            Command::RaycastChunk(request, bytes, length) => {
                Self::append_asset_chunk(&mut self.pending_raycasts, request, bytes, length)?;
                return Ok(true);
            }
            Command::RaycastEnd(request) => {
                let bytes = self
                    .pending_raycasts
                    .remove(&request)
                    .context("Raycast has no matching start")?;
                anyhow::ensure!(bytes.len() == 44, "Raycast payload must be 44 bytes");
                let mut values = [0.0_f32; 11];
                for (index, value) in values.iter_mut().enumerate() {
                    let start = index * 4;
                    let bits = u32::from_le_bytes(
                        bytes[start..start + 4]
                            .try_into()
                            .context("Raycast chunk")?,
                    );
                    *value = f32::from_bits(bits);
                }
                let binding = self.binding_for_node(node)?;
                let half = [values[7], values[8], values[9]];
                let hit = if values[0] == 2.0 {
                    self.director.physics3d_shape_cast(
                        half,
                        [values[1], values[2], values[3]],
                        [values[4], values[5], values[6]],
                        values[10],
                    )?
                } else {
                    self.director.physics3d_raycast(
                        [values[1], values[2], values[3]],
                        [values[4], values[5], values[6]],
                        values[10],
                    )?
                };
                self.pending_raycast_events.push(match hit {
                    Some(hit) => (binding, u64::from(request), hit.distance, hit.normal, false),
                    None => (binding, u64::from(request), 0.0, [0.0; 3], true),
                });
                return Ok(true);
            }
            Command::OverlapStart(request) => {
                anyhow::ensure!(
                    self.pending_overlaps.insert(request, Vec::new()).is_none(),
                    "Overlap query is already in progress"
                );
                return Ok(true);
            }
            Command::OverlapChunk(request, bytes, length) => {
                Self::append_asset_chunk(&mut self.pending_overlaps, request, bytes, length)?;
                return Ok(true);
            }
            Command::OverlapEnd(request) => {
                let bytes = self
                    .pending_overlaps
                    .remove(&request)
                    .context("Overlap query has no matching start")?;
                anyhow::ensure!(bytes.len() == 24, "Overlap payload must be 24 bytes");
                let mut values = [0.0_f32; 6];
                for (index, value) in values.iter_mut().enumerate() {
                    let start = index * 4;
                    let bits = u32::from_le_bytes(
                        bytes[start..start + 4]
                            .try_into()
                            .context("Overlap chunk")?,
                    );
                    *value = f32::from_bits(bits);
                }
                let binding = self.binding_for_node(node)?;
                let hits = self.director.physics3d_overlaps(
                    [values[0], values[1], values[2]],
                    [values[3], values[4], values[5]],
                )?;
                if hits.is_empty() {
                    self.pending_overlap_events
                        .push((binding, u64::from(request), 0, true));
                } else {
                    for (index, entity) in hits.iter().enumerate() {
                        let reference = self.director.script_reference(*entity).unwrap_or(u32::MAX);
                        self.pending_overlap_events.push((
                            binding,
                            u64::from(request),
                            reference,
                            index + 1 == hits.len(),
                        ));
                    }
                }
                return Ok(true);
            }
            Command::MoveNode(index, position) => {
                self.move_node(self.target(index)?, position)?;
                return Ok(true);
            }
            Command::SetTextStart(reference) => {
                self.target(reference as usize)?;
                anyhow::ensure!(
                    self.pending_text.insert(reference, Vec::new()).is_none(),
                    "Text replacement is already in progress"
                );
                return Ok(true);
            }
            Command::SetTextChunk(reference, bytes, length) => {
                let text = self
                    .pending_text
                    .get_mut(&reference)
                    .context("Text chunk has no matching start")?;
                let length = usize::from(length);
                anyhow::ensure!(
                    (1..=bytes.len()).contains(&length) && text.len() + length <= 1512,
                    "Script text exceeds its size limit"
                );
                text.extend_from_slice(&bytes[..length]);
                return Ok(true);
            }
            Command::SetTextEnd(reference) => {
                let bytes = self
                    .pending_text
                    .remove(&reference)
                    .context("Text end has no matching start")?;
                let text = String::from_utf8(bytes).context("Script text is not valid UTF-8")?;
                self.director
                    .set_text(self.target(reference as usize)?, text)?;
                return Ok(true);
            }
            Command::ShowLoading(visible) => {
                if visible && self.loading.is_none() {
                    let texture = self
                        .director
                        .assets_mut()
                        .generated(image::RgbaImage::from_pixel(1, 1, image::Rgba([255; 4])));
                    let size = self.director.display().resolution.map(|value| value as f32);
                    self.loading = Some(Sprite {
                        texture,
                        size,
                        emitter: None,
                    });
                } else if !visible {
                    self.loading = None;
                }
            }
            Command::ConfigSetStart(entry) => {
                anyhow::ensure!(
                    self.pending_config.insert(entry, Vec::new()).is_none(),
                    "Config update is already in progress"
                );
                return Ok(true);
            }
            Command::ConfigSetChunk(entry, bytes, length) => {
                let value = self
                    .pending_config
                    .get_mut(&entry)
                    .context("Config chunk has no matching start")?;
                let length = usize::from(length);
                anyhow::ensure!(
                    (1..=bytes.len()).contains(&length) && value.len() + length <= 1512,
                    "Script config exceeds its size limit"
                );
                value.extend_from_slice(&bytes[..length]);
                return Ok(true);
            }
            Command::ConfigSetEnd(entry) => {
                let bytes = self
                    .pending_config
                    .remove(&entry)
                    .context("Config end has no matching start")?;
                let separator = bytes
                    .iter()
                    .position(|byte| *byte == 0)
                    .context("Config entry has no name")?;
                let name = String::from_utf8(bytes[..separator].to_vec())?;
                let value = String::from_utf8(bytes[separator + 1..].to_vec())?;
                self.settings.set_config(&name, &value)?;
                return Ok(true);
            }
            Command::ScaleNode(index, scale) => {
                let target = self.target(index)?;
                let mut transform = self.director.scene().transform(target)?;
                transform.scale = scale;
                self.director.scene_mut().set_transform(target, transform)?;
                return Ok(true);
            }
            Command::RotateNode(index, angle) => {
                let target = self.target(index)?;
                let mut transform = self.director.scene().transform(target)?;
                transform.rotation = angle;
                self.director.scene_mut().set_transform(target, transform)?;
                return Ok(true);
            }
            Command::ShowNode(index, visible) => {
                let target = self.target(index)?;
                self.director.scene_mut().set_visible(target, visible)?;
                return Ok(true);
            }
            Command::SelectFrame(index, frame) => {
                let target = self.target(index)?;
                if frame >= 0x8000_0000 {
                    let id = *self
                        .dynamic_frames
                        .get(&(frame as u64))
                        .context("Unknown asset handle")?;
                    let mut sprite = self
                        .director
                        .scene()
                        .sprite(target)?
                        .context("Missing sprite")?;
                    sprite.texture = id;
                    self.director.scene_mut().set_sprite(target, sprite)?;
                } else {
                    self.gameplay
                        .as_ref()
                        .context("Scene has no gameplay resources")?
                        .select_frame(&mut self.director, target, frame)?;
                }
                return Ok(true);
            }
            Command::LoadSpriteStart(request) => {
                anyhow::ensure!(
                    self.pending_sprites.insert(request, Vec::new()).is_none(),
                    "Sprite load is already in progress"
                );
                return Ok(true);
            }
            Command::LoadSpriteChunk(request, bytes, length) => {
                Self::append_asset_chunk(&mut self.pending_sprites, request, bytes, length)?;
                return Ok(true);
            }
            Command::LoadSpriteEnd(request) => {
                let relative = Self::finish_asset_path(&mut self.pending_sprites, request)?;
                let binding = self.binding_for_node(node)?;
                let root = parent_dir(self.director.scene_path()).to_path_buf();
                let target = root.join(&relative);
                anyhow::ensure!(target.starts_with(&root), "Asset path escapes the project");
                let result = self.director.assets_mut().import_file(&target);
                let (handle, failed) = match result {
                    Ok(id) => {
                        let handle = self.allocate_asset()?;
                        self.dynamic_frames.insert(handle, id);
                        (handle, false)
                    }
                    Err(_) => (0, true),
                };
                self.pending_asset_events
                    .push((binding, u64::from(request), handle, failed, 1));
                return Ok(true);
            }
            Command::LoadAudioStart(request) => {
                anyhow::ensure!(
                    self.pending_audio.insert(request, Vec::new()).is_none(),
                    "Audio load is already in progress"
                );
                return Ok(true);
            }
            Command::LoadAudioChunk(request, bytes, length) => {
                Self::append_asset_chunk(&mut self.pending_audio, request, bytes, length)?;
                return Ok(true);
            }
            Command::LoadAudioEnd(request) => {
                let relative = Self::finish_asset_path(&mut self.pending_audio, request)?;
                let binding = self.binding_for_node(node)?;
                let root = parent_dir(self.director.scene_path()).to_path_buf();
                let target = root.join(&relative);
                anyhow::ensure!(target.starts_with(&root), "Asset path escapes the project");
                let result = (|| -> Result<AudioClip> {
                    use std::io::Read;
                    let mut bytes = Vec::new();
                    std::fs::File::open(&target)?
                        .take(16_777_217)
                        .read_to_end(&mut bytes)?;
                    ensure!(bytes.len() <= 16_777_216, "Scene sound exceeds 16 MiB");
                    AudioClip::decode(bytes)
                })();
                let (handle, failed) = match result {
                    Ok(clip) => {
                        let handle = self.allocate_asset()?;
                        self.dynamic_audio.insert(handle, clip);
                        (handle, false)
                    }
                    Err(_) => (0, true),
                };
                self.pending_asset_events
                    .push((binding, u64::from(request), handle, failed, 2));
                return Ok(true);
            }
            Command::LoadPrefabStart(request) => {
                anyhow::ensure!(
                    self.pending_prefabs.insert(request, Vec::new()).is_none(),
                    "Prefab load is already in progress"
                );
                return Ok(true);
            }
            Command::LoadPrefabChunk(request, bytes, length) => {
                Self::append_asset_chunk(&mut self.pending_prefabs, request, bytes, length)?;
                return Ok(true);
            }
            Command::LoadPrefabEnd(request) => {
                let relative = Self::finish_asset_path(&mut self.pending_prefabs, request)?;
                let binding = self.binding_for_node(node)?;
                let root = parent_dir(self.director.scene_path()).to_path_buf();
                let target = root.join(&relative);
                anyhow::ensure!(target.starts_with(&root), "Asset path escapes the project");
                let result = crate::save::PrefabFile::load(&target);
                let (handle, failed) = match result {
                    Ok(_) => {
                        let handle = self.allocate_asset()?;
                        let catalog = self.director.register_script_prefab(&target)?;
                        self.dynamic_prefabs.insert(handle, catalog);
                        (handle, false)
                    }
                    Err(_) => (0, true),
                };
                self.pending_asset_events
                    .push((binding, u64::from(request), handle, failed, 3));
                return Ok(true);
            }
            Command::ReleaseAsset(handle) => {
                if let std::collections::btree_map::Entry::Occupied(mut entry) =
                    self.asset_refs.entry(handle)
                {
                    let remaining = entry.get_mut();
                    *remaining = remaining.saturating_sub(1);
                    if *remaining == 0 {
                        entry.remove();
                        self.dynamic_frames.remove(&handle);
                        self.dynamic_audio.remove(&handle);
                        self.dynamic_prefabs.remove(&handle);
                    }
                }
                return Ok(true);
            }
            Command::QueueSound(id) => {
                let clip = self.dynamic_audio.get(&id).cloned();
                self.require_gameplay()?.sound(id, clip.as_ref())?;
                return Ok(true);
            }
            Command::RecordScore(score) => {
                self.require_gameplay()?.record(score)?;
                return Ok(true);
            }
            Command::CreateNode(reference, parent) => {
                anyhow::ensure!(
                    !self.tearing_down,
                    "Cannot create a node during scene teardown"
                );
                let clone = self.director.create_script_node(reference, parent, None)?;
                self.mutations.push_back(Mutation::Attach(clone));
                return Ok(true);
            }
            Command::InstantiatePrefab(reference, parent, asset) => {
                anyhow::ensure!(
                    !self.tearing_down,
                    "Cannot instantiate a prefab during scene teardown"
                );
                let catalog = if asset >= 0x8000_0000 {
                    *self
                        .dynamic_prefabs
                        .get(&u64::from(asset))
                        .context("Unknown prefab handle")?
                } else {
                    asset
                };
                let clone = self
                    .director
                    .create_script_node(reference, parent, Some(catalog))?;
                self.mutations.push_back(Mutation::Attach(clone));
                return Ok(true);
            }
            Command::ReparentNode(reference, parent) => {
                if !self.tearing_down {
                    let node = self.director.reparent_script_node(reference, parent)?;
                    self.mutations.push_back(Mutation::Reparent {
                        reference,
                        node,
                        parent,
                    });
                }
                return Ok(true);
            }
            Command::AddComponent(reference, factory) => {
                anyhow::ensure!(!self.tearing_down, "Cannot add a component during teardown");
                let node = self.director.script_node(reference)?;
                self.mutations.push_back(Mutation::AddComponent {
                    node,
                    reference,
                    factory,
                });
                return Ok(true);
            }
            Command::RemoveComponent(binding, reference) => {
                anyhow::ensure!(
                    !self.tearing_down,
                    "Cannot remove a component during teardown"
                );
                let node = self.director.script_node(reference)?;
                anyhow::ensure!(
                    self.bindings
                        .get(&binding)
                        .is_some_and(|entry| entry.node == node),
                    "Component does not belong to node"
                );
                self.mutations
                    .push_back(Mutation::RemoveComponent { binding });
                return Ok(true);
            }
            Command::DestroyNode(reference) => {
                let node = self.director.dynamic_script_node(reference)?;
                if !self.tearing_down {
                    self.mutations
                        .push_back(Mutation::Detach { reference, node });
                }
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn move_node(&mut self, node: EntityId, position: [f32; 2]) -> Result<()> {
        let mut transform = self.director.scene().transform(node)?;
        transform.position = position;
        self.director.scene_mut().set_transform(node, transform)
    }

    fn tick(&mut self, input: &[InputEvent]) -> Result<()> {
        if let Some(index) = self.pending_scene.take() {
            let (next, entered) = self.director.prepare_scene_change(index)?;
            let destination_program = self.scene_programs.get(index);
            let destination_nodes = destination_program
                .map(|program| program.bindings.as_slice())
                .or_else(|| self.script.as_ref().map(|_| self.script_nodes.as_slice()));
            if let Some(nodes) = destination_nodes {
                for &node in nodes {
                    anyhow::ensure!(
                        node < next.ids.len(),
                        "A script binding needs scene node {node}"
                    );
                }
            }
            let replacement = destination_program
                .map(|program| {
                    let script = spawn_scene_program(program)?;
                    Ok::<_, anyhow::Error>((script, program.bindings.clone()))
                })
                .transpose()?;
            if replacement.is_some()
                && let Some(script) = &mut self.script
            {
                script.begin_shutdown();
            }
            self.end_scene()?;
            self.director.commit_scene_change(index, next, entered)?;
            if let Some((script, nodes)) = replacement {
                self.script = script;
                self.script_nodes = nodes;
                self.bindings.clear();
            }
            self.gameplay =
                gameplay::Gameplay::load(&mut self.director, self.silent, self.profile.as_deref())?;
            self.start()?;
        }
        self.poll_disabled()?;
        self.sync_lifecycle()?;
        for event in input {
            // A loading overlay swallows gameplay input until a script clears it.
            if self.loading.is_some() {
                break;
            }
            match event {
                InputEvent::KeyDown(key) => self.dispatch(Kind::KeyDown, *key as u64)?,
                InputEvent::KeyUp(key) => self.dispatch(Kind::KeyUp, *key as u64)?,
                // Legacy coordinate-free touch input becomes a centre-frame tap
                // so hit testing and pointer node events still run headlessly.
                InputEvent::TouchStart => {
                    let viewport = self.director.display().preview_resolution();
                    self.dispatch_pointer_phase(Kind::TouchStart, 0, [0.5, 0.5])?;
                    self.dispatch_pointer(Kind::PointerTouch, 0, [0.5, 0.5], viewport)?;
                }
                InputEvent::PointerStart {
                    position,
                    viewport,
                    id,
                } => {
                    self.dispatch_pointer_phase(Kind::TouchStart, *id, *position)?;
                    self.dispatch_pointer(Kind::PointerTouch, *id, *position, *viewport)?;
                }
                InputEvent::PointerMove {
                    position,
                    viewport,
                    id,
                } => {
                    self.dispatch_pointer_phase(Kind::PointerMove, *id, *position)?;
                    self.dispatch_pointer(Kind::PointerNodeMove, *id, *position, *viewport)?;
                }
                InputEvent::PointerEnd {
                    position,
                    viewport,
                    id,
                } => {
                    self.dispatch_pointer_phase(Kind::PointerEnd, *id, *position)?;
                    self.dispatch_pointer(Kind::PointerNodeEnd, *id, *position, *viewport)?;
                    self.pointers.remove(id);
                }
                InputEvent::Wheel {
                    position,
                    delta,
                    viewport,
                } => {
                    // Hit test before any handler moves the scene under the cursor.
                    let reference = if self.script.is_some() && !viewport.contains(&0) {
                        self.pointer_target(*position, *viewport)?
                    } else {
                        None
                    };
                    for binding in self.bindings.keys().copied().collect::<Vec<_>>() {
                        let Some(entry) = self.bindings.get(&binding) else {
                            continue;
                        };
                        let node = entry.node;
                        if entry
                            .lifecycle
                            .runs(self.director.scene().active_in_hierarchy(node)?)
                        {
                            self.dispatch_raw(
                                binding,
                                node,
                                Kind::MouseWheel,
                                0,
                                Some(delta.map(f64::from)),
                            )?;
                            self.settle_mutations()?;
                        }
                    }
                    if let Some(reference) = reference {
                        self.dispatch_pointer_wheel(reference, *delta)?;
                    }
                }
                InputEvent::PointerCancel { id } => {
                    let position = self
                        .pointers
                        .get(id)
                        .map_or([0.0; 2], |&(_, position)| position);
                    self.dispatch_pointer_phase(Kind::PointerCancel, *id, position)?;
                    self.dispatch_pointer(Kind::PointerNodeCancel, *id, position, [0; 2])?;
                    self.pointers.remove(id);
                }
                InputEvent::CancelPointers => {
                    // Cancel every finger still touching the scene. A canceled
                    // contact must not keep a node pressed after focus returns.
                    for id in self.pointers.keys().copied().collect::<Vec<_>>() {
                        let position = self
                            .pointers
                            .get(&id)
                            .map_or([0.0; 2], |&(_, position)| position);
                        self.dispatch_pointer_phase(Kind::PointerCancel, id, position)?;
                        self.dispatch_pointer(Kind::PointerNodeCancel, id, position, [0; 2])?;
                        self.pointers.remove(&id);
                    }
                }
            }
        }
        if self.director.is_paused() {
            return Ok(());
        }
        self.dispatch(Kind::FixedUpdate, 0)?;
        if self.director.is_paused() {
            return Ok(());
        }
        for code in self.director.step()? {
            self.dispatch(Kind::AnimationEvent, u64::from(code))?;
        }
        self.dispatch(Kind::Update, 0)?;
        self.dispatch_tween()?;
        self.dispatch(Kind::Schedule, 0)?;
        self.dispatch(Kind::ProcessResources, 0)?;
        self.dispatch(Kind::LateUpdate, 0)?;
        if let Some(gameplay) = &mut self.gameplay {
            gameplay.step_audio();
        }
        self.dispatch_contacts()?;
        self.flush_asset_events()?;
        self.flush_raycast_events()?;
        self.flush_overlap_events()
    }

    /// Delivers overlap hits to their requesting components.
    fn flush_overlap_events(&mut self) -> Result<()> {
        let events = std::mem::take(&mut self.pending_overlap_events);
        for (binding, request, reference, last) in events {
            let Some(node) = self.bindings.get(&binding).map(|entry| entry.node) else {
                continue;
            };
            let reply = self
                .script
                .as_mut()
                .context("Script disconnected")?
                .exchange_control(
                    Kind::OverlapHit,
                    binding,
                    node.0,
                    [f64::from(reference), if last { 1.0 } else { 0.0 }, 0.0],
                    request,
                )?;
            self.bindings
                .get_mut(&binding)
                .context("Unknown component binding")?
                .lifecycle
                .enabled = reply.enabled;
            if let Some(command) = reply.command {
                self.apply(node, command)?;
            }
        }
        Ok(())
    }

    /// Delivers completed raycasts to their requesting components.
    fn flush_raycast_events(&mut self) -> Result<()> {
        let events = std::mem::take(&mut self.pending_raycast_events);
        for (binding, request, distance, normal, missed) in events {
            let Some(node) = self.bindings.get(&binding).map(|entry| entry.node) else {
                continue;
            };
            // The node slot carries the remaining normal components as f32 bits;
            // results are not associated with a scene node.
            let packed_node =
                u64::from(normal[1].to_bits()) | (u64::from(normal[2].to_bits()) << 32);
            let reply = self
                .script
                .as_mut()
                .context("Script disconnected")?
                .exchange_control(
                    Kind::RaycastResult,
                    binding,
                    packed_node,
                    [
                        f64::from(distance),
                        if missed { 1.0 } else { 0.0 },
                        f64::from(normal[0]),
                    ],
                    request,
                )?;
            self.bindings
                .get_mut(&binding)
                .context("Unknown component binding")?
                .lifecycle
                .enabled = reply.enabled;
            if let Some(command) = reply.command {
                self.apply(node, command)?;
            }
        }
        Ok(())
    }

    /// Delivers completed on-demand imports to their requesting components.
    fn flush_asset_events(&mut self) -> Result<()> {
        let events = std::mem::take(&mut self.pending_asset_events);
        for (binding, request, handle, failed, kind) in events {
            let Some(node) = self.bindings.get(&binding).map(|entry| entry.node) else {
                continue;
            };
            self.dispatch_raw_z(
                binding,
                node,
                Kind::AssetLoaded,
                request,
                Some([handle as f64, if failed { 1.0 } else { 0.0 }]),
                Some(f64::from(kind)),
            )?;
        }
        Ok(())
    }

    /// Delivers one pointer phase to the node captured for the pointer identity.
    ///
    /// A press runs hit testing and captures the topmost visible sprite. Movement,
    /// release and cancellation reuse that capture even when the pointer leaves the
    /// sprite, matching touch tracking in graphics APIs. Cancellation without a
    /// capture is a no-op.
    fn dispatch_pointer(
        &mut self,
        kind: Kind,
        id: u64,
        position: [f32; 2],
        viewport: [u32; 2],
    ) -> Result<()> {
        if self.script.is_none() {
            return Ok(());
        }
        let (reference, position) = match self.pointers.get(&id).copied() {
            Some((reference, _)) => (reference, position),
            None if kind == Kind::PointerTouch => {
                if viewport.contains(&0) {
                    return Ok(());
                }
                let Some(reference) = self.pointer_target(position, viewport)? else {
                    return Ok(());
                };
                self.pointers.insert(id, (reference, position));
                (reference, position)
            }
            None => return Ok(()),
        };
        if let Some(entry) = self.pointers.get_mut(&id) {
            entry.1 = position;
        }
        let Some((binding, owner)) = self.binding_for_reference(reference)? else {
            return Ok(());
        };
        // Cancellation has no meaningful position, so it carries no local point.
        let local = if kind == Kind::PointerNodeCancel {
            [0.0; 2]
        } else {
            let world = self.pointer_world(position, viewport);
            self.pointer_local(reference, world)?
        };
        let detail = u64::from(reference) | (id << 32);
        self.dispatch_raw(binding, owner, kind, detail, Some(local.map(f64::from)))?;
        self.settle_mutations()
    }

    /// Maps normalized frame coordinates into the active camera's world space.
    fn pointer_world(&self, position: [f32; 2], viewport: [u32; 2]) -> [f32; 2] {
        let camera = self.director.display().camera();
        let scale = camera.scale(viewport);
        [
            camera.center[0] + (position[0] - 0.5) * viewport[0] as f32 / scale[0],
            camera.center[1] + (0.5 - position[1]) * viewport[1] as f32 / scale[1],
        ]
    }

    /// Maps a world point into a node's local space.
    fn pointer_local(&self, reference: u32, world: [f32; 2]) -> Result<[f32; 2]> {
        let id = self.director.script_node(reference)?;
        Ok(self
            .director
            .scene()
            .snapshot()?
            .iter()
            .find(|node| node.id == id)
            .and_then(|node| node.world.inverse_point(world))
            .unwrap_or([0.0; 2]))
    }

    /// Returns the script reference of the topmost visible sprite under the pointer.
    fn pointer_target(&self, position: [f32; 2], viewport: [u32; 2]) -> Result<Option<u32>> {
        let point = self.pointer_world(position, viewport);
        let mut nodes = self.director.scene().snapshot()?;
        nodes.sort_by_key(|node| node.appearance.map_or(0, |appearance| appearance.layer));
        let target = nodes.iter().rev().find(|node| {
            let Some(sprite) = node.sprite else {
                return false;
            };
            if !node.visible {
                return false;
            }
            let corners = node
                .appearance
                .unwrap_or_default()
                .corners(sprite.size)
                .map(|corner| node.world.point(corner));
            contains_pointer(corners, point)
        });
        Ok(target.and_then(|node| self.director.script_reference(node.id)))
    }

    fn dispatch_contacts(&mut self) -> Result<()> {
        if self.script.is_none()
            || (self.gameplay.is_none()
                && self.director.physics_contacts().is_empty()
                && self.director.physics3d_contacts().is_empty())
        {
            return Ok(());
        }
        let targets: Vec<_> = self
            .bindings
            .iter()
            .map(|(&binding, entry)| (binding, entry.node))
            .collect();
        let states = self.director.scene().snapshot()?;
        let mut events = BTreeMap::new();
        for &(_, target) in &targets {
            if let Entry::Vacant(entry) = events.entry(target) {
                let contacts = if let Some(game) = &mut self.gameplay {
                    game.contacts(&states, target, self.director.physics_overlaps(target))
                        .into_iter()
                        .map(|(kind, other)| (kind, other, false))
                        .collect()
                } else {
                    let mut events: Vec<(Kind, EntityId, bool)> = self
                        .director
                        .physics_contacts()
                        .iter()
                        .filter_map(|contact| {
                            let [a, b] = contact.nodes;
                            let other = if a == target {
                                b
                            } else if b == target {
                                a
                            } else {
                                return None;
                            };
                            Some((
                                if contact.entered {
                                    Kind::CollisionEnter
                                } else {
                                    Kind::CollisionExit
                                },
                                other,
                                false,
                            ))
                        })
                        .collect();
                    events.extend(self.director.physics3d_contacts().iter().filter_map(
                        |contact| {
                            let [a, b] = contact.nodes;
                            let other = if a == target {
                                b
                            } else if b == target {
                                a
                            } else {
                                return None;
                            };
                            Some((
                                if contact.entered {
                                    Kind::CollisionEnter3D
                                } else {
                                    Kind::CollisionExit3D
                                },
                                other,
                                contact.trigger,
                            ))
                        },
                    ));
                    events
                };
                entry.insert(contacts);
            }
        }
        for (binding, target) in targets {
            for &(kind, other, trigger) in &events[&target] {
                self.sync_lifecycle()?;
                if let Some(entry) = self.bindings.get(&binding)
                    && entry
                        .lifecycle
                        .runs(self.director.scene().active_in_hierarchy(target)?)
                {
                    self.dispatch_raw_z(
                        binding,
                        target,
                        kind,
                        other.value(),
                        None,
                        Some(if trigger { 1.0 } else { 0.0 }),
                    )?;
                    self.settle_mutations()?;
                }
            }
        }
        Ok(())
    }
}

// Convex quad containment preserves the renderer's transform, anchor, and skew.
fn contains_pointer(corners: [[f32; 2]; 4], point: [f32; 2]) -> bool {
    let [a, b, c, d] = corners;
    let cross = |from: [f32; 2], to: [f32; 2], p: [f32; 2]| {
        (to[0] - from[0]) * (p[1] - from[1]) - (to[1] - from[1]) * (p[0] - from[0])
    };
    if cross(a, b, c) == 0.0 {
        return false;
    }
    let sides = [
        cross(a, b, point),
        cross(b, d, point),
        cross(d, c, point),
        cross(c, a, point),
    ];
    sides.iter().all(|&side| side >= 0.0) || sides.iter().all(|&side| side <= 0.0)
}
