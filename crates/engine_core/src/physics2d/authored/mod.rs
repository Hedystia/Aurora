//! Physics components used by editable scenes and prefab instances.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use rapier2d::prelude::*;
use serde::{Deserialize, Serialize};

use super::{Contact, Physics2D};
use crate::{
    ecs::EntityId,
    scene::{Affine2D, Scene},
};

const PIXELS_PER_METER: f32 = 32.0;

/// Motion authority of an authored body.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyMode {
    /// Immovable geometry.
    #[default]
    Fixed,
    /// Animated or script-driven geometry.
    Kinematic,
    /// Solver-driven motion under gravity.
    Dynamic,
}

/// Collider centered on the node origin, with dimensions in world pixels.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "shape", rename_all = "snake_case", deny_unknown_fields)]
pub enum Collider2D {
    /// Rectangle with full width and height.
    Box {
        /// Full dimensions.
        size: [f32; 2],
    },
    /// Circle with a positive radius.
    Circle {
        /// Radius in pixels.
        radius: f32,
    },
    /// Simple polygon, using clockwise or counterclockwise boundary order.
    Polygon {
        /// Consecutive vertices relative to the node origin.
        points: Vec<[f32; 2]>,
    },
}

impl Default for Collider2D {
    fn default() -> Self {
        Self::Box { size: [32.0; 2] }
    }
}

/// Editable 2D body and its collider.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Body2D {
    /// Fixed, kinematic, or dynamic motion.
    pub mode: BodyMode,
    /// Solid geometry or trigger volume.
    pub collider: Collider2D,
    /// Emit overlap notifications without collision response.
    pub trigger: bool,
}

impl Body2D {
    /// Checks authored dimensions before loading or saving a document.
    pub fn validate(&self) -> Result<()> {
        self.builder().map(drop)
    }

    fn builder(&self) -> Result<ColliderBuilder> {
        let positive = |n: f32| n.is_finite() && n > 0.0;
        let shape = match &self.collider {
            Collider2D::Circle { radius } => {
                ensure!(positive(*radius), "Circle radius must be positive");
                ColliderBuilder::ball(radius / PIXELS_PER_METER)
            }
            Collider2D::Box { size } => {
                ensure!(
                    size.iter().copied().all(positive),
                    "Box dimensions must be positive"
                );
                ColliderBuilder::cuboid(
                    size[0] / (2.0 * PIXELS_PER_METER),
                    size[1] / (2.0 * PIXELS_PER_METER),
                )
            }
            Collider2D::Polygon { points } => polygon(points)?,
        };
        Ok(shape
            .sensor(self.trigger)
            .active_collision_types(ActiveCollisionTypes::all()))
    }
}

fn polygon(points: &[[f32; 2]]) -> Result<ColliderBuilder> {
    use rapier2d::parry::{shape::TriMesh, utils::segments_intersection2d};
    let n = points.len();
    ensure!(
        (3..=128).contains(&n) && points.iter().flatten().all(|v| v.is_finite()),
        "Expected 3 to 128 finite polygon points"
    );
    let mut vertices: Vec<_> = points
        .iter()
        .map(|&p| Vector::from_array(p) / PIXELS_PER_METER)
        .collect();
    let mut twice_area = 0.0;
    for i in 0..n {
        let a = vertices[i];
        let b = vertices[(i + 1) % n];
        ensure!(a != b, "Polygon has a zero-length edge");
        twice_area += a.perp_dot(b);
        for j in (i + 2)..n {
            if (j + 1) % n == i {
                continue;
            }
            ensure!(
                segments_intersection2d(a, b, vertices[j], vertices[(j + 1) % n], f32::EPSILON)
                    .is_none(),
                "Polygon edges cross"
            );
        }
    }
    ensure!(
        twice_area.is_finite() && twice_area.abs() > f32::EPSILON,
        "Polygon has no finite area"
    );
    if twice_area < 0.0 {
        vertices.reverse();
    }
    if (0..n).all(|i| {
        let [a, b, c] = [vertices[i], vertices[(i + 1) % n], vertices[(i + 2) % n]];
        (b - a).perp_dot(c - b) >= 0.0
    }) {
        return ColliderBuilder::convex_hull(&vertices).context("Degenerate convex polygon");
    }
    let mesh = TriMesh::from_polygon(vertices).context("Cannot triangulate polygon boundary")?;
    Ok(ColliderBuilder::compound(
        mesh.triangles()
            .map(|t| (Pose::IDENTITY, SharedShape::triangle(t.a, t.b, t.c)))
            .collect(),
    ))
}

