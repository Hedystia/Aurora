//! Converts a static glTF scene into a portable, editable Aurora prefab.
use anyhow::{Context, Result, bail, ensure};
use base64::Engine as _;
use engine_core::{
    animation::{Clip, Interpolation as ChannelInterpolation, Key, Property, Track, Wrap},
    save::{MAX_NODES, PrefabFile, VERSION},
    scene::spatial::{Geometry, Role, SpatialNode, bake_skin},
};
use glam::{EulerRot, Mat4, Quat};
use gltf::{buffer::Source, mesh::Mode};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

const MAX_BYTES: usize = 32 * 1024 * 1024;
const SAMPLE_RATE: u32 = 60;
const MAX_ANIMATION_FRAMES: u32 = 36_000;
const MAX_ANIMATION_KEYS: usize = 8_192;

fn bounded(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= MAX_BYTES, "glTF input exceeds 32 MiB");
    Ok(bytes)
}

/// Rest-pose world matrices for every glTF node, keyed by its document index.
fn node_worlds(document: &gltf::Document) -> BTreeMap<usize, Mat4> {
    fn world(
        index: usize,
        parents: &BTreeMap<usize, usize>,
        locals: &BTreeMap<usize, Mat4>,
        cache: &mut BTreeMap<usize, Mat4>,
    ) -> Mat4 {
        if let Some(matrix) = cache.get(&index) {
            return *matrix;
        }
        let local = locals.get(&index).copied().unwrap_or(Mat4::IDENTITY);
        let matrix = parents.get(&index).map_or(local, |parent| {
            world(*parent, parents, locals, cache) * local
        });
        cache.insert(index, matrix);
        matrix
    }

    let mut parents = BTreeMap::new();
    let mut locals = BTreeMap::new();
    for node in document.nodes() {
        locals.insert(
            node.index(),
            Mat4::from_cols_array_2d(&node.transform().matrix()),
        );
        for child in node.children() {
            parents.insert(child.index(), node.index());
        }
    }
    let mut cache = BTreeMap::new();
    for index in locals.keys().copied().collect::<Vec<_>>() {
        world(index, &parents, &locals, &mut cache);
    }
    cache
}

/// Inverse bind matrices and rest joint transforms in glTF joint-array order.
///
/// Joint matrices are expressed relative to the skinned node, whose own local
/// transform is ignored by glTF. The pair is consumed by [`bake_skin`].
fn skin_matrices(
    skin: gltf::Skin<'_>,
    buffers: &[Vec<u8>],
    worlds: &BTreeMap<usize, Mat4>,
    base: Mat4,
) -> Result<(Vec<Mat4>, Vec<Mat4>)> {
    let joints: Vec<_> = skin.joints().collect();
    ensure!(
        !joints.is_empty() && joints.len() <= usize::from(u16::MAX) + 1,
        "Unsupported glTF skin joint count"
    );
    let inverse_bind: Vec<_> = match skin
        .reader(|buffer| buffers.get(buffer.index()).map(Vec::as_slice))
        .read_inverse_bind_matrices()
    {
        Some(values) => values
            .map(|matrix| Mat4::from_cols_array_2d(&matrix))
            .collect(),
        None => vec![Mat4::IDENTITY; joints.len()],
    };
    ensure!(
        inverse_bind.len() >= joints.len(),
        "glTF inverse bind matrix count does not match its joints"
    );
    let base_inverse = base.inverse();
    let joint_world = joints
        .iter()
        .map(|joint| {
            base_inverse
                * worlds
                    .get(&joint.index())
                    .copied()
                    .unwrap_or(Mat4::IDENTITY)
        })
        .collect();
    Ok((inverse_bind, joint_world))
}

struct ImportedChannel {
    node: usize,
    properties: [Property; 3],
    interpolation: ChannelInterpolation,
    frames: Vec<u32>,
    values: Vec<[f32; 3]>,
}

