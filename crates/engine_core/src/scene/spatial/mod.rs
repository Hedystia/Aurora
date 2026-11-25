//! Optional spatial components coexist with the pixel-space scene graph.

use anyhow::{Result, ensure};
use glam::{EulerRot, Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::assets::{AssetId, Assets};

/// The visual or camera role of a spatial node.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Transform-only pivot.
    #[default]
    Pivot,
    /// Unit cube with a linear RGB material.
    Cube,
    /// Authored indexed triangles.
    Mesh,
    /// Perspective camera looking down its local negative Z axis.
    Camera,
    /// Directional light shining along local negative Z.
    DirectionalLight,
}

/// Physics participation of a spatial node.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Body {
    /// No collision.
    #[default]
    None,
    /// Immovable collider.
    Fixed,
    /// Solver-driven body that follows its authored transform.
    Kinematic,
    /// Gravity-driven rigid body.
    Dynamic,
}

/// Collider shape used by a physics-enabled spatial node.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Collider {
    /// Box sized by the node's local scale. This is the legacy shape.
    #[default]
    Cuboid,
    /// Sphere with a radius of half the largest axis scale.
    Sphere,
    /// Y-aligned capsule with hemispherical caps.
    Capsule,
}

/// Project-owned static triangle mesh in metres, imported from a glTF primitive.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Geometry {
    /// Local-space vertices.
    pub positions: Vec<[f32; 3]>,
    /// Three vertex indices per triangle.
    pub indices: Vec<u32>,
    /// Optional per-vertex texture coordinates matching `positions` in length.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uvs: Option<Vec<[f32; 2]>>,
}

impl Geometry {
    /// Validates topology, texture coordinates, and numeric limits before rendering.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.positions.is_empty()
                && self.positions.len() <= 250_000
                && self.positions.iter().flatten().all(|v| v.is_finite())
                && !self.indices.is_empty()
                && self.indices.len() <= 750_000
                && self.indices.len().is_multiple_of(3)
                && self
                    .indices
                    .iter()
                    .all(|i| (*i as usize) < self.positions.len())
                && self
                    .uvs
                    .as_ref()
                    .is_none_or(|uvs| uvs.len() == self.positions.len()
                        && uvs.iter().flatten().all(|v| v.is_finite())),
            "Invalid triangle mesh"
        );
        Ok(())
    }
}

/// Editable local transform and components, in metres and degrees.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SpatialNode {
    /// Visual role.
    pub role: Role,
    /// Parent-relative translation.
    pub position: [f32; 3],
    /// XYZ Euler rotation in degrees.
    pub rotation: [f32; 3],
    /// Positive local axis scales. A unit cube spans one metre.
    pub scale: [f32; 3],
    /// Linear RGB base color multiplied by the base-color texture when present.
    pub color: [f32; 3],
    /// Indexed triangles for a mesh role.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geometry: Option<std::sync::Arc<Geometry>>,
    /// Base-color PNG path relative to the containing document.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture: Option<PathBuf>,
    /// Resolved texture identity, filled while loading the document.
    #[serde(skip)]
    pub texture_asset: Option<AssetId>,
    /// PBR metalness in the zero-to-one range.
    pub metallic: f32,
    /// PBR perceptual roughness in the zero-to-one range.
    pub roughness: f32,
    /// Physics body type.
    pub body: Body,
    /// Collider shape. Defaults to the original cuboid for older documents.
    #[serde(default)]
    pub collider: Collider,
    /// Reports overlaps without applying contact forces.
    #[serde(default)]
    pub trigger: bool,
    /// Contact friction. The legacy value is preserved for older documents.
    #[serde(default)]
    pub friction: f32,
    /// Contact restitution. The legacy value is preserved for older documents.
    #[serde(default)]
    pub restitution: f32,
    /// Vertical field of view in degrees.
    pub fov: f32,
}

impl Default for SpatialNode {
    fn default() -> Self {
        Self {
            geometry: None,
            role: Role::Pivot,
            position: [0.; 3],
            rotation: [0.; 3],
            scale: [1.; 3],
            color: [0.35, 0.66, 0.9],
            texture: None,
            texture_asset: None,
            metallic: 0.,
            roughness: 0.5,
            body: Body::None,
            collider: Collider::Cuboid,
            trigger: false,
            friction: 0.65,
            restitution: 0.12,
            fov: 45.,
        }
    }
}

