//! Figma-style design elements resolved into the engine's sprite pass.
//!
//! Geometry uses the scene's design pixels, so a frame has the same units in
//! the editor, the Game view and Play. Auto layout and constraints are applied
//! to the authored node transforms by the editor, keeping runtime rendering
//! unchanged.

use std::path::{Path, PathBuf};

use ab_glyph::FontArc;
use anyhow::{Context, Result, ensure};
use image::{Rgba, RgbaImage, imageops};
use serde::{Deserialize, Serialize};

use crate::{assets::Assets, save::noise::Noise};

/// Shape or container kind in a design document.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignKind {
    /// Artboard that can contain other elements.
    #[default]
    Frame,
    /// Rectangle with optional rounded corners.
    Rectangle,
    /// Ellipse inscribed in the element bounds.
    Ellipse,
    /// Straight line from the element origin to its size.
    Line,
    /// Text label.
    Text,
    /// Bitmap image.
    Image,
    /// Invisible hierarchy parent.
    Group,
}

impl DesignKind {
    /// Whether this kind paints its own pixels.
    pub fn is_renderable(self) -> bool {
        self != Self::Group
    }

    /// Whether this kind can position child elements.
    pub fn is_container(self) -> bool {
        matches!(self, Self::Frame | Self::Group)
    }
}

/// Flat or two-color linear fill.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Fill {
    /// One straight-alpha sRGB color.
    Solid([u8; 4]),
    /// Gradient between two colors, in degrees.
    Linear {
        /// Starting color.
        start: [u8; 4],
        /// Ending color.
        end: [u8; 4],
        /// Gradient angle in degrees.
        angle: f32,
    },
    /// Deterministic procedural noise, tiled over the element bounds.
    Noise(Noise),
}

impl Default for Fill {
    fn default() -> Self {
        Self::Solid([48, 52, 60, 255])
    }
}

/// Border drawn inside the element bounds.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Stroke {
    /// Straight-alpha sRGB color.
    pub color: [u8; 4],
    /// Width in design pixels.
    pub width: f32,
}

/// Soft drop shadow baked behind the element.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Shadow {
    /// Straight-alpha sRGB color.
    pub color: [u8; 4],
    /// Offset from the element in design pixels.
    pub offset: [f32; 2],
    /// Edge falloff in design pixels.
    pub blur: f32,
}

/// Text alignment inside a text element.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    /// Start of the line.
    #[default]
    Start,
    /// Middle of the line.
    Center,
    /// End of the line.
    End,
}

/// Figma-style auto layout applied to direct children by the editor.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct AutoLayout {
    /// Layout direction; `None` leaves children freely positioned.
    pub horizontal: bool,
    /// Whether this container has auto layout enabled.
    pub enabled: bool,
    /// Gap between children in design pixels.
    pub gap: f32,
    /// Left, top, right and bottom padding in design pixels.
    pub padding: [f32; 4],
    /// Child alignment along the cross axis, 0 start, 0.5 center, 1 end.
    pub align: f32,
}

/// Edge constraint used when a parent frame is resized.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Constraint {
    /// Keep the distance to the left or top edge.
    #[default]
    Start,
    /// Keep the distance to the right or bottom edge.
    End,
    /// Keep the element centred in its parent.
    Center,
    /// Stretch between both edges.
    Both,
    /// Scale with the parent.
    Scale,
}

/// Serializable Figma-style design element.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Design {
    /// Shape or container kind.
    #[serde(rename = "element")]
    pub kind: DesignKind,
    /// Width and height in design pixels.
    pub size: [f32; 2],
    /// Fill color or gradient.
    pub fill: Fill,
    /// Optional border.
    pub stroke: Stroke,
    /// Corner radius in design pixels.
    pub corner: f32,
    /// Optional soft drop shadow.
    pub shadow: Option<Shadow>,
    /// Layer blur radius in design pixels.
    pub blur: f32,
    /// Text content for text elements.
    pub text: String,
    /// Font height in design pixels.
    pub font_size: u32,
    /// Text color.
    pub text_color: [u8; 4],
    /// Text alignment.
    pub text_align: TextAlign,
    /// Source image for image elements, relative to the document.
    pub png: Option<PathBuf>,
    /// Auto layout settings for frames and groups.
    pub layout: AutoLayout,
    /// Horizontal and vertical resize constraints.
    pub constraints: [Constraint; 2],
    /// Whether children are clipped to the element bounds.
    pub clips_content: bool,
}

