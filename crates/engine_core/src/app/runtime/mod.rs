//! Shared player entry point for built-in demos and authored scenes.

mod gameplay;
mod spatial;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use script_api::Kind;
use winit::{
    event::{ElementState, WindowEvent},
    keyboard::{KeyCode, PhysicalKey},
};

use crate::{
    director::Director,
    input::InputEvent,
    physics3d::Backend,
    render::{Cube, Frame},
    scene::NodeState,
    scripting::{Command, Script, Session},
};

use super::{
    Scenario,
    demo::{CAMERA, Demo},
};

pub(super) enum Runtime {
    Demo(Box<Demo>),
    Authored(Box<Authored>),
    Spatial(Box<spatial::Spatial>),
}

impl Runtime {
    pub(super) fn new(
        sprite: Option<&Path>,
        cache: Option<PathBuf>,
        scenario: &Scenario<'_>,
        silent: bool,
    ) -> Result<Self> {
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
                profile: scenario.profile.map(Path::to_path_buf),
                silent,
                script: None,
                pending_scene: None,
                script_node: scenario.script_node,
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
                game.script = script.map(Session::spawn).transpose()?;
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
            Self::Authored(game) => Frame::Sprites {
                assets: game.director.assets(),
                nodes: game.director.scene().snapshot()?,
                camera: CAMERA,
            },
            Self::Spatial(game) => Frame::Cubes(&game.cubes),
        })
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

pub(super) struct Authored {
    director: Director,
    gameplay: Option<gameplay::Gameplay>,
    profile: Option<PathBuf>,
    silent: bool,
    script: Option<Session>,
    pending_scene: Option<usize>,
    script_node: usize,
}

impl Authored {
    fn start(&mut self) -> Result<()> {
        if let Some(gameplay) = &self.gameplay {
            self.dispatch(Kind::RestoreScore, u64::from(gameplay.best))?;
        }
        self.dispatch(Kind::Start, 0)
    }

    fn dispatch(&mut self, mut kind: Kind, mut detail: u64) -> Result<()> {
        if self.script.is_none() {
            return Ok(());
        }
        let node = self
            .director
            .node(self.script_node)
            .context("The script needs a bound scene node")?;
        for _ in 0..=128 {
            let transform = self.director.scene().transform(node)?;
            let command = self
                .script
                .as_mut()
                .context("Script disconnected")?
                .exchange(kind, node.0, transform.position, detail)?;
            let Some(command) = command else {
                return Ok(());
            };
            if !self.apply(node, command)? {
                return Ok(());
            }
            kind = Kind::FlushCommands;
            detail = 0;
        }
        anyhow::bail!("Script scene command limit exceeded")
    }

    fn target(&self, index: usize) -> Result<crate::ecs::EntityId> {
        self.director.node(index).context("Unknown authored node")
    }

    fn require_gameplay(&mut self) -> Result<&mut gameplay::Gameplay> {
        self.gameplay
            .as_mut()
            .context("Scene has no gameplay resources")
    }

    fn apply(&mut self, node: crate::ecs::EntityId, command: Command) -> Result<bool> {
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
            Command::PlayOneShot(index) => self.require_gameplay()?.sound(index)?,
            Command::SetLinearVelocity(_) => {
                anyhow::bail!("The bound authored node has no physics body")
            }
            Command::MoveNode(index, position) => {
                self.move_node(self.target(index)?, position)?;
                return Ok(true);
            }
            Command::ShowNode(index, visible) => {
                let target = self.target(index)?;
                self.director.scene_mut().set_visible(target, visible)?;
                return Ok(true);
            }
            Command::SelectFrame(index, frame) => {
                let target = self.target(index)?;
                self.gameplay
                    .as_ref()
                    .context("Scene has no gameplay resources")?
                    .select_frame(&mut self.director, target, frame)?;
                return Ok(true);
            }
            Command::QueueSound(index) => {
                self.require_gameplay()?.sound(index)?;
                return Ok(true);
            }
            Command::RecordScore(score) => {
                self.require_gameplay()?.record(score)?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn move_node(&mut self, node: crate::ecs::EntityId, position: [f32; 2]) -> Result<()> {
        let mut transform = self.director.scene().transform(node)?;
        transform.position = position;
        self.director.scene_mut().set_transform(node, transform)
    }

    fn tick(&mut self, input: &[InputEvent]) -> Result<()> {
        if let Some(index) = self.pending_scene.take() {
            self.director.change_scene(index)?;
            self.gameplay =
                gameplay::Gameplay::load(&mut self.director, self.silent, self.profile.as_deref())?;
            self.start()?;
        }
        for event in input {
            match event {
                InputEvent::KeyDown(key) => self.dispatch(Kind::KeyDown, *key as u64)?,
                InputEvent::TouchStart => self.dispatch(Kind::TouchStart, 0)?,
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
        if let Some(gameplay) = &mut self.gameplay {
            gameplay.step_audio();
            if let Some(target) = self.director.node(self.script_node) {
                for (kind, other) in gameplay.contacts(&self.director, target)? {
                    self.dispatch(kind, other.value())?;
                }
            }
        }
        Ok(())
    }
}
