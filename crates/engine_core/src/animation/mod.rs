//! Authored keyframe clips sampled on the runtime's 60 Hz clock.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::{ecs::EntityId, scene::Scene};

/// Handling of the end of an animation.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Wrap {
    /// Stop on the final pose.
    Once,
    /// Restart forward playback.
    Loop,
    /// Alternate forward and backward playback.
    PingPong,
}

/// Interpolation between successive keys.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpolation {
    /// Hold the earlier key.
    Step,
    /// Blend linearly.
    Linear,
    /// Cubic ease with zero tangents at both keys.
    Cubic,
}

/// Animated local transform channel.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Property {
    /// Horizontal local position.
    X,
    /// Vertical local position.
    Y,
    /// Counterclockwise local radians.
    Rotation,
    /// Horizontal local scale.
    ScaleX,
    /// Vertical local scale.
    ScaleY,
}

/// Scalar keyframe at an authored sample.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Key {
    /// Sample index.
    pub frame: u32,
    /// Channel value.
    pub value: f32,
}

/// A single channel's keys for one scene node.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Track {
    /// Index of the node in the scene document.
    pub node: usize,
    /// Channel to animate.
    pub property: Property,
    /// Curve between keys.
    pub interpolation: Interpolation,
    /// Keys in strictly increasing sample order.
    pub keys: Vec<Key>,
}

impl Track {
    fn sample(&self, frame: f64) -> f32 {
        let index = self
            .keys
            .partition_point(|key| f64::from(key.frame) <= frame);
        if index == 0 {
            return self.keys[0].value;
        }
        let current = &self.keys[index - 1];
        let Some(next) = self.keys.get(index) else {
            return current.value;
        };
        let t = (frame - f64::from(current.frame)) / f64::from(next.frame - current.frame);
        let blend = match self.interpolation {
            Interpolation::Step => 0.0,
            Interpolation::Linear => t,
            Interpolation::Cubic => t * t * (3.0 - 2.0 * t),
        };
        (f64::from(current.value) * (1.0 - blend) + f64::from(next.value) * blend) as f32
    }
}

/// Clip marker emitted when playback crosses the authored sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Marker {
    /// Sample index. Markers may use either endpoint.
    pub frame: u32,
    /// User-defined event code.
    pub code: u32,
}

/// Version-independent clip data embedded in an Aurora scene document.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Clip {
    /// Name for display in the editor.
    pub name: String,
    /// Authored samples per second.
    pub sample_rate: u32,
    /// Length in authored samples.
    pub duration: u32,
    /// Positive playback multiplier.
    pub speed: f64,
    /// End-of-clip behavior.
    pub wrap: Wrap,
    /// Bound scalar property curves.
    pub tracks: Vec<Track>,
    /// Ordered markers.
    #[serde(default)]
    pub events: Vec<Marker>,
}

impl Clip {
    /// Checks track bindings, timing, and unique channels.
    pub fn validate(&self, nodes: usize) -> Result<()> {
        ensure!(
            (1..=240).contains(&self.sample_rate) && self.duration > 0,
            "Invalid clip dimensions"
        );
        ensure!(self.speed > 0.0 && self.speed <= 16.0, "Invalid clip speed");
        let mut channels = std::collections::BTreeSet::new();
        for track in &self.tracks {
            ensure!(
                track.node < nodes && channels.insert((track.node, track.property)),
                "Invalid or duplicate animation channel"
            );
            ensure!(
                !track.keys.is_empty()
                    && track
                        .keys
                        .iter()
                        .all(|key| key.frame <= self.duration && key.value.is_finite()),
                "Invalid animation keys"
            );
            ensure!(
                track
                    .keys
                    .windows(2)
                    .all(|keys| keys[0].frame < keys[1].frame),
                "Animation keys are out of order"
            );
        }
        ensure!(
            self.events.iter().all(|event| event.frame <= self.duration)
                && self
                    .events
                    .windows(2)
                    .all(|events| events[0].frame <= events[1].frame),
            "Invalid animation markers"
        );
        Ok(())
    }
}

/// Playback of a validated clip against a specific scene instance.
pub struct Playback {
    clip: Clip,
    nodes: Vec<EntityId>,
    ticks: u64,
    finished: bool,
}

impl Playback {
    /// Binds a clip to the scene's authored node order.
    pub fn new(clip: Clip, nodes: Vec<EntityId>) -> Result<Self> {
        clip.validate(nodes.len())?;
        Ok(Self {
            clip,
            nodes,
            ticks: 0,
            finished: false,
        })
    }

