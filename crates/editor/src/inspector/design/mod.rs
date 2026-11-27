//! Figma-style Inspector sections for design elements.

use egui::{DragValue, Ui};
use engine_core::{
    save::{
        NodeFile,
        visual::{
            Visual,
            design::{Constraint, DesignKind, Fill, TextAlign},
        },
    },
    scene::SpriteAppearance,
};

use crate::strings;

fn section(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui) -> bool) -> bool {
    let mut changed = false;
    ui.collapsing(title, |ui| {
        changed = add(ui);
    });
    changed
}

fn field(ui: &mut Ui, value: &mut f32, prefix: &str, range: std::ops::RangeInclusive<f32>) -> bool {
    ui.add(DragValue::new(value).prefix(prefix).range(range).speed(0.5))
        .changed()
}

fn color(ui: &mut Ui, label: &str, value: &mut [u8; 4]) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.color_edit_button_srgba_unmultiplied(value).changed()
    })
    .inner
}

fn constraint(ui: &mut Ui, label: &str, value: &mut Constraint) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(label)
        .selected_text(format!("{label}: {value:?}"))
        .show_ui(ui, |ui| {
            for choice in [
                Constraint::Start,
                Constraint::End,
                Constraint::Center,
                Constraint::Both,
                Constraint::Scale,
            ] {
                changed |= ui
                    .selectable_value(value, choice, format!("{choice:?}"))
                    .changed();
            }
        });
    changed
}

