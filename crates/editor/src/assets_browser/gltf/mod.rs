//! Converts a static glTF scene into a portable, editable Aurora prefab.
use anyhow::{Context, Result, bail, ensure};
use base64::Engine as _;
use engine_core::{
    save::{MAX_NODES, PrefabFile, VERSION},
    scene::spatial::{Geometry, Role, SpatialNode},
};
use glam::{EulerRot, Quat};
use gltf::{buffer::Source, mesh::Mode};
use std::{collections::BTreeSet, fs, io::Read, path::Path, sync::Arc};

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
                let geometry = Geometry { positions, indices };
                geometry.validate()?;
                total_indices += geometry.indices.len();
                ensure!(
                    total_indices <= 750_000 && nodes.len() < MAX_NODES,
                    "glTF scene exceeds import limits"
                );
                let [r, g, b, _] = primitive
                    .material()
                    .pbr_metallic_roughness()
                    .base_color_factor();
                let mut part = super::node(mesh.name().unwrap_or("Mesh"), Some(index));
                part.spatial = Some(SpatialNode {
                    role: Role::Mesh,
                    color: [r, g, b],
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn triangle_import_is_portable_and_never_overwrites_an_authored_prefab() -> Result<()> {
        let source = tempfile::tempdir()?;
        let destination = tempfile::tempdir()?;
        let points = [0_f32, 0., 0., 1., 0., 0., 0., 1., 0.];
        let bytes: Vec<u8> = points.into_iter().flat_map(f32::to_le_bytes).collect();
        fs::write(source.path().join("triangle.bin"), bytes)?;
        let document = json!({
            "asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],
            "nodes":[{"name":"Panel","mesh":0,"translation":[2,3,4]}],
            "meshes":[{"primitives":[{"attributes":{"POSITION":0},"material":0}]}],
            "materials":[{"pbrMetallicRoughness":{"baseColorFactor":[0.1,0.2,0.3,1]}}],
            "buffers":[{"uri":"triangle.bin","byteLength":36}],
            "bufferViews":[{"buffer":0,"byteLength":36}],
            "accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}]
        });
        let path = source.path().join("art.gltf");
        fs::write(&path, serde_json::to_vec(&document)?)?;
        import(&path, destination.path())?;
        assert!(import(&path, destination.path()).is_err());
        let prefab = PrefabFile::load(&destination.path().join("art.prefab"))?;
        assert_eq!(
            prefab.nodes[1].spatial.as_ref().unwrap().position,
            [2., 3., 4.]
        );
        let mesh = prefab.nodes[2].spatial.as_ref().unwrap();
        assert_eq!(mesh.color, [0.1, 0.2, 0.3]);
        assert_eq!(mesh.geometry.as_ref().unwrap().indices, [0, 1, 2]);
        Ok(())
    }
}