impl Default for Design {
    fn default() -> Self {
        Self {
            kind: DesignKind::Frame,
            size: [160.0, 96.0],
            fill: Fill::default(),
            stroke: Stroke::default(),
            corner: 0.0,
            shadow: None,
            blur: 0.0,
            text: "Text".into(),
            font_size: 20,
            text_color: [255, 255, 255, 255],
            text_align: TextAlign::Start,
            png: None,
            layout: AutoLayout::default(),
            constraints: [Constraint::Start; 2],
            clips_content: false,
        }
    }
}

impl Design {
    /// Checks document settings independently of external resources.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.size
                .iter()
                .all(|value| value.is_finite() && (1.0..=4096.0).contains(value)),
            "Design size must be 1 to 4096 pixels"
        );
        ensure!(
            self.corner.is_finite() && self.corner.abs() <= 2048.0,
            "Invalid corner radius"
        );
        ensure!(
            self.stroke.width.is_finite() && self.stroke.width.abs() <= 256.0,
            "Invalid stroke width"
        );
        ensure!(
            self.blur.is_finite() && self.blur.abs() <= 256.0,
            "Invalid blur radius"
        );
        ensure!(
            (1..=256).contains(&self.font_size) && self.text.len() <= 16384,
            "Invalid design text"
        );
        if let Fill::Noise(texture) = &self.fill {
            texture.validate()?;
        }
        ensure!(
            self.layout.gap.abs() <= 4096.0
                && self
                    .layout
                    .padding
                    .iter()
                    .all(|value| value.abs() <= 4096.0)
                && (0.0..=1.0).contains(&self.layout.align),
            "Invalid auto layout"
        );
        if let Some(shadow) = &self.shadow {
            ensure!(
                shadow.offset.iter().all(|value| value.abs() <= 4096.0)
                    && shadow.blur.is_finite()
                    && shadow.blur.abs() <= 256.0,
                "Invalid shadow"
            );
        }
        Ok(())
    }

    /// Bakes the element into one RGBA texture in design pixels.
    pub(crate) fn resolve(&self, dir: &Path, assets: &mut Assets) -> Result<RgbaImage> {
        self.validate()?;
        if !self.kind.is_renderable() {
            return Ok(RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 0])));
        }
        let width = self.size[0].round().max(1.0) as u32;
        let height = self.size[1].round().max(1.0) as u32;
        let mut image = self.fill_image(width, height)?;
        match self.kind {
            DesignKind::Image => {
                let id = if let Some(png) = &self.png {
                    assets.import_file(&dir.join(png))?
                } else {
                    assets.import_png(include_bytes!("../../app/demo/aurora.png"))?
                };
                let source = assets.texture(id).context("Missing design image")?;
                let scaled =
                    imageops::resize(source, width, height, imageops::FilterType::Lanczos3);
                imageops::overlay(&mut image, &scaled, 0, 0);
            }
            DesignKind::Text => {
                let font = FontArc::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT)?;
                let text = super::raster_text(
                    &font,
                    &[super::Span {
                        text: self.text.clone(),
                        size: self.font_size,
                        color: self.text_color,
                    }],
                )?;
                let x = match self.text_align {
                    TextAlign::Start => 0,
                    TextAlign::Center => (width as i64 - text.width() as i64) / 2,
                    TextAlign::End => width as i64 - text.width() as i64,
                };
                imageops::overlay(&mut image, &text, x, 0);
            }
            _ => {}
        }
        self.mask_and_stroke(&mut image, width, height);
        Ok(image)
    }

    fn fill_image(&self, width: u32, height: u32) -> Result<RgbaImage> {
        let mut image = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 0]));
        match &self.fill {
            Fill::Solid(color) => {
                for y in 0..height {
                    for x in 0..width {
                        *image.get_pixel_mut(x, y) = Rgba(*color);
                    }
                }
            }
            Fill::Noise(texture) => {
                let source = texture.render()?;
                image = imageops::resize(&source, width, height, imageops::FilterType::Nearest);
            }
            Fill::Linear { start, end, angle } => {
                let radians = angle.to_radians();
                let (dx, dy) = (radians.cos().abs(), radians.sin().abs());
                for y in 0..height {
                    for x in 0..width {
                        let nx = if width > 1 {
                            x as f32 / (width - 1) as f32
                        } else {
                            0.0
                        };
                        let ny = if height > 1 {
                            y as f32 / (height - 1) as f32
                        } else {
                            0.0
                        };
                        let t = (nx * dx + ny * dy) / (dx + dy).max(1e-3);
                        *image.get_pixel_mut(x, y) = Rgba(std::array::from_fn(|channel| {
                            (start[channel] as f32 * (1.0 - t) + end[channel] as f32 * t) as u8
                        }));
                    }
                }
            }
        }
        Ok(image)
    }

    fn mask_and_stroke(&self, image: &mut RgbaImage, width: u32, height: u32) {
        let radius = self.corner.max(0.0).min(width.min(height) as f32 * 0.5);
        let stroke = self.stroke.width.max(0.0);
        if radius <= 0.0 && stroke <= 0.0 {
            return;
        }
        for y in 0..height {
            for x in 0..width {
                let inside = if self.kind == DesignKind::Ellipse {
                    let rx = width as f32 * 0.5;
                    let ry = height as f32 * 0.5;
                    ((x as f32 + 0.5 - rx) / rx).powi(2) + ((y as f32 + 0.5 - ry) / ry).powi(2)
                        <= 1.0
                } else if radius > 0.0 {
                    let cx = if (x as f32) < radius {
                        radius
                    } else if (x as f32) > width as f32 - radius {
                        width as f32 - radius
                    } else {
                        x as f32
                    };
                    let cy = if (y as f32) < radius {
                        radius
                    } else if (y as f32) > height as f32 - radius {
                        height as f32 - radius
                    } else {
                        y as f32
                    };
                    (x as f32 - cx).powi(2) + (y as f32 - cy).powi(2) <= radius * radius
                } else {
                    true
                };
                let border = stroke > 0.0
                    && (x as f32 <= stroke
                        || y as f32 <= stroke
                        || width as f32 - x as f32 <= stroke
                        || height as f32 - y as f32 <= stroke);
                let pixel = image.get_pixel_mut(x, y);
                if !inside {
                    pixel.0[3] = 0;
                } else if border && self.stroke.color[3] > 0 {
                    *pixel = Rgba(self.stroke.color);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn design_elements_validate_their_geometry() {
        let mut element = Design::default();
        assert!(element.validate().is_ok());
        element.size = [0.0, 96.0];
        assert!(element.validate().is_err());
        element.size = [160.0, 96.0];
        element.corner = 3000.0;
        assert!(element.validate().is_err());
    }

    #[test]
    fn rounded_rectangles_bake_in_design_pixels() -> Result<()> {
        let mut assets = Assets::new(None);
        let element = Design {
            kind: DesignKind::Rectangle,
            size: [64.0, 64.0],
            fill: Fill::Solid([255, 0, 0, 255]),
            corner: 16.0,
            ..Design::default()
        };
        let image = element.resolve(Path::new("."), &mut assets)?;
        assert_eq!(image.dimensions(), (64, 64));
        assert_eq!(image.get_pixel(32, 32).0, [255, 0, 0, 255]);
        assert_eq!(image.get_pixel(0, 0).0[3], 0);
        Ok(())
    }
}