#[derive(Default)]
pub(crate) struct AuthoredPhysics {
    solver: Physics2D,
    // Last published world pose distinguishes an external scene edit from solver motion.
    published: BTreeMap<EntityId, Pose>,
    velocities: BTreeMap<EntityId, Vector>,
    pub(crate) contacts: Vec<Contact>,
}

impl AuthoredPhysics {
    pub(crate) fn set_velocity(
        &mut self,
        scene: &Scene,
        node: EntityId,
        velocity: [f32; 2],
    ) -> Result<()> {
        ensure!(
            scene
                .body2d(node)?
                .is_some_and(|body| body.mode == BodyMode::Dynamic),
            "Linear velocity requires a dynamic 2D body"
        );
        let velocity = Vector::from_array(velocity);
        ensure!(velocity.is_finite(), "Linear velocity must be finite");
        self.velocities.insert(node, velocity);
        Ok(())
    }

    pub(crate) fn overlaps(&self, target: EntityId) -> impl Iterator<Item = EntityId> + '_ {
        self.solver.touching.iter().filter_map(move |&[a, b]| {
            if target == a {
                Some(b)
            } else if target == b {
                Some(a)
            } else {
                None
            }
        })
    }

    pub(crate) fn step(&mut self, scene: &mut Scene) -> Result<()> {
        self.contacts.clear();
        let velocities = std::mem::take(&mut self.velocities);
        if self.published.is_empty() && !scene.has_bodies2d() {
            return Ok(());
        }
        let states = scene.snapshot()?;
        let mut active = BTreeSet::new();
        let mut angles = BTreeMap::new();
        for node in &states {
            let angle = node.local.rotation + node.parent.map_or(0.0, |id| angles[&id]);
            angles.insert(node.id, angle);
            let Some(def) = scene.body2d(node.id)? else {
                continue;
            };
            if !node.visible {
                continue;
            }
            active.insert(node.id);
            let pose = Pose::new(
                Vector::from_array(node.world.point([0.0; 2])) / PIXELS_PER_METER,
                angle,
            );
            if let Some(body) = self.solver.body_mut(node.id) {
                if self.published.get(&node.id) != Some(&pose) {
                    if def.mode == BodyMode::Kinematic {
                        body.set_next_kinematic_position(pose);
                    } else {
                        body.set_position(pose, true);
                    }
                }
            } else {
                let builder = match def.mode {
                    BodyMode::Fixed => RigidBodyBuilder::fixed(),
                    BodyMode::Kinematic => RigidBodyBuilder::kinematic_position_based(),
                    BodyMode::Dynamic => RigidBodyBuilder::dynamic(),
                };
                self.solver
                    .insert(node.id, builder.pose(pose), def.builder()?)?;
            }
        }
        self.published.retain(|id, _| {
            if active.contains(id) {
                true
            } else {
                self.solver.remove(*id);
                false
            }
        });
        for (node, velocity) in velocities {
            if let Some(body) = self.solver.body_mut(node) {
                body.set_linvel(velocity, true);
            }
        }
        self.contacts = self.solver.step();
        let mut hierarchy = BTreeMap::new();
        for node in states {
            let (parent, angle) = node
                .parent
                .map_or((Affine2D::IDENTITY, 0.0), |id| hierarchy[&id]);
            let mut local = node.local;
            if let Some(body) = self.solver.body(node.id)
                && body.is_dynamic()
            {
                let point = (body.translation() * PIXELS_PER_METER).to_array();
                local.position = parent
                    .inverse_point(point)
                    .context("A dynamic 2D body requires an invertible parent transform")?;
                local.rotation = body.rotation().angle() - angle;
                scene.set_transform(node.id, local)?;
            }
            let world = parent.compose(local);
            let angle = angle + local.rotation;
            hierarchy.insert(node.id, (world, angle));
            if active.contains(&node.id) {
                // Static and kinematic descendants must follow simulated parents next tick.
                let pose = if self.solver.body(node.id).is_some_and(RigidBody::is_dynamic) {
                    Pose::new(
                        Vector::from_array(world.point([0.0; 2])) / PIXELS_PER_METER,
                        angle,
                    )
                } else {
                    Pose::new(
                        Vector::from_array(node.world.point([0.0; 2])) / PIXELS_PER_METER,
                        angles[&node.id],
                    )
                };
                self.published.insert(node.id, pose);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        director::Director,
        save::{PrefabFile, SceneFile},
        scene::Transform2D,
    };

    #[test]
    fn polygon_preserves_concave_boundary_and_rejects_crossing_edges() -> Result<()> {
        let mut points = vec![
            [0., 0.],
            [64., 0.],
            [64., 32.],
            [32., 32.],
            [32., 64.],
            [0., 64.],
        ];
        for _ in 0..2 {
            let collider = polygon(&points)?.build();
            assert!(collider.shape().contains_local_point(Vector::new(0.5, 1.5)));
            assert!(!collider.shape().contains_local_point(Vector::new(1.5, 1.5)));
            points.reverse();
        }
        assert!(polygon(&[[0., 0.], [32., 32.], [0., 32.], [32., 0.]]).is_err());
        assert!(polygon(&[[0., 0.], [1., 0.], [2., 0.]]).is_err());
        Ok(())
    }

    #[test]
    fn editable_prefab_drops_on_floor_and_reports_enter_and_removal() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let file = scratch.path().join("drop.scene");
        let mut scene = Scene::default();
        let (stage, _) = scene.spawn(
            "Stage",
            None,
            Transform2D {
                position: [15.0, -10.0],
                ..Default::default()
            },
        )?;
        let (floor, _) = scene.spawn("Floor", Some(stage), Transform2D::default())?;
        scene.set_body2d(
            floor,
            Some(Body2D {
                collider: Collider2D::Polygon {
                    points: vec![[-60., -8.], [60., -8.], [60., 0.], [-60., 0.]],
                },
                ..Default::default()
            }),
        )?;
        let (actor, _) = scene.spawn(
            "Ball",
            Some(stage),
            Transform2D {
                position: [0., 80.],
                ..Default::default()
            },
        )?;
        scene.set_body2d(
            actor,
            Some(Body2D {
                mode: BodyMode::Dynamic,
                collider: Collider2D::Circle { radius: 8. },
                trigger: false,
            }),
        )?;
        PrefabFile::capture(&scene, actor, |_| None)?.save(&scratch.path().join("ball.prefab"))?;
        scene.remove(actor)?;
        let mut document = SceneFile::capture(&scene, |_| None)?;
        document.instances.push(crate::save::InstanceFile {
            prefab: "ball.prefab".into(),
            parent: Some(0),
        });
        document.save(&file)?;
        let mut director = Director::new(vec![file], None)?;
        let ball = director.node(2).context("missing instantiated ball")?;
        let floor = director.node(1).context("missing floor")?;
        let mut events = Vec::new();
        for _ in 0..180 {
            director.step()?;
            events.extend_from_slice(director.physics_contacts());
        }
        assert_eq!(
            events,
            [Contact {
                nodes: [floor, ball],
                entered: true
            }]
        );
        assert!((director.scene().transform(ball)?.position[1] - 8.).abs() < 0.5);
        director.pause();
        let settled = director.scene().transform(ball)?;
        director.step()?;
        assert_eq!(director.scene().transform(ball)?, settled);
        director.resume();
        assert!(director.set_linear_velocity(floor, [0., 1.]).is_err());
        director.set_linear_velocity(ball, [0., 3.])?;
        director.scene_mut().remove(floor)?;
        director.step()?;
        assert!(director.scene().transform(ball)?.position[1] > settled.position[1]);
        assert_eq!(
            director.physics_contacts(),
            [Contact {
                nodes: [floor, ball],
                entered: false
            }]
        );
        Ok(())
    }

    #[test]
    fn kinematic_trigger_exits_when_animated_away() -> Result<()> {
        let mut scene = Scene::default();
        let (floor, _) = scene.spawn("Floor", None, Default::default())?;
        let (trigger, _) = scene.spawn("Trigger", None, Default::default())?;
        scene.set_body2d(floor, Some(Body2D::default()))?;
        scene.set_body2d(
            trigger,
            Some(Body2D {
                mode: BodyMode::Kinematic,
                trigger: true,
                ..Default::default()
            }),
        )?;
        let mut physics = AuthoredPhysics::default();
        physics.step(&mut scene)?;
        assert_eq!(
            physics.contacts,
            [Contact {
                nodes: [floor, trigger],
                entered: true
            }]
        );
        scene.set_transform(
            trigger,
            Transform2D {
                position: [128., 0.],
                ..Default::default()
            },
        )?;
        physics.step(&mut scene)?;
        // Position-based kinematics may complete narrow-phase reconciliation on the next tick.
        if physics.contacts.is_empty() {
            physics.step(&mut scene)?;
        }
        assert_eq!(
            physics.contacts,
            [Contact {
                nodes: [floor, trigger],
                entered: false
            }]
        );
        Ok(())
    }
}