/// Converts glTF animation samplers into scene clips sampled at 60 Hz.
///
/// Quaternion keys are decomposed to XYZ Euler degrees and blended per axis,
/// which approximates rather than reproduces spherical interpolation.
/// Cubic-spline samplers and morph-target weights are rejected explicitly.
fn import_animations(
    document: &gltf::Document,
    buffers: &[Vec<u8>],
    nodes: &BTreeMap<usize, usize>,
    node_count: usize,
) -> Result<Vec<Clip>> {
    use gltf::animation::util::ReadOutputs;
    use gltf::animation::{Interpolation as SourceInterpolation, Property as SourceProperty};

    let mut clips = Vec::new();
    for animation in document.animations() {
        let mut channels = Vec::new();
        for channel in animation.channels() {
            let Some(&node) = nodes.get(&channel.target().node().index()) else {
                continue;
            };
            let interpolation = match channel.sampler().interpolation() {
                SourceInterpolation::Step => ChannelInterpolation::Step,
                SourceInterpolation::Linear => ChannelInterpolation::Linear,
                SourceInterpolation::CubicSpline => {
                    bail!("glTF cubic spline animation interpolation is not supported")
                }
            };
            let reader = channel.reader(|buffer| buffers.get(buffer.index()).map(Vec::as_slice));
            let inputs: Vec<_> = reader
                .read_inputs()
                .context("Invalid glTF animation input accessor")?
                .collect();
            ensure!(
                !inputs.is_empty() && inputs.len() <= MAX_ANIMATION_KEYS,
                "Invalid glTF animation key count"
            );
            ensure!(
                inputs.windows(2).all(|times| times[0] <= times[1]),
                "glTF animation inputs are out of order"
            );
            let (properties, values): ([Property; 3], Vec<[f32; 3]>) =
                match (channel.target().property(), reader.read_outputs()) {
                    (SourceProperty::Translation, Some(ReadOutputs::Translations(values))) => (
                        [
                            Property::TranslationX,
                            Property::TranslationY,
                            Property::TranslationZ,
                        ],
                        values.collect(),
                    ),
                    (SourceProperty::Scale, Some(ReadOutputs::Scales(values))) => (
                        [
                            Property::SpatialScaleX,
                            Property::SpatialScaleY,
                            Property::SpatialScaleZ,
                        ],
                        values.collect(),
                    ),
                    (SourceProperty::Rotation, Some(ReadOutputs::Rotations(rotations))) => (
                        [
                            Property::RotationX,
                            Property::RotationY,
                            Property::RotationZ,
                        ],
                        rotations
                            .into_f32()
                            .map(|quaternion| {
                                let (x, y, z) =
                                    Quat::from_array(quaternion).to_euler(EulerRot::XYZ);
                                [x.to_degrees(), y.to_degrees(), z.to_degrees()]
                            })
                            .collect(),
                    ),
                    (SourceProperty::MorphTargetWeights, _) => {
                        bail!("glTF morph-target animation is not supported")
                    }
                    _ => bail!("glTF animation output does not match its target property"),
                };
            ensure!(
                values.len() == inputs.len(),
                "glTF animation sampler length mismatch"
            );
            if properties[0] == Property::SpatialScaleX {
                ensure!(
                    values
                        .iter()
                        .all(|value| value.iter().all(|scale| *scale > 0.0)),
                    "glTF animation has a non-positive scale key"
                );
            }
            let mut frames = Vec::with_capacity(inputs.len());
            let mut previous: Option<u32> = None;
            for time in inputs {
                let seconds = f64::from(time);
                ensure!(
                    seconds.is_finite() && seconds >= 0.0,
                    "Invalid glTF animation time"
                );
                let frame = (seconds * f64::from(SAMPLE_RATE)).round();
                let frame = (frame as u64).min(u64::from(MAX_ANIMATION_FRAMES)) as u32;
                let frame = previous.map_or(frame, |last| frame.max(last.saturating_add(1)));
                ensure!(frame <= MAX_ANIMATION_FRAMES, "glTF animation is too long");
                frames.push(frame);
                previous = Some(frame);
            }
            channels.push(ImportedChannel {
                node,
                properties,
                interpolation,
                frames,
                values,
            });
        }
        if channels.is_empty() {
            continue;
        }
        let duration = channels
            .iter()
            .flat_map(|channel| channel.frames.iter().copied())
            .max()
            .unwrap_or(1)
            .max(1);
        let mut tracks = Vec::new();
        for channel in channels {
            for axis in 0..3 {
                tracks.push(Track {
                    node: channel.node,
                    property: channel.properties[axis],
                    interpolation: channel.interpolation,
                    keys: channel
                        .frames
                        .iter()
                        .zip(&channel.values)
                        .map(|(&frame, value)| Key {
                            frame,
                            value: value[axis],
                            tangents: None,
                        })
                        .collect(),
                });
            }
        }
        let clip = Clip {
            name: animation.name().unwrap_or("Animation").to_owned(),
            sample_rate: SAMPLE_RATE,
            duration,
            speed: 1.0,
            wrap: Wrap::Once,
            tracks,
            events: Vec::new(),
        };
        clip.validate(node_count)?;
        clips.push(clip);
    }
    Ok(clips)
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
    let worlds = node_worlds(&gltf.document);
    let mut node_map = BTreeMap::new();
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
        node_map.insert(node.index(), index);
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
            let skin = node
                .skin()
                .map(|skin| {
                    let base = worlds.get(&node.index()).copied().unwrap_or(Mat4::IDENTITY);
                    skin_matrices(skin, &buffers, &worlds, base)
                })
                .transpose()?;
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
                let positions = if let Some((inverse_bind, joint_world)) = &skin {
                    let joints: Vec<_> = reader
                        .read_joints(0)
                        .context("Skinned glTF primitive is missing JOINTS_0")?
                        .into_u16()
                        .collect();
                    let weights: Vec<_> = reader
                        .read_weights(0)
                        .context("Skinned glTF primitive is missing WEIGHTS_0")?
                        .into_f32()
                        .collect();
                    ensure!(
                        joints.len() == positions.len() && weights.len() == positions.len(),
                        "glTF skin weights do not match its vertices"
                    );
                    bake_skin(&positions, &joints, &weights, inverse_bind, joint_world)?
                } else {
                    positions
                };
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
    let animations = import_animations(&gltf.document, &buffers, &node_map, nodes.len())?;
    let prefab = PrefabFile {
        version: VERSION,
        nodes,
        animations,
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
    fn animation_import_converts_sampler_keys_to_60hz_tracks() -> Result<()> {
        let source = tempfile::tempdir()?;
        let destination = tempfile::tempdir()?;
        let mut bytes = Vec::new();
        for point in [[0_f32, 0., 0.], [1., 0., 0.], [0., 1., 0.]] {
            for value in point {
                bytes.extend(value.to_le_bytes());
            }
        }
        for time in [0_f32, 1.0] {
            bytes.extend(time.to_le_bytes());
        }
        for translation in [[0_f32, 0., 0.], [0., 2., 0.]] {
            for value in translation {
                bytes.extend(value.to_le_bytes());
            }
        }
        fs::write(source.path().join("animated.bin"), &bytes)?;
        let path = source.path().join("animated.gltf");
        fs::write(
            &path,
            br#"{
                "asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],
                "nodes":[{"name":"Mover","mesh":0}],
                "meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],
                "animations":[{"name":"Move","channels":[
                    {"sampler":0,"target":{"node":0,"path":"translation"}}
                ],"samplers":[{"input":1,"output":2,"interpolation":"LINEAR"}]}],
                "buffers":[{"uri":"animated.bin","byteLength":68}],
                "bufferViews":[
                    {"buffer":0,"byteOffset":0,"byteLength":36},
                    {"buffer":0,"byteOffset":36,"byteLength":8},
                    {"buffer":0,"byteOffset":44,"byteLength":24}
                ],
                "accessors":[
                    {"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]},
                    {"bufferView":1,"componentType":5126,"count":2,"type":"SCALAR","min":[0],"max":[1]},
                    {"bufferView":2,"componentType":5126,"count":2,"type":"VEC3"}
                ]
            }"#,
        )?;

        import(&path, destination.path())?;
        let prefab = PrefabFile::load(&destination.path().join("animated.prefab"))?;
        assert_eq!(prefab.animations.len(), 1);
        let clip = &prefab.animations[0];
        assert_eq!(clip.name, "Move");
        assert_eq!(clip.duration, 60);
        assert_eq!(clip.tracks.len(), 3);
        let track = clip
            .tracks
            .iter()
            .find(|track| track.property == Property::TranslationY)
            .context("Missing translation Y channel")?;
        assert_eq!(track.node, 1);
        assert_eq!(
            track
                .keys
                .iter()
                .map(|key| (key.frame, key.value))
                .collect::<Vec<_>>(),
            [(0, 0.0), (60, 2.0)]
        );
        Ok(())
    }

    #[test]
    fn skinned_import_bakes_the_rest_pose_into_mesh_positions() -> Result<()> {
        let source = tempfile::tempdir()?;
        let destination = tempfile::tempdir()?;
        let mut bytes = Vec::new();
        for point in [[0_f32, 0., 0.], [1., 0., 0.], [0., 1., 0.]] {
            for value in point {
                bytes.extend(value.to_le_bytes());
            }
        }
        for _ in 0..3 {
            for value in [0_u16, 0, 0, 0] {
                bytes.extend(value.to_le_bytes());
            }
        }
        for _ in 0..3 {
            for value in [1_f32, 0., 0., 0.] {
                bytes.extend(value.to_le_bytes());
            }
        }
        for value in [
            1_f32, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ] {
            bytes.extend(value.to_le_bytes());
        }
        fs::write(source.path().join("skinned.bin"), &bytes)?;
        let path = source.path().join("skinned.gltf");
        fs::write(
            &path,
            br#"{
                "asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0,1]}],
                "nodes":[
                    {"name":"Joint","translation":[1,0,0]},
                    {"name":"Mesh","mesh":0,"skin":0}
                ],
                "skins":[{"joints":[0],"inverseBindMatrices":3}],
                "meshes":[{"primitives":[{"attributes":{
                    "POSITION":0,"JOINTS_0":1,"WEIGHTS_0":2
                }}]}],
                "buffers":[{"uri":"skinned.bin","byteLength":172}],
                "bufferViews":[
                    {"buffer":0,"byteOffset":0,"byteLength":36},
                    {"buffer":0,"byteOffset":36,"byteLength":24},
                    {"buffer":0,"byteOffset":60,"byteLength":48},
                    {"buffer":0,"byteOffset":108,"byteLength":64}
                ],
                "accessors":[
                    {"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]},
                    {"bufferView":1,"componentType":5123,"count":3,"type":"VEC4"},
                    {"bufferView":2,"componentType":5126,"count":3,"type":"VEC4"},
                    {"bufferView":3,"componentType":5126,"count":1,"type":"MAT4"}
                ]
            }"#,
        )?;

        import(&path, destination.path())?;
        let prefab = PrefabFile::load(&destination.path().join("skinned.prefab"))?;
        let mesh = prefab.nodes[3]
            .spatial
            .as_ref()
            .context("Imported mesh is missing")?;
        assert_eq!(
            mesh.geometry
                .as_ref()
                .context("Imported geometry is missing")?
                .positions,
            [[1., 0., 0.], [2., 0., 0.], [1., 1., 0.]]
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