impl SpatialNode {
    /// Returns the parent-relative model matrix.
    pub fn matrix(&self) -> Mat4 {
        let [x, y, z] = self.rotation.map(f32::to_radians);
        Mat4::from_scale_rotation_translation(
            self.scale.into(),
            Quat::from_euler(EulerRot::XYZ, x, y, z),
            self.position.into(),
        )
    }

    /// Resolves the authored base-color texture against a document directory.
    pub(crate) fn resolve(
        &mut self,
        directory: &std::path::Path,
        assets: &mut Assets,
    ) -> Result<()> {
        self.validate()?;
        self.texture_asset = match &self.texture {
            Some(path) => Some(assets.import_file(&directory.join(path))?),
            None => None,
        };
        Ok(())
    }

    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.position
                .iter()
                .chain(&self.rotation)
                .all(|v| v.is_finite())
                && self.scale.iter().all(|v| v.is_finite() && *v > 0.)
                && self.color.iter().all(|v| (0.0..=1.).contains(v))
                && (0.0..=1.).contains(&self.metallic)
                && (0.0..=1.).contains(&self.roughness)
                && (1.0..179.).contains(&self.fov),
            "Invalid spatial component"
        );
        ensure!(
            self.texture.as_ref().is_none_or(|path| {
                !path.as_os_str().is_empty()
                    && path
                        .components()
                        .all(|part| matches!(part, std::path::Component::Normal(_)))
            }),
            "Spatial textures need a document-relative path"
        );
        ensure!(
            (self.role == Role::Mesh) == self.geometry.is_some(),
            "Mesh nodes require triangle geometry"
        );
        ensure!(
            self.role != Role::Mesh || self.metallic.is_finite() && self.roughness.is_finite(),
            "Mesh nodes require finite material values"
        );
        if let Some(mesh) = &self.geometry {
            mesh.validate()?;
        }
        ensure!(
            self.role == Role::Cube || self.body == Body::None,
            "Only cubes support physics colliders"
        );
        ensure!(
            (0.0..=10.0).contains(&self.friction) && (0.0..=1.0).contains(&self.restitution),
            "Invalid spatial physics material"
        );
        Ok(())
    }
}

/// Computes parent-first world matrices, carrying spatial ancestry through ordinary nodes.
pub fn matrices(
    nodes: &[super::NodeState],
) -> std::collections::BTreeMap<crate::ecs::EntityId, Mat4> {
    let mut worlds = std::collections::BTreeMap::new();
    for node in nodes {
        let parent = node
            .parent
            .and_then(|id| worlds.get(&id).copied())
            .unwrap_or(Mat4::IDENTITY);
        worlds.insert(
            node.id,
            parent
                * node
                    .spatial
                    .as_ref()
                    .map_or(Mat4::IDENTITY, SpatialNode::matrix),
        );
    }
    worlds
}

/// Bakes a glTF skin at its rest pose into mesh-local vertex positions.
///
/// Each vertex is transformed by the weight-normalized sum of
/// `joint_world * inverse_bind` for its four joints. This captures the skin's
/// bind pose honestly but cannot retain deformation from animated joints: the
/// resulting geometry is static and the mesh node transform is ignored, as in
/// glTF's skinned-mesh rule.
pub fn bake_skin(
    positions: &[[f32; 3]],
    joints: &[[u16; 4]],
    weights: &[[f32; 4]],
    inverse_bind: &[Mat4],
    joint_world: &[Mat4],
) -> Result<Vec<[f32; 3]>> {
    ensure!(
        positions.len() == joints.len() && joints.len() == weights.len(),
        "Skin vertex data has inconsistent lengths"
    );
    ensure!(
        inverse_bind.len() == joint_world.len(),
        "Skin has inconsistent joint data"
    );
    let transforms: Vec<_> = joint_world
        .iter()
        .zip(inverse_bind)
        .map(|(world, inverse)| *world * *inverse)
        .collect();
    let mut baked = Vec::with_capacity(positions.len());
    for (index, position) in positions.iter().enumerate() {
        let mut point = Vec3::ZERO;
        let mut total = 0.0;
        for (&joint, &weight) in joints[index].iter().zip(&weights[index]) {
            if weight == 0.0 {
                continue;
            }
            let joint = joint as usize;
            ensure!(joint < transforms.len(), "Skin joint index out of bounds");
            point += transforms[joint].transform_point3(Vec3::from_array(*position)) * weight;
            total += weight;
        }
        ensure!(total > 0.0, "Skin vertex has no effective weight");
        baked.push((point / total).to_array());
    }
    Ok(baked)
}