/// Draws the Figma-like Design panel for the selected node.
pub(super) fn show(
    ui: &mut Ui,
    node: &mut NodeFile,
    appearance: &mut SpriteAppearance,
    root: &std::path::Path,
) -> bool {
    let (design, transform) = match (&mut node.visual, &mut node.transform) {
        (Some(Visual::Design(design)), transform) => (design, transform),
        _ => return false,
    };
    let mut changed = false;
    let kind = design.kind;

    changed |= section(ui, strings::POSITION_AND_SIZE, |ui| {
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            changed |= field(ui, &mut transform.position[0], "X ", -4096.0..=4096.0);
            changed |= field(ui, &mut transform.position[1], "Y ", -4096.0..=4096.0);
        });
        ui.horizontal_wrapped(|ui| {
            changed |= field(ui, &mut design.size[0], "W ", 1.0..=4096.0);
            changed |= field(ui, &mut design.size[1], "H ", 1.0..=4096.0);
        });
        ui.horizontal_wrapped(|ui| {
            changed |= field(
                ui,
                &mut transform.rotation,
                strings::ROTATION,
                -360.0..=360.0,
            );
            if matches!(kind, DesignKind::Rectangle | DesignKind::Frame) {
                changed |= field(ui, &mut design.corner, "R ", 0.0..=2048.0);
            }
        });
        changed
    });

    changed |= section(ui, strings::CONSTRAINTS, |ui| {
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            changed |= constraint(ui, "H", &mut design.constraints[0]);
            changed |= constraint(ui, "V", &mut design.constraints[1]);
        });
        changed
    });

    if design.kind.is_container() {
        changed |= section(ui, strings::AUTO_LAYOUT, |ui| {
            let mut changed = false;
            changed |= ui.checkbox(&mut design.layout.enabled, "Enabled").changed();
            if design.layout.enabled {
                changed |= ui
                    .checkbox(&mut design.layout.horizontal, "Horizontal")
                    .changed();
                ui.horizontal_wrapped(|ui| {
                    changed |= field(ui, &mut design.layout.gap, "Gap ", 0.0..=4096.0);
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Padding L T R B");
                    for value in &mut design.layout.padding {
                        changed |= field(ui, value, "", -512.0..=512.0);
                    }
                });
                changed |= field(ui, &mut design.layout.align, "Align ", 0.0..=1.0);
                changed |= ui
                    .checkbox(&mut design.clips_content, "Clip content")
                    .changed();
            }
            changed
        });
    }

    if kind == DesignKind::Text {
        changed |= section(ui, strings::TEXT, |ui| {
            let mut changed = ui.text_edit_singleline(&mut design.text).changed();
            changed |= ui
                .add(
                    DragValue::new(&mut design.font_size)
                        .range(1..=256)
                        .prefix("Size "),
                )
                .changed();
            egui::ComboBox::from_id_salt("design-text-align")
                .selected_text(format!("{:?}", design.text_align))
                .show_ui(ui, |ui| {
                    for (choice, label) in [
                        (TextAlign::Start, "Start"),
                        (TextAlign::Center, "Center"),
                        (TextAlign::End, "End"),
                    ] {
                        changed |= ui
                            .selectable_value(&mut design.text_align, choice, label)
                            .changed();
                    }
                });
            changed |= color(ui, "Color", &mut design.text_color);
            changed
        });
    }

    if matches!(kind, DesignKind::Image) {
        changed |= section(ui, strings::IMAGE, |ui| {
            let mut changed = false;
            if let Some(path) = super::asset_drop(ui, strings::DROP_PNG, &["png"], root) {
                design.png = Some(path);
                changed = true;
            }
            if design.png.is_some() && ui.small_button(strings::CLEAR).clicked() {
                design.png = None;
                changed = true;
            }
            changed
        });
    }

    changed |= section(ui, strings::FILL, |ui| {
        let mut changed = false;
        let mut gradient = matches!(design.fill, Fill::Linear { .. });
        changed |= ui.checkbox(&mut gradient, "Gradient").changed();
        match &mut design.fill {
            Fill::Solid(value) => changed |= color(ui, "Color", value),
            Fill::Linear { start, end, angle } => {
                changed |= color(ui, "From", start) | color(ui, "To", end);
                changed |= field(ui, angle, "Angle ", -360.0..=360.0);
            }
        }
        if gradient && matches!(design.fill, Fill::Solid(_)) {
            let start = match design.fill {
                Fill::Solid(value) => value,
                Fill::Linear { start, .. } => start,
            };
            design.fill = Fill::Linear {
                start,
                end: [255, 255, 255, 255],
                angle: 0.0,
            };
            changed = true;
        }
        if !gradient && matches!(design.fill, Fill::Linear { .. }) {
            let start = match design.fill {
                Fill::Solid(value) => value,
                Fill::Linear { start, .. } => start,
            };
            design.fill = Fill::Solid(start);
            changed = true;
        }
        changed
    });

    changed |= section(ui, strings::STROKE, |ui| {
        let mut enabled = design.stroke.color[3] > 0 || design.stroke.width > 0.0;
        let mut changed = ui.checkbox(&mut enabled, "Enabled").changed();
        if enabled {
            if design.stroke.color[3] == 0 {
                design.stroke.color = [255, 255, 255, 255];
                changed = true;
            }
            if design.stroke.width <= 0.0 {
                design.stroke.width = 1.0;
                changed = true;
            }
            changed |= color(ui, "Color", &mut design.stroke.color);
            changed |= field(ui, &mut design.stroke.width, "Width ", 0.0..=256.0);
        } else if design.stroke.color[3] > 0 || design.stroke.width > 0.0 {
            design.stroke = Default::default();
            changed = true;
        }
        changed
    });

    changed |= section(ui, strings::EFFECTS, |ui| {
        let mut changed = false;
        let mut shadow = design.shadow.is_some();
        changed |= ui.checkbox(&mut shadow, "Drop shadow").changed();
        if shadow && design.shadow.is_none() {
            design.shadow = Some(Default::default());
            changed = true;
        }
        if !shadow && design.shadow.is_some() {
            design.shadow = None;
            changed = true;
        }
        if let Some(effect) = &mut design.shadow {
            changed |= color(ui, "Color", &mut effect.color);
            ui.horizontal_wrapped(|ui| {
                for value in &mut effect.offset {
                    changed |= field(ui, value, "", -512.0..=512.0);
                }
            });
            changed |= field(ui, &mut effect.blur, "Blur ", 0.0..=256.0);
        }
        changed |= field(ui, &mut design.blur, "Layer blur ", 0.0..=256.0);
        changed
    });

    changed |= section(ui, strings::APPEARANCE, |ui| {
        let mut changed = ui
            .add(
                DragValue::new(&mut appearance.opacity)
                    .range(0..=255)
                    .prefix("Opacity "),
            )
            .changed();
        changed |= ui
            .add(DragValue::new(&mut appearance.layer).prefix("Layer "))
            .changed();
        changed
    });

    changed
}
