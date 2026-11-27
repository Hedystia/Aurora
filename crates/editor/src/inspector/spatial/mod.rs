//! Spatial node presets and local component editing.
use egui::{DragValue, Ui};
use engine_core::scene::spatial::{Body, Collider, FaceUv, Role, SpatialNode};

pub(crate) const PRESETS: [(&str, Role); 4] = [
    ("Node 3D", Role::Pivot),
    ("Cube 3D", Role::Cube),
    ("Camera 3D", Role::Camera),
    ("Directional Light", Role::DirectionalLight),
];

pub(crate) fn preset(name: &str) -> Option<SpatialNode> {
    let (_, role) = PRESETS.iter().find(|(label, _)| *label == name)?;
    let mut node = SpatialNode {
        role: *role,
        ..Default::default()
    };
    if *role == Role::Camera {
        node.position = [0., 1., 6.];
    }
    if *role == Role::DirectionalLight {
        node.rotation = [-35., -25., 0.];
    }
    Some(node)
}

pub(super) fn show(ui: &mut Ui, node: &mut SpatialNode, root: &std::path::Path) -> bool {
    let mut changed = false;
    ui.collapsing("Spatial 3D", |ui| {
        for (label, values, speed) in [
            ("Position (m)", &mut node.position, 0.05),
            ("Rotation (°)", &mut node.rotation, 1.),
            ("Scale", &mut node.scale, 0.05),
        ] {
            ui.label(label);
            ui.horizontal_wrapped(|ui| {
                for (axis, value) in ["X", "Y", "Z"].into_iter().zip(values) {
                    let mut drag = DragValue::new(value)
                        .speed(speed)
                        .prefix(format!("{axis} "));
                    if label == "Scale" {
                        drag = drag.range(0.001..=1e5);
                    }
                    changed |= ui.add(drag).changed();
                }
            });
        }
        if matches!(node.role, Role::Cube | Role::Mesh) {
            ui.horizontal(|ui| {
                ui.label("Base color");
                changed |= ui.color_edit_button_rgb(&mut node.color).changed();
            });
            changed |= ui
                .add(egui::Slider::new(&mut node.metallic, 0.0..=1.0).text("Metallic"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(&mut node.roughness, 0.0..=1.0).text("Roughness"))
                .changed();
            if let Some(path) = super::asset_drop(ui, "Drop base texture", &["png"], root) {
                node.texture = Some(path);
                changed = true;
            }
            ui.horizontal(|ui| {
                if let Some(path) = &node.texture {
                    ui.label(path.display().to_string());
                    if ui.small_button("Clear").clicked() {
                        node.texture = None;
                        changed = true;
                    }
                }
            });
            ui.collapsing("Procedural noise", |ui| {
                let mut enabled = node.noise.is_some();
                changed |= ui.checkbox(&mut enabled, "Noise base color").changed();
                if enabled && node.noise.is_none() {
                    node.noise = Some(Default::default());
                    changed = true;
                }
                if !enabled && node.noise.is_some() {
                    node.noise = None;
                    changed = true;
                }
                if let Some(texture) = &mut node.noise {
                    changed |= super::design::noise_ui(ui, texture, root);
                }
            });
            if node.role == Role::Cube {
                ui.collapsing("Face UVs", |ui| {
                    let mut enabled = node.faces.is_some();
                    changed |= ui.checkbox(&mut enabled, "Per-face regions").changed();
                    if enabled && node.faces.is_none() {
                        node.faces = Some(FaceUv::default());
                        changed = true;
                    }
                    if !enabled && node.faces.is_some() {
                        node.faces = None;
                        changed = true;
                    }
                    if let Some(faces) = &mut node.faces {
                        for (face, region) in faces.regions.iter_mut().enumerate() {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(format!("Face {face}"));
                                for value in region {
                                    changed |= ui
                                        .add(DragValue::new(value).range(0.0..=1.0).speed(0.01))
                                        .changed();
                                }
                            });
                        }
                    }
                });
            }
        }
        if node.role == Role::Cube {
            egui::ComboBox::from_label("Physics")
                .selected_text(format!("{:?}", node.body))
                .show_ui(ui, |ui| {
                    for (body, label) in [
                        (Body::None, "None"),
                        (Body::Fixed, "Fixed"),
                        (Body::Kinematic, "Kinematic"),
                        (Body::Dynamic, "Dynamic"),
                    ] {
                        changed |= ui.selectable_value(&mut node.body, body, label).changed();
                    }
                });
            if node.body != Body::None {
                egui::ComboBox::from_label("Collider")
                    .selected_text(format!("{:?}", node.collider))
                    .show_ui(ui, |ui| {
                        for (shape, label) in [
                            (Collider::Cuboid, "Box"),
                            (Collider::Sphere, "Sphere"),
                            (Collider::Capsule, "Capsule (Y)"),
                            (Collider::ConvexHull, "Convex hull"),
                            (Collider::Mesh, "Triangle mesh (fixed)"),
                        ] {
                            changed |= ui
                                .selectable_value(&mut node.collider, shape, label)
                                .changed();
                        }
                    });
                changed |= ui.checkbox(&mut node.trigger, "Trigger").changed();
                changed |= ui
                    .add(egui::Slider::new(&mut node.friction, 0.0..=10.0).text("Friction"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut node.restitution, 0.0..=1.0).text("Restitution"))
                    .changed();
            }
        }
        if node.role == Role::Camera {
            changed |= ui
                .add(egui::Slider::new(&mut node.fov, 1.0..=178.0).text("Vertical FOV (°)"))
                .changed();
        }
    });
    changed
}
