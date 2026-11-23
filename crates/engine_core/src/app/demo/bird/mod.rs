//! M3 sample linking physics bodies, rendering and sampled audio.

use anyhow::{Context, Result};
use rapier2d::prelude::{ColliderBuilder, RigidBodyBuilder, Vector};
use serde::Serialize;

use crate::{
    assets::{AssetId, Assets},
    audio::{Audio, AudioClip, AudioSource},
    ecs::EntityId,
    physics2d::{Contact, Physics2D},
    scene::{Scene, Sprite, Transform2D},
};

const PIXELS_PER_METER: f32 = 32.0;
const RADIUS: f32 = 0.47;

#[derive(Default, Serialize)]
pub(super) struct Stats {
    flaps: u64,
    pipe_hits: u64,
    contact_exits: u64,
    one_shots: u64,
}

pub(super) struct Bird {
    physics: Physics2D,
    pub(super) actor: EntityId,
    floor: EntityId,
    pipes: Vec<EntityId>,
    audio: Audio,
    source: AudioSource,
    pub(super) stats: Stats,
}

impl Bird {
    pub(super) fn new(assets: &mut Assets, scene: &mut Scene, silent: bool) -> Result<Self> {
        let bird = assets.import_png(include_bytes!("bird.png"))?;
        let pipe = assets.import_png(include_bytes!("pipe.png"))?;
        let mut physics = Physics2D::default();
        let actor_pose = Vector::new(-2.5, 0.0);
        let actor = sprite(scene, "Bird", bird, actor_pose, [RADIUS * 2.0; 2])?;
        physics.insert(
            actor,
            RigidBodyBuilder::dynamic()
                .translation(actor_pose)
                .enabled_translations(false, true)
                .lock_rotations()
                .ccd_enabled(true),
            ColliderBuilder::ball(RADIUS),
        )?;

        let floor_pose = Vector::new(0.0, -5.55);
        let floor = sprite(scene, "Floor", pipe, floor_pose, [20.0, 0.9])?;
        physics.insert(
            floor,
            RigidBodyBuilder::fixed().translation(floor_pose),
            ColliderBuilder::cuboid(10.0, 0.45),
        )?;

        let mut pipes = Vec::new();
        for x in [2.4, 8.0] {
            for (name, y, half_height) in [("Lower pipe", -2.5, 2.6), ("Upper pipe", 4.45, 2.15)] {
                let pose = Vector::new(x, y);
                let id = sprite(scene, name, pipe, pose, [1.1, half_height * 2.0])?;
                physics.insert(
                    id,
                    RigidBodyBuilder::kinematic_velocity_based()
                        .translation(pose)
                        .linvel(Vector::new(-2.8, 0.0)),
                    ColliderBuilder::cuboid(0.55, half_height),
                )?;
                pipes.push(id);
            }
        }
        let source = AudioSource::new(vec![
            AudioClip::decode(include_bytes!("flap.wav").to_vec())?,
            AudioClip::decode(include_bytes!("hit.wav").to_vec())?,
        ]);
        Ok(Self {
            physics,
            actor,
            floor,
            pipes,
            audio: Audio::new(silent)?,
            source,
            stats: Stats::default(),
        })
    }

    pub(super) fn flap(&mut self) -> Result<()> {
        self.set_velocity([0.0, 5.8])
    }

    pub(super) fn set_velocity(&mut self, velocity: [f32; 2]) -> Result<()> {
        let body = self
            .physics
            .body_mut(self.actor)
            .context("Missing bird body")?;
        body.set_linvel(Vector::from_array(velocity), true);
        if velocity[1] > 0.0 {
            self.stats.flaps += 1;
        }
        Ok(())
    }

    pub(super) fn set_position(&mut self, position: [f32; 2]) -> Result<()> {
        let body = self
            .physics
            .body_mut(self.actor)
            .context("Missing bird body")?;
        body.set_translation(Vector::from_array(position) / PIXELS_PER_METER, true);
        Ok(())
    }

    pub(super) fn play(&mut self, index: usize) -> Result<()> {
        self.source.play_one_shot(&mut self.audio, index)?;
        self.stats.one_shots += 1;
        Ok(())
    }

    pub(super) fn step(&mut self, scene: &mut Scene) -> Result<([f32; 2], Vec<Contact>)> {
        let events = self.physics.step();
        for contact in &events {
            if contact.nodes.contains(&self.actor) {
                if contact.entered && !contact.nodes.contains(&self.floor) {
                    self.stats.pipe_hits += 1;
                } else if !contact.entered {
                    self.stats.contact_exits += 1;
                }
            }
        }
        let position = self.sync(scene, self.actor)?;
        for &id in &self.pipes {
            self.sync(scene, id)?;
        }
        Ok((position, events))
    }

    pub(super) fn step_audio(&mut self) {
        self.audio.step();
    }

    fn sync(&self, scene: &mut Scene, id: EntityId) -> Result<[f32; 2]> {
        let body = self
            .physics
            .body(id)
            .context("Missing scene physics body")?;
        let position = (body.translation() * PIXELS_PER_METER).to_array();
        scene.set_transform(
            id,
            Transform2D {
                position,
                ..Default::default()
            },
        )?;
        Ok(position)
    }
}

fn sprite(
    scene: &mut Scene,
    name: &str,
    texture: AssetId,
    pose: Vector,
    size: [f32; 2],
) -> Result<EntityId> {
    let (node, _) = scene.spawn(
        name,
        None,
        Transform2D {
            position: (pose * PIXELS_PER_METER).to_array(),
            ..Default::default()
        },
    )?;
    scene.set_sprite(
        node,
        Sprite {
            emitter: None,
            texture,
            size: size.map(|side| side * PIXELS_PER_METER),
        },
    )?;
    Ok(node)
}
