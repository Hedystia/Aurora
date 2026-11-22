//! Small runtime scene exercising imported PNGs and nested transforms.

mod bird;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use script_api::Kind;

use crate::{
    assets::Assets,
    ecs::EntityId,
    input::InputEvent,
    scene::{Camera2D, Lifecycle, Scene, Sprite, Transform2D},
    scripting::{Command, Script, Session},
};

pub(super) const CAMERA: Camera2D = Camera2D {
    center: [0.0, 0.0],
    design_size: [640.0, 384.0],
};
const DEFAULT_PNG: &[u8] = include_bytes!("aurora.png");

pub(super) struct Demo {
    pub(super) assets: Assets,
    pub(super) scene: Scene,
    pivot: EntityId,
    bird: Option<bird::Bird>,
    primary: EntityId,
    script: Option<Session>,
    script_position: [f32; 2],
    pub(super) ticks: u64,
}

impl Demo {
    pub(super) fn new(
        sprite: Option<&Path>,
        cache: Option<PathBuf>,
        bird_mode: bool,
        silent: bool,
    ) -> Result<Self> {
        let mut assets = Assets::new(cache);
        if bird_mode {
            let mut scene = Scene::default();
            let bird = bird::Bird::new(&mut assets, &mut scene, silent)?;
            let primary = bird.actor;
            scene.activate()?;
            return Ok(Self {
                assets,
                scene,
                pivot: primary,
                bird: Some(bird),
                primary,
                script: None,
                script_position: [-80.0, 0.0],
                ticks: 0,
            });
        }
        let texture = match sprite {
            Some(path) => assets.import_file(path)?,
            None => assets.import_png(DEFAULT_PNG)?,
        };
        let image = assets
            .texture(texture)
            .context("Imported texture is missing")?;
        let extent = image.width().max(image.height()) as f32;
        let size = [
            image.width() as f32 * 160.0 / extent,
            image.height() as f32 * 160.0 / extent,
        ];
        let mut scene = Scene::default();
        let (pivot, _) = scene.spawn("Pivot", None, Transform2D::default())?;
        let (primary, _) = scene.spawn("Aurora", Some(pivot), Transform2D::default())?;
        scene.set_sprite(primary, Sprite { texture, size })?;
        for (name, x, angle) in [("Left", -180.0, 0.25), ("Right", 180.0, -0.25)] {
            let (id, _) = scene.spawn(
                name,
                Some(pivot),
                Transform2D {
                    position: [x, 0.0],
                    rotation: angle,
                    scale: [0.5; 2],
                },
            )?;
            scene.set_sprite(id, Sprite { texture, size })?;
        }
        scene.activate()?;
        Ok(Self {
            assets,
            scene,
            pivot,
            bird: None,
            primary,
            script: None,
            script_position: [0.0; 2],
            ticks: 0,
        })
    }

    pub(super) fn is_bird(&self) -> bool {
        self.bird.is_some()
    }

    pub(super) fn bird_stats(&self) -> Result<Option<serde_json::Value>> {
        Ok(self
            .bird
            .as_ref()
            .map(|bird| serde_json::to_value(&bird.stats))
            .transpose()?)
    }

    pub(super) fn attach_script(&mut self, script: Option<&Script>) -> Result<()> {
        self.script = script.map(Session::spawn).transpose()?;
        self.dispatch(Kind::Start, 0)
    }

    pub(super) fn disconnect_script(&mut self) {
        self.script = None;
    }

    fn dispatch(&mut self, kind: Kind, detail: u64) -> Result<()> {
        let command = match &mut self.script {
            Some(script) => script.exchange(kind, self.primary.0, self.script_position, detail)?,
            None => return Ok(()),
        };
        match command {
            Some(Command::SetPosition(position)) => {
                if let Some(bird) = &mut self.bird {
                    bird.set_position(position)?;
                }
                self.scene.set_transform(
                    self.primary,
                    Transform2D {
                        position,
                        ..Default::default()
                    },
                )?;
                self.script_position = position;
            }
            Some(Command::SetLinearVelocity(velocity)) => self
                .bird
                .as_mut()
                .context("No bound body")?
                .set_velocity(velocity)?,
            Some(Command::PlayOneShot(index)) => self
                .bird
                .as_mut()
                .context("No bound audio source")?
                .play(index)?,
            Some(Command::Pause | Command::Resume | Command::ChangeScene(_)) => {
                anyhow::bail!("Director commands require an authored scene");
            }
            None => {}
        }
        Ok(())
    }

    pub(super) fn tick(&mut self, input: &[InputEvent]) -> Result<()> {
        if self.bird.is_some() {
            self.tick_bird(input)?;
            self.ticks += 1;
            return Ok(());
        }
        self.dispatch(Kind::FixedUpdate, 0)?;
        self.dispatch(Kind::Update, 0)?;
        self.ticks += 1;
        for event in self.scene.process()? {
            if event == Lifecycle::Process(self.pivot) {
                let rotation = (self.ticks % 720) as f32 * std::f32::consts::TAU / 720.0;
                self.scene.set_transform(
                    self.pivot,
                    Transform2D {
                        rotation,
                        ..Transform2D::default()
                    },
                )?;
            }
        }
        Ok(())
    }

    fn tick_bird(&mut self, input: &[InputEvent]) -> Result<()> {
        for event in input {
            if let Some(bird) = self.bird.as_mut().filter(|_| self.script.is_none()) {
                bird.flap()?;
                bird.play(0)?;
            } else {
                match event {
                    InputEvent::KeyDown(key) => self.dispatch(Kind::KeyDown, *key as u64)?,
                    InputEvent::TouchStart => self.dispatch(Kind::TouchStart, 0)?,
                }
            }
        }
        self.dispatch(Kind::FixedUpdate, 0)?;
        let bird = self.bird.as_mut().context("Bird scene is unavailable")?;
        let (position, contacts) = bird.step(&mut self.scene)?;
        self.script_position = position;
        for event in contacts {
            if let Some(other) = event.nodes.iter().find(|id| **id != self.primary)
                && event.nodes.contains(&self.primary)
            {
                if self.script.is_none() && event.entered {
                    self.bird
                        .as_mut()
                        .context("Bird scene is unavailable")?
                        .play(1)?;
                }
                let kind = if event.entered {
                    Kind::CollisionEnter
                } else {
                    Kind::CollisionExit
                };
                self.dispatch(kind, other.0)?;
            }
        }
        self.dispatch(Kind::Update, 0)?;
        self.bird
            .as_mut()
            .context("Bird scene is unavailable")?
            .step_audio();
        Ok(())
    }
}
