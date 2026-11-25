//! Converts a static glTF scene into a portable, editable Aurora prefab.
use anyhow::{Context, Result, bail, ensure};
use base64::Engine as _;
use engine_core::{
    save::{MAX_NODES, PrefabFile, VERSION},
    scene::spatial::{Geometry, Role, SpatialNode},
};
use glam::{EulerRot, Quat};
use gltf::{buffer::Source, mesh::Mode};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

const MAX_BYTES: usize = 32 * 1024 * 1024;

fn bounded(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= MAX_BYTES, "glTF input exceeds 32 MiB");
    Ok(bytes)
}

pub(super) fn import(source: &Path, destination: &Path) -> Result<()> {
    let mut gltf = gltf::Gltf::from_slice(&bounded(source)?)?;
    let base = source
        .canonicalize()?
        .parent()
        .context("Missing glTF directory")?
        .to_owned();
    let mut buffers = Vec::new();
    let mut bytes = 0usize;
    for buffer in gltf.document.buffers() {
        let data = match buffer.source() {
            Source::Bin => gltf.blob.take().context("Missing GLB binary chunk")?,
            Source::Uri(uri) if uri.starts_with("data:") => {
                let (mime, data) = uri.split_once(',').context("Invalid data URI")?;
                ensure!(
                    matches!(
                        mime,
                        "data:application/octet-stream;base64"
                            | "data:application/gltf-buffer;base64"
                    ),
                    "Unsupported glTF data URI"
                );
                base64::engine::general_purpose::STANDARD.decode(data)?
            }
            Source::Uri(uri) => {
                ensure!(
                    !uri.contains(':') && !Path::new(uri).is_absolute(),
                    "Unsupported glTF buffer URI"
                );
                let path = base.join(uri).canonicalize()?;
                ensure!(
                    path.starts_with(&base),
                    "glTF buffer escapes asset directory"
                );
                bounded(&path)?
            }
        };
        bytes += data.len();
        ensure!(
            bytes <= MAX_BYTES && data.len() >= buffer.length(),
            "Invalid glTF buffer length"
        );
        buffers.push(data);
    }
    let scene = gltf
        .default_scene()
        .or_else(|| gltf.scenes().next())
        .context("glTF has no scene")?;
    let name = source
        .file_stem()
        .context("Missing model name")?
        .to_string_lossy();
    let stem: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let mut nodes = vec![super::node(&name, None)];
    nodes[0].spatial = Some(SpatialNode::default());
    let mut pending: Vec<_> = scene.nodes().map(|n| (n, 0)).collect();
    pending.reverse();
    let mut visited = BTreeSet::new();
    let mut total_indices = 0;
    while let Some((node, parent)) = pending.pop() {
        ensure!(
            nodes.len() < MAX_NODES && visited.insert(node.index()),
            "Repeated glTF node or scene too large"
        );
        let index = nodes.len();
        let (position, rotation, scale) = node.transform().decomposed();
        let (x, y, z) = Quat::from_array(rotation).to_euler(EulerRot::XYZ);
        let mut authored = super::node(node.name().unwrap_or("Node 3D"), Some(parent));
        authored.spatial = Some(SpatialNode {
            position,
            rotation: [x, y, z].map(f32::to_degrees),
            scale,
            ..Default::default()
        });
        nodes.push(authored);
        if let Some(mesh) = node.mesh() {
            for primitive in mesh.primitives() {
                ensure!(
                    primitive.mode() == Mode::Triangles,
                    "Only triangle glTF primitives are supported"
                );
                let reader = primitive.reader(|buf| buffers.get(buf.index()).map(Vec::as_slice));
                let positions = reader
                    .read_positions()
                    .context("Missing or invalid glTF positions")?;
                ensure!(
                    positions.len() <= 250_000,
                    "glTF primitive exceeds vertex limit"
                );
                let positions: Vec<_> = positions.collect();
                let uvs = reader
                    .read_tex_coords(0)
                    .map(|coords| coords.into_f32().collect::<Vec<_>>());
                ensure!(
                    uvs.as_ref().is_none_or(|uvs| uvs.len() == positions.len()),
                    "glTF texture coordinate count does not match its vertices"
                );
                let indices = if primitive.indices().is_some() {
                    let iter = reader
                        .read_indices()
                        .context("Invalid glTF index buffer")?
                        .into_u32();
                    ensure!(
                        iter.len() <= 750_000,
                        "glTF primitive exceeds triangle limit"
                    );
                    iter.collect()
                } else {
                    (0..positions.len() as u32).collect()
                };
                let geometry = Geometry {
                    positions,
                    indices,
                    uvs,
                };
                geometry.validate()?;
                total_indices += geometry.indices.len();
                ensure!(
                    total_indices <= 750_000 && nodes.len() < MAX_NODES,
                    "glTF scene exceeds import limits"
                );
                let pbr = primitive.material().pbr_metallic_roughness();
                let [r, g, b, _] = pbr.base_color_factor();
                let texture = match pbr.base_color_texture() {
                    Some(info) if info.tex_coord() == 0 && geometry.uvs.is_some() => {
                        Some(export_texture(
                            info.texture().source(),
                            &buffers,
                            &base,
                            destination,
                            &stem,
                        )?)
                    }
                    _ => None,
                };
                let mut part = super::node(mesh.name().unwrap_or("Mesh"), Some(index));
                part.spatial = Some(SpatialNode {
                    role: Role::Mesh,
                    color: [r, g, b],
                    texture,
                    metallic: pbr.metallic_factor().clamp(0., 1.),
                    roughness: pbr.roughness_factor().clamp(0., 1.),
                    geometry: Some(Arc::new(geometry)),
                    ..Default::default()
                });
                nodes.push(part);
            }
        }
        let children: Vec<_> = node.children().collect();
        pending.extend(children.into_iter().rev().map(|n| (n, index)));
    }
    if total_indices == 0 {
        bail!("glTF scene has no triangle meshes");
    }
    let prefab = PrefabFile {
        version: VERSION,
        nodes,
    };
    fs::create_dir_all(destination)?;
    let target = destination.join(format!("{name}.prefab"));
    let staging = tempfile::tempdir_in(destination)?;
    let staged = staging.path().join("model.prefab");
    prefab.save(&staged)?;
    // Atomic no-clobber publication on the same filesystem.
    fs::hard_link(staged, target).context("Publishing imported prefab")?;
    Ok(())
}

