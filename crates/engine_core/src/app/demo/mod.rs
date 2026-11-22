//! Small runtime scene exercising imported PNGs and nested transforms.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::{
    assets::Assets,
    ecs::EntityId,
    scene::{Camera2D, Lifecycle, Scene, Sprite, Transform2D},
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
    pub(super) ticks: u64,
}

impl Demo {
    pub(super) fn new(sprite: Option<&Path>, cache: Option<PathBuf>) -> Result<Self> {
        let mut assets = Assets::new(cache);
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
            ticks: 0,
        })
    }

    pub(super) fn tick(&mut self) -> Result<()> {
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
}
