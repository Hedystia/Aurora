//! Content-addressed PNG import and persistent decoded image cache.

use std::{
    collections::HashMap,
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use image::{ImageFormat, ImageReader, Limits, RgbaImage};
use serde::Serialize;
use sha2::{Digest, Sha256};

const IMPORTER: &[u8] = b"aurora-png-rgba8-srgb-v1";
const MAX_DIMENSION: u32 = 4096;
const MAX_BYTES: usize = MAX_DIMENSION as usize * MAX_DIMENSION as usize * 4;

/// SHA-256 identity of the source image and importer format.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct AssetId([u8; 32]);

impl std::fmt::Display for AssetId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Imported textures and their optional on-disk cache.
pub struct Assets {
    cache: Option<PathBuf>,
    textures: HashMap<AssetId, RgbaImage>,
}

impl Assets {
    /// Creates an asset store backed by an optional cache directory.
    pub fn new(cache: Option<PathBuf>) -> Self {
        Self {
            cache,
            textures: HashMap::new(),
        }
    }

    /// Imports a PNG after reading it from the filesystem.
    pub fn import_file(&mut self, path: &Path) -> Result<AssetId> {
        let source = read_bounded(path, MAX_BYTES)?;
        self.import_png(&source)
            .with_context(|| format!("Importing {}", path.display()))
    }

    /// Imports a PNG in memory and returns its content identity.
    pub fn import_png(&mut self, source: &[u8]) -> Result<AssetId> {
        ensure!(source.len() <= MAX_BYTES, "PNG source is too large");
        let mut hash = Sha256::new_with_prefix(IMPORTER);
        hash.update(source);
        let id = AssetId(hash.finalize().into());
        if self.textures.contains_key(&id) {
            return Ok(id);
        }

        let cache = self
            .cache
            .as_ref()
            .map(|dir| dir.join(format!("{id}.aurora-texture")));
        let cached_image = cache.as_deref().and_then(|path| {
            let bytes = read_bounded(path, MAX_BYTES + 8 + 32).ok()?;
            let (digest, payload) = bytes.split_at_checked(32)?;
            let mut hash = Sha256::new_with_prefix(id.0);
            hash.update(payload);
            (hash.finalize().as_slice() == digest).then_some(())?;
            let width = u32::from_le_bytes(payload.get(..4)?.try_into().ok()?);
            let height = u32::from_le_bytes(payload.get(4..8)?.try_into().ok()?);
            (width > 0 && height > 0 && width <= MAX_DIMENSION && height <= MAX_DIMENSION)
                .then_some(())?;
            let pixels = payload.get(8..)?;
            (pixels.len() == width as usize * height as usize * 4).then_some(())?;
            RgbaImage::from_raw(width, height, pixels.to_vec())
        });
        let image = match cached_image {
            Some(image) => image,
            None => {
                let mut reader = ImageReader::with_format(Cursor::new(source), ImageFormat::Png);
                let mut limits = Limits::default();
                limits.max_image_width = Some(MAX_DIMENSION);
                limits.max_image_height = Some(MAX_DIMENSION);
                reader.limits(limits);
                let image = reader.decode()?.into_rgba8();
                if let (Some(directory), Some(path)) = (&self.cache, cache) {
                    fs::create_dir_all(directory)?;
                    let mut temp = tempfile::NamedTempFile::new_in(directory)?;
                    let width = image.width().to_le_bytes();
                    let height = image.height().to_le_bytes();
                    let mut hash = Sha256::new_with_prefix(id.0);
                    hash.update(width);
                    hash.update(height);
                    hash.update(image.as_raw());
                    temp.write_all(&hash.finalize())?;
                    temp.write_all(&width)?;
                    temp.write_all(&height)?;
                    temp.write_all(image.as_raw())?;
                    temp.persist(path)?;
                }
                image
            }
        };
        self.textures.insert(id, image);
        Ok(id)
    }

    pub(crate) fn generated(&mut self, image: RgbaImage) -> AssetId {
        let mut hash = Sha256::new_with_prefix(b"aurora-generated-rgba8-v1");
        hash.update(image.width().to_le_bytes());
        hash.update(image.height().to_le_bytes());
        hash.update(image.as_raw());
        let id = AssetId(hash.finalize().into());
        self.textures.entry(id).or_insert(image);
        id
    }

    /// Gets the decoded sRGB texture pixels.
    pub fn texture(&self, id: AssetId) -> Option<&RgbaImage> {
        self.textures.get(&id)
    }
}

pub(crate) fn read_bounded(path: &Path, max: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .with_context(|| format!("Opening {}", path.display()))?
        .take((max + 1) as u64)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= max,
        "{} exceeds the size limit",
        path.display()
    );
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, Rgba};

    fn png(color: [u8; 4]) -> Result<Vec<u8>> {
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, Rgba(color)))
            .write_to(&mut bytes, ImageFormat::Png)?;
        Ok(bytes.into_inner())
    }

    #[test]
    fn import_reuses_content_and_recovers_a_corrupt_cache() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let mut assets = Assets::new(Some(directory.path().into()));
        let bytes = png([7, 9, 11, 255])?;
        let id = assets.import_png(&bytes)?;
        assert_eq!(assets.import_png(&bytes)?, id);
        assert!(assets.import_png(b"invalid").is_err());
        let changed = assets.import_png(&png([11, 9, 7, 255])?)?;
        assert_ne!(changed, id);
        let file = directory.path().join(format!("{id}.aurora-texture"));
        let input = directory.path().join("test.png");
        fs::write(&input, bytes)?;
        let original = fs::read(&file)?;
        let mut cached = Assets::new(Some(directory.path().into()));
        assert_eq!(cached.import_file(&input)?, id);
        let mut corrupt = original.clone();
        *corrupt.last_mut().context("Empty cache entry")? ^= 1;
        fs::write(&file, corrupt)?;
        let mut recovered = Assets::new(Some(directory.path().into()));
        assert_eq!(recovered.import_file(&input)?, id);
        assert_eq!(fs::read(file)?, original);
        assert_eq!(
            recovered
                .texture(id)
                .context("Missing recovered image")?
                .get_pixel(0, 0)
                .0,
            [7, 9, 11, 255]
        );
        Ok(())
    }
}