/// Decodes one glTF image and publishes it as a project-local PNG.
fn export_texture(
    image: gltf::image::Image<'_>,
    buffers: &[Vec<u8>],
    base: &Path,
    destination: &Path,
    stem: &str,
) -> Result<PathBuf> {
    let bytes = match image.source() {
        gltf::image::Source::Uri { uri, .. } if uri.starts_with("data:") => {
            let (_, payload) = uri.split_once(',').context("Invalid glTF image data URI")?;
            base64::engine::general_purpose::STANDARD.decode(payload)?
        }
        gltf::image::Source::Uri { uri, .. } => {
            ensure!(
                !uri.contains(':') && !Path::new(uri).is_absolute(),
                "Unsupported glTF image URI"
            );
            let path = base.join(uri).canonicalize()?;
            ensure!(path.starts_with(base), "glTF image escapes asset directory");
            bounded(&path)?
        }
        gltf::image::Source::View { view, .. } => {
            let data = buffers
                .get(view.buffer().index())
                .context("Missing glTF image buffer")?;
            let end = view
                .offset()
                .checked_add(view.length())
                .context("Invalid glTF image view")?;
            data.get(view.offset()..end)
                .context("glTF image view is out of bounds")?
                .to_vec()
        }
    };
    ensure!(bytes.len() <= MAX_BYTES, "glTF texture exceeds 32 MiB");
    let decoded = image::load_from_memory(&bytes).context("Unsupported glTF texture")?;
    ensure!(
        decoded.width() <= 4096 && decoded.height() <= 4096,
        "glTF texture exceeds 4096 pixels"
    );
    let directory = destination.join("textures");
    fs::create_dir_all(&directory)?;
    let relative = PathBuf::from(format!("textures/{stem}-{}.png", image.index()));
    let staging = tempfile::tempdir_in(&directory)?;
    let staged = staging.path().join("texture.png");
    decoded.to_rgba8().save(&staged)?;
    fs::rename(staged, destination.join(&relative))?;
    Ok(relative)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn textured_material_import_preserves_uvs_and_embeds_the_png() -> Result<()> {
        let source = tempfile::tempdir()?;
        let destination = tempfile::tempdir()?;
        let mut mesh = Vec::new();
        for point in [[0_f32, 0., 0.], [1., 0., 0.], [0., 1., 0.]] {
            for value in point {
                mesh.extend(value.to_le_bytes());
            }
        }
        for uv in [[0_f32, 0.], [1., 0.], [0., 1.]] {
            for value in uv {
                mesh.extend(value.to_le_bytes());
            }
        }
        fs::write(source.path().join("mesh.bin"), mesh)?;
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            1,
            1,
            image::Rgba([200, 100, 50, 255]),
        ))
        .write_to(&mut encoded, image::ImageFormat::Png)?;
        let png = base64::engine::general_purpose::STANDARD.encode(encoded.into_inner());
        let document = r#"{
            "asset": {"version": "2.0"},
            "scene": 0,
            "scenes": [{"nodes": [0]}],
            "nodes": [{"name": "Statue", "mesh": 0}],
            "meshes": [{"primitives": [{
                "attributes": {"POSITION": 0, "TEXCOORD_0": 1},
                "material": 0
            }]}],
            "materials": [{"pbrMetallicRoughness": {
                "baseColorFactor": [0.5, 0.6, 0.7, 1],
                "metallicFactor": 0.8,
                "roughnessFactor": 0.35,
                "baseColorTexture": {"index": 0}
            }}],
            "samplers": [{}],
            "textures": [{"source": 0, "sampler": 0}],
            "images": [{"uri": "data:image/png;base64,__PNG__"}],
            "buffers": [{"uri": "mesh.bin", "byteLength": 60}],
            "bufferViews": [
                {"buffer": 0, "byteOffset": 0, "byteLength": 36},
                {"buffer": 0, "byteOffset": 36, "byteLength": 24}
            ],
            "accessors": [
                {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0, 0, 0], "max": [1, 1, 0]},
                {"bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC2"}
            ]
        }"#
        .replace("__PNG__", &png);
        let path = source.path().join("statue.gltf");
        fs::write(&path, document)?;

        import(&path, destination.path())?;
        let prefab = PrefabFile::load(&destination.path().join("statue.prefab"))?;
        let mesh = prefab.nodes[2]
            .spatial
            .as_ref()
            .context("Imported material is missing")?;
        assert_eq!(mesh.color, [0.5, 0.6, 0.7]);
        assert_eq!(mesh.metallic, 0.8);
        assert_eq!(mesh.roughness, 0.35);
        let texture = mesh
            .texture
            .as_ref()
            .context("Imported texture is missing")?;
        assert_eq!(texture, Path::new("textures/statue-0.png"));
        assert!(destination.path().join(texture).is_file());
        assert_eq!(
            mesh.geometry
                .as_ref()
                .context("Imported geometry is missing")?
                .uvs
                .as_deref(),
            Some([[0., 0.], [1., 0.], [0., 1.]].as_slice())
        );
        Ok(())
    }

    #[test]
    fn triangle_import_is_portable_and_never_overwrites_an_authored_prefab() -> Result<()> {
        let source = tempfile::tempdir()?;
        let destination = tempfile::tempdir()?;
        let points = [0_f32, 0., 0., 1., 0., 0., 0., 1., 0.];
        let bytes: Vec<u8> = points.into_iter().flat_map(f32::to_le_bytes).collect();
        fs::write(source.path().join("triangle.bin"), bytes)?;
        let path = source.path().join("art.gltf");
        fs::write(
            &path,
            br#"{
                "asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],
                "nodes":[{"name":"Panel","mesh":0,"translation":[2,3,4]}],
                "meshes":[{"primitives":[{"attributes":{"POSITION":0},"material":0}]}],
                "materials":[{"pbrMetallicRoughness":{"baseColorFactor":[0.1,0.2,0.3,1]}}],
                "buffers":[{"uri":"triangle.bin","byteLength":36}],
                "bufferViews":[{"buffer":0,"byteLength":36}],
                "accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}]
            }"#,
        )?;
        import(&path, destination.path())?;
        assert!(import(&path, destination.path()).is_err());
        let prefab = PrefabFile::load(&destination.path().join("art.prefab"))?;
        let transform = prefab.nodes[1]
            .spatial
            .as_ref()
            .context("Imported transform is missing")?;
        assert_eq!(transform.position, [2., 3., 4.]);
        let mesh = prefab.nodes[2]
            .spatial
            .as_ref()
            .context("Imported mesh is missing")?;
        assert_eq!(mesh.color, [0.1, 0.2, 0.3]);
        assert_eq!(
            mesh.geometry
                .as_ref()
                .context("Imported mesh geometry is missing")?
                .indices,
            [0, 1, 2]
        );
        Ok(())
    }
}
