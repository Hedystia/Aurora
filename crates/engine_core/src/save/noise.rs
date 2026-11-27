//! Deterministic procedural noise used by 2D visuals and 3D materials.
//!
//! Noise is generated from integer hashes, so the same document always
//! produces the same pixels without storing a random seed table. A baked PNG
//! can override generation once the editor exports it into the project.

use std::path::PathBuf;

use anyhow::{Result, ensure};
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

/// Fractal value-noise texture parameters.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Noise {
    /// Output dimensions in pixels.
    pub size: [u32; 2],
    /// Noise cells across 100 pixels.
    pub frequency: f32,
    /// Number of fractal octaves.
    pub octaves: u8,
    /// Deterministic seed.
    pub seed: u32,
    /// Amplitude multiplier per octave.
    pub roughness: f32,
    /// Color at noise zero.
    pub low: [u8; 4],
    /// Color at noise one.
    pub high: [u8; 4],
    /// Project-relative baked PNG, when one has been exported.
    pub baked: Option<PathBuf>,
}

impl Default for Noise {
    fn default() -> Self {
        Self {
            size: [128, 128],
            frequency: 4.0,
            octaves: 4,
            seed: 1,
            roughness: 0.5,
            low: [32, 32, 40, 255],
            high: [220, 220, 235, 255],
            baked: None,
        }
    }
}

impl Noise {
    /// Checks document settings independently of external resources.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.size.iter().all(|value| (1..=4096).contains(value)),
            "Noise dimensions must be 1 to 4096 pixels"
        );
        ensure!(
            self.frequency.is_finite() && (0.01..=100.0).contains(&self.frequency),
            "Noise frequency must be 0.01 to 100"
        );
        ensure!(
            (1..=8).contains(&self.octaves),
            "Noise octaves must be 1 to 8"
        );
        ensure!(
            self.roughness.is_finite() && (0.01..=1.0).contains(&self.roughness),
            "Noise roughness must be 0.01 to 1"
        );
        Ok(())
    }

    /// Renders the texture in memory.
    pub fn render(&self) -> Result<RgbaImage> {
        self.validate()?;
        let mut image = RgbaImage::new(self.size[0], self.size[1]);
        for y in 0..self.size[1] {
            for x in 0..self.size[0] {
                let sample = self.sample(
                    x as f32 * self.frequency / 100.0,
                    y as f32 * self.frequency / 100.0,
                );
                let t = (sample * 0.5 + 0.5).clamp(0.0, 1.0);
                *image.get_pixel_mut(x, y) = Rgba(std::array::from_fn(|channel| {
                    (self.low[channel] as f32 * (1.0 - t) + self.high[channel] as f32 * t) as u8
                }));
            }
        }
        Ok(image)
    }

    /// Fractal value noise in roughly -1..1.
    fn sample(&self, x: f32, y: f32) -> f32 {
        let mut sum = 0.0_f32;
        let mut amplitude = 1.0_f32;
        let mut frequency = 1.0_f32;
        for _ in 0..self.octaves {
            sum += (value_noise(x * frequency, y * frequency, self.seed) * 2.0 - 1.0) * amplitude;
            frequency *= 2.0;
            amplitude *= self.roughness;
        }
        sum
    }
}

fn hash(x: i32, y: i32, seed: u32) -> u32 {
    let mut value = (x as u32).wrapping_mul(0x9E37_79B9)
        ^ (y as u32).wrapping_mul(0x85EB_CA6B)
        ^ seed.wrapping_mul(0xC2B2_AE3D);
    value ^= value >> 16;
    value = value.wrapping_mul(0x7FEB_352D);
    value ^= value >> 15;
    value
}

fn value_noise(x: f32, y: f32, seed: u32) -> f32 {
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let fx = x - ix as f32;
    let fy = y - iy as f32;
    let sx = fx * fx * (3.0 - 2.0 * fx);
    let sy = fy * fy * (3.0 - 2.0 * fy);
    let corner = |dx, dy| hash(ix + dx, iy + dy, seed) as f32 / u32::MAX as f32;
    let top = corner(0, 0) * (1.0 - sx) + corner(1, 0) * sx;
    let bottom = corner(0, 1) * (1.0 - sx) + corner(1, 1) * sx;
    top * (1.0 - sy) + bottom * sy
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_deterministic_and_bounded() -> Result<()> {
        let noise = Noise {
            size: [16, 16],
            ..Noise::default()
        };
        let first = noise.render()?;
        let second = noise.render()?;
        assert_eq!(first.as_raw(), second.as_raw());
        assert!(
            first
                .as_raw()
                .chunks(4)
                .map(|pixel| pixel[0])
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                > 1
        );
        Ok(())
    }

    #[test]
    fn invalid_noise_parameters_are_rejected() {
        let noise = Noise {
            octaves: 0,
            ..Noise::default()
        };
        assert!(noise.validate().is_err());
        let noise = Noise {
            frequency: 0.0,
            ..Noise::default()
        };
        assert!(noise.validate().is_err());
    }
}
