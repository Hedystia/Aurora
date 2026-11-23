//! Editable 2D visuals resolved into the engine's ordered sprite pass.

use std::path::{Path, PathBuf};

use ab_glyph::{Font, FontArc, ScaleFont, point};
use anyhow::{Context, Result, ensure};
use image::{Rgba, RgbaImage, imageops};
use serde::{Deserialize, Serialize};

use crate::{
    assets::{Assets, read_bounded},
    scene::{NodeState, Sprite, SpriteAppearance},
};

/// An independently styled run of text.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Span {
    /// Unicode text, including line breaks.
    pub text: String,
    /// Font height in local pixels.
    pub size: u32,
    /// Straight-alpha sRGB color.
    pub color: [u8; 4],
}

/// Continuous local-space particle emitter sampled on the fixed simulation clock.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Emitter {
    /// Number of staggered particles.
    pub count: u16,
    /// Lifetime per particle in fixed ticks.
    pub lifetime: u16,
    /// Outward speed in pixels per second.
    pub speed: f32,
    /// Downward acceleration in pixels per second squared.
    pub gravity: f32,
    /// Particle diameter in pixels.
    pub diameter: f32,
}

/// Serializable visual component for a scene or prefab node.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Visual {
    /// Multiline text with styled runs and optional project font.
    Text {
        /// TrueType or OpenType font relative to the document.
        font: Option<PathBuf>,
        /// Runs in reading order.
        spans: Vec<Span>,
    },
    /// Rectangular cell selected from a regular sprite sheet.
    Atlas {
        /// Source image, or the built-in starter tile.
        png: Option<PathBuf>,
        /// Cell dimensions in pixels.
        cell: [u32; 2],
        /// Zero-based row-major cell.
        index: u32,
    },
    /// Tile map with row-major cells from a regular sprite sheet.
    TileMap {
        /// Source atlas, or the built-in starter tile.
        png: Option<PathBuf>,
        /// Source and destination tile dimensions.
        cell: [u32; 2],
        /// Number of columns per row.
        columns: u32,
        /// Row-major tile indices; null leaves an empty cell.
        tiles: Vec<Option<u32>>,
    },
    /// Image stretched across nine regions while preserving border widths.
    NineSlice {
        /// Source image, or the built-in starter tile.
        png: Option<PathBuf>,
        /// Left, top, right and bottom border widths in pixels.
        borders: [u32; 4],
        /// Destination dimensions in local pixels.
        size: [u32; 2],
    },
    /// Deterministic radial particles.
    Particles {
        /// Emitter parameters.
        emitter: Emitter,
    },
}

fn dimensions(size: [u32; 2]) -> Result<()> {
    ensure!(
        size.into_iter().all(|n| (1..=4096).contains(&n)),
        "Visual dimensions must be 1 to 4096 pixels"
    );
    Ok(())
}

