//! Shared player entry point for built-in demos and authored scenes.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use script_api::Kind;
use winit::{
    event::{ElementState, WindowEvent},
    keyboard::{KeyCode, PhysicalKey},
};

use crate::{
    assets::Assets,
    director::Director,
    input::InputEvent,
    scene::Scene,
    scripting::{Command, Script, Session},
};

use super::{Scenario, demo::Demo};

pub(super) enum Runtime {
    Demo(Box<Demo>),
    Authored(Box<Authored>),
}

impl Runtime {
    pub(super) fn new(
        sprite: Option<&Path>,
        cache: Option<PathBuf>,
        scenario: &Scenario<'_>,
        silent: bool,
    ) -> Result<Self> {
        if scenario.scenes.is_empty() {
            Ok(Self::Demo(Box::new(Demo::new(
                sprite,
                cache,
                scenario.bird,
                silent,
            )?)))
        } else {
            Ok(Self::Authored(Box::new(Authored {
                director: Director::new(scenario.scenes.to_vec(), cache)?,
                script: None,
                pending_scene: None,
            })))
        }
    }

    pub(super) fn attach_script(&mut self, script: Option<&Script>) -> Result<()> {
        match self {
            Self::Demo(demo) => demo.attach_script(script),
            Self::Authored(game) => {
                game.script = script.map(Session::spawn).transpose()?;
                game.dispatch(Kind::Start, 0)
            }
        }
    }

    pub(super) fn disconnect_script(&mut self) {
        match self {
            Self::Demo(demo) => demo.disconnect_script(),
            Self::Authored(game) => game.script = None,
        }
    }

    pub(super) fn tick(&mut self, input: &[InputEvent]) -> Result<()> {
        match self {
            Self::Demo(demo) => demo.tick(input),
            Self::Authored(game) => game.tick(input),
        }
    }

    pub(super) fn scene(&self) -> &Scene {
        match self {
            Self::Demo(demo) => &demo.scene,
            Self::Authored(game) => game.director.scene(),
        }
    }

    pub(super) fn assets(&self) -> &Assets {
        match self {
            Self::Demo(demo) => &demo.assets,
            Self::Authored(game) => game.director.assets(),
        }
    }

    pub(super) fn ticks(&self) -> u64 {
        match self {
            Self::Demo(demo) => demo.ticks,
            Self::Authored(game) => game.director.ticks(),
        }
    }

    pub(super) fn bird_stats(&self) -> Result<Option<serde_json::Value>> {
        match self {
            Self::Demo(demo) => demo.bird_stats(),
            Self::Authored(_) => Ok(None),
        }
    }

    pub(super) fn title(&self) -> &'static str {
        match self {
            Self::Demo(demo) if demo.is_bird() => "Aurora Bird · Space, ↑ or touch to flap",
            Self::Authored(_) => "Aurora Player · P pause/resume · Tab switch scene",
            Self::Demo(_) => "Aurora Player",
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
    script: Option<Session>,
    pending_scene: Option<usize>,
}

impl Authored {
    fn dispatch(&mut self, kind: Kind, detail: u64) -> Result<()> {
        let Some(script) = &mut self.script else {
            return Ok(());
        };
        let node = self
            .director
            .primary()
            .context("The script needs a bound scene node")?;
        let transform = self.director.scene().transform(node)?;
        match script.exchange(kind, node.0, transform.position, detail)? {
            Some(Command::SetPosition(position)) => self.director.scene_mut().set_transform(
                node,
                crate::scene::Transform2D {
                    position,
                    ..transform
                },
            )?,
            Some(Command::Pause) => self.director.pause(),
            Some(Command::Resume) => self.director.resume(),
            Some(Command::ChangeScene(index)) => {
                anyhow::ensure!(
                    index < self.director.scene_count(),
                    "Unknown scene index {index}"
                );
                self.pending_scene = Some(index);
            }
            Some(Command::SetLinearVelocity(_) | Command::PlayOneShot(_)) => {
                anyhow::bail!("The bound authored node has no physics body or audio source");
            }
            None => {}
        }
        Ok(())
    }

    fn tick(&mut self, input: &[InputEvent]) -> Result<()> {
        if let Some(index) = self.pending_scene.take() {
            self.director.change_scene(index)?;
            self.dispatch(Kind::Start, 0)?;
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
        self.dispatch(Kind::Update, 0)
    }
}
