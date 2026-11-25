//! Spatial node presets and local component editing.
use egui::{DragValue, Ui};
use engine_core::scene::spatial::{Body, Role, SpatialNode};

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

pub(super) fn show(ui: &mut Ui, node: &mut SpatialNode) -> bool {
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
            if let Some(path) = &node.texture {
                ui.label(format!("Base texture: {}", path.display()));
            }
        }
        if node.role == Role::Cube {
            egui::ComboBox::from_label("Physics")
                .selected_text(format!("{:?}", node.body))
                .show_ui(ui, |ui| {
                    for (body, label) in [
                        (Body::None, "None"),
                        (Body::Fixed, "Fixed"),
                        (Body::Dynamic, "Dynamic"),
                    ] {
                        changed |= ui.selectable_value(&mut node.body, body, label).changed();
                    }
                });
        }
        if node.role == Role::Camera {
            changed |= ui
                .add(egui::Slider::new(&mut node.fov, 1.0..=178.0).text("Vertical FOV (°)"))
                .changed();
        }
    });
    changed
}