impl Visual {
    /// Checks document settings independently of external resources.
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Text { spans, .. } => {
                ensure!(
                    !spans.is_empty()
                        && spans.len() <= 256
                        && spans.iter().map(|s| s.text.len()).sum::<usize>() <= 16384,
                    "Text exceeds its run or length limit"
                );
                ensure!(
                    spans.iter().all(|s| (1..=256).contains(&s.size)),
                    "Text sizes must be 1 to 256 pixels"
                );
            }
            Self::Atlas { cell, .. } => dimensions(*cell)?,
            Self::TileMap {
                cell,
                columns,
                tiles,
                ..
            } => {
                dimensions(*cell)?;
                ensure!(
                    !tiles.is_empty() && tiles.len() <= 65536 && (1..=4096).contains(columns),
                    "Invalid tile grid"
                );
                dimensions([
                    columns * cell[0],
                    (tiles.len() as u32).div_ceil(*columns) * cell[1],
                ])?;
            }
            Self::NineSlice { size, borders, .. } => {
                dimensions(*size)?;
                ensure!(
                    borders[0].saturating_add(borders[2]) < size[0]
                        && borders[1].saturating_add(borders[3]) < size[1],
                    "Nine-slice borders must leave a center"
                );
            }
            Self::Particles { emitter: e } => ensure!(
                (1..=256).contains(&e.count)
                    && e.lifetime > 0
                    && [e.speed, e.gravity, e.diameter]
                        .iter()
                        .all(|x| x.is_finite() && x.abs() <= 100000.0)
                    && e.diameter > 0.0,
                "Invalid particle parameters"
            ),
        }
        Ok(())
    }

    /// Mutable document-relative resource reference, when configured.
    pub fn asset_mut(&mut self) -> Option<&mut PathBuf> {
        match self {
            Self::Text { font, .. } => font.as_mut(),
            Self::Atlas { png, .. } | Self::NineSlice { png, .. } | Self::TileMap { png, .. } => {
                png.as_mut()
            }
            Self::Particles { .. } => None,
        }
    }

    pub(super) fn resolve(&self, dir: &Path, assets: &mut Assets) -> Result<Sprite> {
        self.validate()?;
        let image = match self {
            Self::Text { font, spans } => {
                let font = match font {
                    Some(path) => {
                        FontArc::try_from_vec(read_bounded(&dir.join(path), 16 * 1024 * 1024)?)?
                    }
                    None => FontArc::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT)?,
                };
                raster_text(&font, spans)?
            }
            Self::Particles { emitter } => {
                let texture = assets.generated(RgbaImage::from_pixel(1, 1, Rgba([255; 4])));
                return Ok(Sprite {
                    texture,
                    size: [emitter.diameter; 2],
                    emitter: Some(*emitter),
                });
            }
            Self::Atlas { png, .. } | Self::TileMap { png, .. } | Self::NineSlice { png, .. } => {
                let id = if let Some(path) = png {
                    assets.import_file(&dir.join(path))?
                } else {
                    assets.import_png(include_bytes!("../../app/demo/aurora.png"))?
                };
                self.raster_sheet(assets.texture(id).context("Missing atlas texture")?)?
            }
        };
        let size = [image.width() as f32, image.height() as f32];
        Ok(Sprite {
            texture: assets.generated(image),
            size,
            emitter: None,
        })
    }

    fn raster_sheet(&self, src: &RgbaImage) -> Result<RgbaImage> {
        let cell_at = |index: u32, [w, h]: [u32; 2]| -> Result<_> {
            let columns = src.width() / w;
            ensure!(
                index < columns * (src.height() / h),
                "Tile {index} outside atlas"
            );
            Ok(imageops::crop_imm(
                src,
                (index % columns) * w,
                (index / columns) * h,
                w,
                h,
            ))
        };
        match self {
            Self::Atlas { index, cell, .. } => Ok(cell_at(*index, *cell)?.to_image()),
            Self::TileMap {
                tiles,
                columns,
                cell,
                ..
            } => {
                let mut out = RgbaImage::new(
                    columns * cell[0],
                    (tiles.len() as u32).div_ceil(*columns) * cell[1],
                );
                for (i, tile) in tiles.iter().enumerate() {
                    if let Some(tile) = tile {
                        imageops::replace(
                            &mut out,
                            &*cell_at(*tile, *cell)?,
                            i64::from((i as u32 % columns) * cell[0]),
                            i64::from((i as u32 / columns) * cell[1]),
                        );
                    }
                }
                Ok(out)
            }
            Self::NineSlice {
                borders: [l, t, r, b],
                size: [w, h],
                ..
            } => {
                ensure!(
                    l + r < src.width() && t + b < src.height(),
                    "Nine-slice borders exceed source size"
                );
                let map = |p: u32, low: u32, high: u32, src: u32, dst: u32| {
                    if p < low {
                        p
                    } else if p >= dst - high {
                        src - (dst - p)
                    } else {
                        low + (p - low) * (src - low - high) / (dst - low - high)
                    }
                };
                Ok(RgbaImage::from_fn(*w, *h, |x, y| {
                    *src.get_pixel(
                        map(x, *l, *r, src.width(), *w),
                        map(y, *t, *b, src.height(), *h),
                    )
                }))
            }
            _ => unreachable!(),
        }
    }
}