    /// Applies frame zero and returns its event codes.
    pub fn start(&mut self, scene: &mut Scene) -> Result<Vec<u32>> {
        self.apply(scene, 0.0)?;
        self.ticks = 0;
        self.finished = false;
        Ok(self
            .clip
            .events
            .iter()
            .filter(|marker| marker.frame == 0)
            .map(|marker| marker.code)
            .collect())
    }

    /// Advances by one simulation tick and returns crossed events in playback order.
    pub fn step(&mut self, scene: &mut Scene) -> Result<Vec<u32>> {
        if self.finished {
            return Ok(Vec::new());
        }
        let rate = f64::from(self.clip.sample_rate) * self.clip.speed / 60.0;
        let previous = self.ticks as f64 * rate;
        self.ticks = self
            .ticks
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Animation clock exhausted"))?;
        let current = self.ticks as f64 * rate;
        let duration = f64::from(self.clip.duration);
        let frame = match self.clip.wrap {
            Wrap::Once => current.min(duration),
            Wrap::Loop => current.rem_euclid(duration),
            Wrap::PingPong => duration - (current.rem_euclid(2.0 * duration) - duration).abs(),
        };
        self.apply(scene, frame)?;
        self.finished = matches!(self.clip.wrap, Wrap::Once) && current >= duration;

        let mut crossed = Vec::new();
        let mut collect = |time: f64, code| {
            if time > previous && time <= current {
                crossed.push((time, code));
            }
        };
        match self.clip.wrap {
            Wrap::Once => {
                for marker in &self.clip.events {
                    collect(f64::from(marker.frame), marker.code);
                }
            }
            wrap => {
                let period = if matches!(wrap, Wrap::Loop) {
                    duration
                } else {
                    2.0 * duration
                };
                for cycle in (previous / period).floor() as u64..=(current / period).floor() as u64
                {
                    let base = cycle as f64 * period;
                    for marker in &self.clip.events {
                        let offset = f64::from(marker.frame);
                        collect(base + offset, marker.code);
                        if matches!(wrap, Wrap::PingPong) && offset > 0.0 && offset < duration {
                            collect(base + period - offset, marker.code);
                        }
                    }
                }
            }
        }
        crossed.sort_by(|a, b| a.0.total_cmp(&b.0));
        Ok(crossed.into_iter().map(|(_, code)| code).collect())
    }

    fn apply(&self, scene: &mut Scene, frame: f64) -> Result<()> {
        for track in &self.clip.tracks {
            let node = self.nodes[track.node];
            let mut transform = scene.transform(node)?;
            let value = track.sample(frame);
            match track.property {
                Property::X => transform.position[0] = value,
                Property::Y => transform.position[1] = value,
                Property::Rotation => transform.rotation = value,
                Property::ScaleX => transform.scale[0] = value,
                Property::ScaleY => transform.scale[1] = value,
            }
            scene.set_transform(node, transform)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_modes_sample_correctly_and_emit_crossed_markers() -> Result<()> {
        for (wrap, expected) in [
            (Wrap::Once, vec![1.0, 2.0, 2.0, 2.0]),
            (Wrap::Loop, vec![1.0, 0.0, 1.0, 0.0]),
            (Wrap::PingPong, vec![1.0, 2.0, 1.0, 0.0]),
        ] {
            let mut scene = Scene::default();
            let (node, _) = scene.spawn("Sprite", None, Default::default())?;
            let clip = Clip {
                name: "Test".into(),
                sample_rate: 60,
                duration: 2,
                speed: 1.0,
                wrap,
                tracks: vec![Track {
                    node: 0,
                    property: Property::X,
                    interpolation: Interpolation::Cubic,
                    keys: vec![
                        Key {
                            frame: 0,
                            value: 0.0,
                        },
                        Key {
                            frame: 2,
                            value: 2.0,
                        },
                    ],
                }],
                events: vec![Marker { frame: 1, code: 42 }],
            };
            let mut playback = Playback::new(clip, vec![node])?;
            assert!(playback.start(&mut scene)?.is_empty());
            let mut events = Vec::new();
            for position in expected {
                events.extend(playback.step(&mut scene)?);
                assert_eq!(scene.transform(node)?.position[0], position);
            }
            assert_eq!(
                events,
                if matches!(wrap, Wrap::Once) {
                    vec![42]
                } else {
                    vec![42, 42]
                }
            );
        }
        Ok(())
    }
}