/// Perspective and lighting shared by the runtime and Scene view.
pub struct View3D {
    /// World-to-camera transformation.
    pub view: Mat4,
    /// Vertical field of view in degrees.
    pub fov: f32,
    /// Direction toward the light in world coordinates.
    pub light: Vec3,
}

impl Default for View3D {
    fn default() -> Self {
        Self {
            view: glam::camera::rh::view::look_at_mat4(
                Vec3::new(8.6, 6.6, 10.8),
                Vec3::new(0., 1.05, 0.),
                Vec3::Y,
            ),
            fov: 45.,
            light: Vec3::new(-0.4, 0.8, 0.5).normalize(),
        }
    }
}

impl View3D {
    /// Camera projection for a given target aspect ratio.
    pub fn projection(&self, aspect: f32) -> Mat4 {
        glam::camera::rh::proj::directx::perspective(self.fov.to_radians(), aspect, 0.1, 1000.)
            * self.view
    }

    /// Selects the first visible camera and directional light in tree order.
    pub fn resolve(
        nodes: &[super::NodeState],
        worlds: &std::collections::BTreeMap<crate::ecs::EntityId, Mat4>,
    ) -> Self {
        let mut view = Self::default();
        let mut camera = false;
        let mut light = false;
        for node in nodes.iter().filter(|n| n.visible) {
            if let Some(spatial) = &node.spatial {
                let world = worlds[&node.id];
                match spatial.role {
                    Role::Camera if !camera => {
                        view.view = world.inverse();
                        view.fov = spatial.fov;
                        camera = true;
                    }
                    Role::DirectionalLight if !light => {
                        view.light = world.transform_vector3(Vec3::Z).normalize();
                        light = true;
                    }
                    _ => {}
                }
            }
        }
        view
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn triangle(uvs: Option<Vec<[f32; 2]>>) -> Geometry {
        Geometry {
            positions: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
            indices: vec![0, 1, 2],
            uvs,
        }
    }

    #[test]
    fn geometry_accepts_matching_texture_coordinates_and_rejects_broken_ones() {
        assert!(triangle(None).validate().is_ok());
        assert!(
            triangle(Some(vec![[0., 0.], [1., 0.], [0., 1.]]))
                .validate()
                .is_ok()
        );
        assert!(triangle(Some(vec![[0., 0.]])).validate().is_err());
        assert!(
            triangle(Some(vec![[0., 0.], [f32::NAN, 0.], [0., 1.]]))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn skin_baking_applies_inverse_bind_and_rest_joint_transforms() -> Result<()> {
        let positions = [[1., 2., 3.]];
        let joints = [[0, 0, 0, 0]];
        let weights = [[1., 0., 0., 0.]];
        let world = [Mat4::from_translation(Vec3::new(5., 0., 0.))];
        let inverse = [Mat4::from_translation(Vec3::new(-5., 0., 0.))];
        assert_eq!(
            bake_skin(&positions, &joints, &weights, &inverse, &world)?,
            vec![[1., 2., 3.]]
        );
        assert_eq!(
            bake_skin(&positions, &joints, &weights, &[Mat4::IDENTITY], &world)?,
            vec![[6., 2., 3.]]
        );
        let two_joints = [[0, 1, 0, 0]];
        let two_weights = [[0.25, 0.75, 0., 0.]];
        let two_world = [world[0], Mat4::IDENTITY];
        let two_inverse = [Mat4::IDENTITY, Mat4::IDENTITY];
        assert_eq!(
            bake_skin(
                &positions,
                &two_joints,
                &two_weights,
                &two_inverse,
                &two_world
            )?,
            vec![[2.25, 2., 3.]]
        );
        assert!(bake_skin(&positions, &[[9, 0, 0, 0]], &weights, &inverse, &world).is_err());
        assert!(bake_skin(&positions, &joints, &[[0., 0., 0., 0.]], &inverse, &world).is_err());
        Ok(())
    }
}