fn raster_text(font: &FontArc, spans: &[Span]) -> Result<RgbaImage> {
    let mut lines = Vec::new();
    let mut line = Vec::new();
    for span in spans {
        for c in span.text.chars() {
            if c == '\n' {
                lines.push(std::mem::take(&mut line));
            } else {
                line.push((c, span));
            }
        }
    }
    lines.push(line);
    let mut glyphs = Vec::new();
    let (mut width, mut height) = (1u32, 0u32);
    let mut left = 0i32;
    for line in lines {
        let line_height = line
            .iter()
            .map(|(_, s)| s.size)
            .max()
            .unwrap_or(spans[0].size);
        let mut x = 0.;
        let mut previous = None;
        let baseline = height as f32 + font.as_scaled(line_height as f32).ascent();
        for (c, span) in line {
            let scaled = font.as_scaled(span.size as f32);
            let id = scaled.glyph_id(c);
            if let Some((prev, size)) = previous
                && size == span.size
            {
                x += scaled.kern(prev, id);
            }
            let mut glyph = scaled.scaled_glyph(c);
            glyph.position = point(x, baseline);
            if let Some(outlined) = font.outline_glyph(glyph) {
                let bounds = outlined.px_bounds();
                left = left.min(bounds.min.x as i32);
                width = width.max(bounds.max.x.ceil() as u32);
                glyphs.push((outlined, span.color));
            }
            x += scaled.h_advance(id);
            previous = Some((id, span.size));
        }
        width = width.max(x.ceil() as u32);
        height += line_height;
        dimensions([width + left.unsigned_abs(), height])?;
    }
    let top = glyphs
        .iter()
        .map(|(g, _)| g.px_bounds().min.y as i32)
        .min()
        .unwrap_or(0)
        .min(0);
    height = height.max(
        glyphs
            .iter()
            .map(|(g, _)| g.px_bounds().max.y as u32)
            .max()
            .unwrap_or(0),
    ) + top.unsigned_abs();
    dimensions([width + left.unsigned_abs(), height])?;
    let mut out = RgbaImage::new(width + left.unsigned_abs(), height);
    for (g, color) in glyphs {
        use image::Pixel;
        let bounds = g.px_bounds();
        g.draw(|x, y, coverage| {
            let mut color = color;
            color[3] = (f32::from(color[3]) * coverage).round() as u8;
            out.get_pixel_mut(
                (bounds.min.x as i32 - left) as u32 + x,
                (bounds.min.y as i32 - top) as u32 + y,
            )
            .blend(&Rgba(color));
        });
    }
    Ok(out)
}

/// Supplies the simulation clock to renderer snapshots and bounds geometry before submission.
pub(crate) fn draw_nodes(mut nodes: Vec<NodeState>, tick: u64) -> Result<Vec<NodeState>> {
    let mut quads = 0;
    for node in &mut nodes {
        node.draw_tick = tick;
        if let Some(sprite) = node.sprite
            && node.visible
        {
            quads += u32::from(sprite.emitter.map_or(1, |e| e.count));
            ensure!(quads <= 1_000_000, "Visual draw limit exceeded");
        }
    }
    Ok(nodes)
}

impl Sprite {
    /// Local quad geometry for this sprite or particle emitter at a fixed tick.
    pub fn quads(
        self,
        appearance: SpriteAppearance,
        tick: u64,
    ) -> impl Iterator<Item = [[f32; 2]; 4]> {
        let corners = appearance.corners(self.size);
        let count = self.emitter.map_or(1, |e| e.count);
        (0..count).map(move |i| {
            let Some(e) = self.emitter else {
                return corners;
            };
            let life = u64::from(e.lifetime).max(1);
            let age = ((tick % life + u64::from(i) * life / u64::from(count)) % life) as f32 / 60.;
            let (sin, cos) = (f32::from(i) * 2.3999631).sin_cos();
            let x = cos * e.speed * age;
            let y = sin * e.speed * age - e.gravity * age * age * 0.5;
            corners.map(|[a, b]| [a + x, b + y])
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save::{PrefabFile, SceneFile};
    use crate::scene::Scene;

    #[test]
    fn atlas_tiles_and_nine_slice_sample_the_declared_cells() -> Result<()> {
        let sheet = RgbaImage::from_fn(3, 3, |x, y| Rgba([x as u8, y as u8, 42, 255]));
        let visual = Visual::TileMap {
            png: None,
            cell: [1, 1],
            columns: 2,
            tiles: vec![Some(8), None, Some(1)],
        };
        visual.validate()?;
        let image = visual.raster_sheet(&sheet)?;
        assert_eq!(image.dimensions(), (2, 2));
        assert_eq!(image.get_pixel(0, 0), sheet.get_pixel(2, 2));
        assert_eq!(image.get_pixel(1, 0).0, [0; 4]);
        assert_eq!(image.get_pixel(0, 1), sheet.get_pixel(1, 0));
        let slice = Visual::NineSlice {
            png: None,
            borders: [1; 4],
            size: [7, 4],
        };
        slice.validate()?;
        let image = slice.raster_sheet(&sheet)?;
        assert_eq!(image.get_pixel(6, 3), sheet.get_pixel(2, 2));
        assert_eq!(image.get_pixel(5, 2), sheet.get_pixel(1, 1));
        assert_eq!(image.get_pixel(0, 1), sheet.get_pixel(0, 1));
        let bad = Visual::Atlas {
            png: None,
            cell: [1; 2],
            index: 9,
        };
        assert!(bad.raster_sheet(&sheet).is_err());
        Ok(())
    }

    #[test]
    fn imported_rich_text_and_emitters_survive_scene_and_prefab_capture() -> Result<()> {
        let root = tempfile::tempdir()?;
        std::fs::write(
            root.path().join("font.ttf"),
            epaint_default_fonts::UBUNTU_LIGHT,
        )?;
        let path = root.path().join("main.scene");
        std::fs::write(
            &path,
            r#"{"version":1,"nodes":[{"name":"Title","visual":{"kind":"text","font":"font.ttf","spans":[{"text":"AB","color":[255,0,0,255],"size":16},{"text":"cd\nEf","size":24,"color":[0,255,0,255]}]}},{"name":"Stars","parent":0,"visual":{"kind":"particles","emitter":{"count":3,"speed":32.0,"gravity":5.0,"diameter":4.0,"lifetime":120}}}]}"#,
        )?;
        let mut assets = Assets::new(None);
        let loaded =
            SceneFile::load(&path)?.instantiate(root.path(), &mut assets, Scene::default())?;
        let id = loaded.ids[0];
        let sprite = loaded
            .scene
            .sprite(id)?
            .context("Missing generated text sprite")?;
        let glyphs = assets
            .texture(sprite.texture)
            .context("Missing rasterized glyphs")?;
        assert!(glyphs.height() >= 48);
        for channel in [0, 1] {
            assert!(glyphs.pixels().any(|p| p.0[channel] > 200 && p.0[3] > 200));
        }
        let capture = SceneFile::capture(&loaded.scene, |_| None)?;
        capture.save(&root.path().join("copy.scene"))?;
        let prefab = PrefabFile::capture(&loaded.scene, id, |_| None)?;
        let resolved = prefab.resolve(root.path(), &mut assets)?;
        assert_eq!(
            resolved.nodes()[0]
                .sprite
                .context("Missing prefab sprite")?
                .texture,
            sprite.texture
        );
        let positions = |tick| -> Result<Vec<_>> {
            let nodes = draw_nodes(loaded.scene.snapshot()?, tick)?;
            let state = &nodes[1];
            Ok(state
                .sprite
                .context("Missing particle emitter")?
                .quads(state.appearance.unwrap_or_default(), state.draw_tick)
                .collect())
        };
        assert_eq!(positions(1)?.len(), 3);
        assert_eq!(positions(1)?, positions(121)?);
        assert_ne!(positions(1)?, positions(2)?);
        Ok(())
    }
}
