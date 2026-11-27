//! Node palette presets and Inspector fields for authored visuals.

use egui::{DragValue, Ui};
use engine_core::save::visual::design::{Design, DesignKind};
use engine_core::save::visual::{Emitter, Span, Visual};
use std::path::{Path, PathBuf};

/// Figma-style design elements exposed in the Node Library.
pub(crate) const DESIGN_KINDS: [(&str, DesignKind); 7] = [
    ("Frame", DesignKind::Frame),
    ("Rectangle", DesignKind::Rectangle),
    ("Ellipse", DesignKind::Ellipse),
    ("Line", DesignKind::Line),
    ("Design Text", DesignKind::Text),
    ("Image", DesignKind::Image),
    ("Group", DesignKind::Group),
];

pub(crate) const NAMES: [&str; 14] = [
    "Text",
    "Rich Text",
    "Particles",
    "Tile Map",
    "Atlas Sprite",
    "Nine-Slice",
    "Frame",
    "Rectangle",
    "Ellipse",
    "Line",
    "Design Text",
    "Image",
    "Group",
    "Noise",
];

pub(crate) fn preset(name: &str) -> Option<Visual> {
    Some(match name {
        "Text" | "Rich Text" => {
            let mut spans = vec![Span {
                text: "Text".into(),
                size: 24,
                color: [255; 4],
            }];
            if name == "Rich Text" {
                spans.push(Span {
                    text: " sample".into(),
                    size: 32,
                    color: [109, 190, 244, 255],
                });
            }
            Visual::Text { font: None, spans }
        }
        "Particles" => Visual::Particles {
            emitter: Emitter {
                count: 32,
                lifetime: 90,
                speed: 55.,
                gravity: 32.,
                diameter: 4.,
            },
        },
        "Tile Map" => Visual::TileMap {
            png: None,
            cell: [32; 2],
            columns: 4,
            tiles: vec![Some(0), Some(1), Some(2), Some(3)],
        },
        "Atlas Sprite" => Visual::Atlas {
            png: None,
            cell: [64; 2],
            index: 0,
        },
        "Nine-Slice" => Visual::NineSlice {
            png: None,
            borders: [8; 4],
            size: [192, 96],
        },
        "Noise" => Visual::Noise(engine_core::save::noise::Noise::default()),
        name => {
            let (_, kind) = DESIGN_KINDS.iter().find(|(label, _)| *label == name)?;
            Visual::Design(Design {
                kind: *kind,
                ..Design::default()
            })
        }
    })
}

fn source(
    ui: &mut Ui,
    root: &Path,
    slot: &mut Option<PathBuf>,
    extensions: &[&str],
    label: &str,
) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        if let Some(path) = super::asset_drop(ui, label, extensions, root) {
            *slot = Some(path);
            changed = true;
        }
        if let Some(path) = slot {
            ui.label(path.display().to_string());
            if ui.small_button("Clear").clicked() {
                *slot = None;
                changed = true;
            }
        }
    });
    changed
}

fn ui_color(ui: &mut Ui, color: &mut [u8; 4]) -> bool {
    ui.color_edit_button_srgba_unmultiplied(color).changed()
}

fn values(ui: &mut Ui, label: &str, list: &mut [u32], min: u32) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.label(label);
        for value in list {
            changed |= ui.add(DragValue::new(value).range(min..=4096)).changed();
        }
    });
    changed
}

pub(crate) fn show(ui: &mut Ui, visual: &mut Option<Visual>, root: &Path) -> bool {
    let Some(v) = visual.as_mut() else {
        return false;
    };
    let mut changed = false;
    let mut remove = false;
    ui.group(|ui| {
        match v {
            Visual::Text { font, spans } => {
                ui.strong("Text");
                changed |= source(ui, root, font, &["ttf", "otf"], "Drop font");
                let mut remove_span = None;
                let removable = spans.len() > 1;
                for (i, span) in spans.iter_mut().enumerate() {
                    ui.push_id(i, |ui| {
                        changed |= ui.text_edit_multiline(&mut span.text).changed();
                        ui.horizontal(|ui| {
                            changed |= ui
                                .add(
                                    DragValue::new(&mut span.size)
                                        .prefix("Size ")
                                        .range(1..=256),
                                )
                                .changed();
                            changed |= ui
                                .color_edit_button_srgba_unmultiplied(&mut span.color)
                                .changed();
                            if removable && ui.small_button("Remove run").clicked() {
                                remove_span = Some(i);
                            }
                        });
                    });
                }
                if let Some(i) = remove_span {
                    spans.remove(i);
                    changed = true;
                }
                if spans.len() < 256 && ui.button("Add styled run").clicked() {
                    spans.push(Span {
                        text: "Text".into(),
                        size: 24,
                        color: [255; 4],
                    });
                    changed = true;
                }
            }
            Visual::Particles { emitter: e } => {
                ui.strong("Particle emitter");
                changed |= ui
                    .add(DragValue::new(&mut e.count).prefix("Count ").range(1..=256))
                    .changed();
                changed |= ui
                    .add(
                        DragValue::new(&mut e.lifetime)
                            .prefix("Lifetime (ticks) ")
                            .range(1..=65535),
                    )
                    .changed();
                changed |= ui
                    .add(
                        DragValue::new(&mut e.speed)
                            .prefix("Speed ")
                            .range(-100000.0..=100000.0),
                    )
                    .changed();
                changed |= ui
                    .add(
                        DragValue::new(&mut e.gravity)
                            .prefix("Gravity ")
                            .range(-100000.0..=100000.0),
                    )
                    .changed();
                changed |= ui
                    .add(
                        DragValue::new(&mut e.diameter)
                            .prefix("Diameter ")
                            .range(0.1..=4096.),
                    )
                    .changed();
            }
            Visual::Atlas { png, cell, index } => {
                ui.strong("Sprite atlas");
                changed |= source(ui, root, png, &["png"], "Drop sprite sheet");
                changed |= values(ui, "Cell size", cell, 1);
                changed |= ui.add(DragValue::new(index).prefix("Frame ")).changed();
            }
            Visual::NineSlice { png, borders, size } => {
                ui.strong("Nine-slice sprite");
                changed |= source(ui, root, png, &["png"], "Drop source PNG");
                changed |= values(ui, "Dimensions", size, 1);
                changed |= values(ui, "Borders (L T R B)", borders, 0);
            }
            Visual::TileMap {
                png,
                cell,
                columns,
                tiles,
            } => {
                ui.strong("Tile map");
                changed |= source(ui, root, png, &["png"], "Drop tileset");
                changed |= values(ui, "Tile dimensions", cell, 1);
                changed |= ui
                    .add(DragValue::new(columns).prefix("Columns ").range(1..=4096))
                    .changed();
                ui.label("Row-major cells (−1 is empty)");
                egui::ScrollArea::vertical()
                    .id_salt("tile-cells")
                    .max_height(160.)
                    .show_rows(ui, 20., tiles.len(), |ui, rows| {
                        for i in rows {
                            let mut index = tiles[i].map(i64::from).unwrap_or(-1);
                            if ui
                                .add(
                                    DragValue::new(&mut index)
                                        .prefix(format!("[{i}] "))
                                        .range(-1..=i64::from(u32::MAX)),
                                )
                                .changed()
                            {
                                tiles[i] = u32::try_from(index).ok();
                                changed = true;
                            }
                        }
                    });
                ui.horizontal(|ui| {
                    if tiles.len() < 65536 && ui.button("Add cell").clicked() {
                        tiles.push(Some(0));
                        changed = true;
                    }
                    if tiles.len() > 1 && ui.button("Remove last").clicked() {
                        tiles.pop();
                        changed = true;
                    }
                });
            }
            Visual::Noise(texture) => {
                ui.strong("Noise texture");
                changed |= super::design::noise_ui(ui, texture, root);
            }
            Visual::Design(element) => {
                ui.strong(format!("Design · {:?}", element.kind));
                ui.horizontal_wrapped(|ui| {
                    ui.label("Size W H");
                    for value in &mut element.size {
                        changed |= ui
                            .add(DragValue::new(value).range(1.0..=4096.0).speed(1.0))
                            .changed();
                    }
                });
                changed |= source(ui, root, &mut element.png, &["png"], "Drop image");
                if element.kind == DesignKind::Text {
                    changed |= ui.text_edit_singleline(&mut element.text).changed();
                    changed |= ui
                        .add(DragValue::new(&mut element.font_size).range(1..=256))
                        .changed();
                    egui::ComboBox::from_label("Align")
                        .selected_text(format!("{:?}", element.text_align))
                        .show_ui(ui, |ui| {
                            for (kind, label) in [
                                (engine_core::save::visual::design::TextAlign::Start, "Start"),
                                (
                                    engine_core::save::visual::design::TextAlign::Center,
                                    "Center",
                                ),
                                (engine_core::save::visual::design::TextAlign::End, "End"),
                            ] {
                                changed |= ui
                                    .selectable_value(&mut element.text_align, kind, label)
                                    .changed();
                            }
                        });
                    ui.horizontal(|ui| {
                        ui.label("Text color");
                        changed |= ui_color(ui, &mut element.text_color);
                    });
                }
                ui.collapsing("Fill", |ui| {
                    changed |= super::design::fill_ui(ui, &mut element.fill, root);
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Corner");
                    changed |= ui
                        .add(DragValue::new(&mut element.corner).range(0.0..=2048.0))
                        .changed();
                    ui.label("Blur");
                    changed |= ui
                        .add(DragValue::new(&mut element.blur).range(0.0..=256.0))
                        .changed();
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Stroke");
                    changed |= ui_color(ui, &mut element.stroke.color);
                    changed |= ui
                        .add(DragValue::new(&mut element.stroke.width).range(0.0..=256.0))
                        .changed();
                });
                ui.horizontal(|ui| {
                    let mut shadow = element.shadow.is_some();
                    changed |= ui.checkbox(&mut shadow, "Shadow").changed();
                    if shadow && element.shadow.is_none() {
                        element.shadow = Some(Default::default());
                        changed = true;
                    }
                    if !shadow && element.shadow.is_some() {
                        element.shadow = None;
                        changed = true;
                    }
                    if let Some(effect) = &mut element.shadow {
                        changed |= ui_color(ui, &mut effect.color);
                        for value in &mut effect.offset {
                            changed |= ui.add(DragValue::new(value).speed(1.0)).changed();
                        }
                        changed |= ui
                            .add(DragValue::new(&mut effect.blur).range(0.0..=256.0))
                            .changed();
                    }
                });
                ui.checkbox(&mut element.clips_content, "Clip content");
                ui.collapsing("Auto layout", |ui| {
                    changed |= ui
                        .checkbox(&mut element.layout.enabled, "Enabled")
                        .changed();
                    changed |= ui
                        .checkbox(&mut element.layout.horizontal, "Horizontal")
                        .changed();
                    changed |= ui
                        .add(
                            DragValue::new(&mut element.layout.gap)
                                .range(0.0..=4096.0)
                                .prefix("Gap "),
                        )
                        .changed();
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Padding");
                        for value in &mut element.layout.padding {
                            changed |= ui
                                .add(DragValue::new(value).range(-512.0..=512.0))
                                .changed();
                        }
                    });
                    changed |= ui
                        .add(
                            DragValue::new(&mut element.layout.align)
                                .range(0.0..=1.0)
                                .speed(0.05)
                                .prefix("Align "),
                        )
                        .changed();
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Constraints H V");
                    for constraint in &mut element.constraints {
                        let label = format!("{constraint:?}");
                        egui::ComboBox::from_id_salt(label.clone())
                            .selected_text(label)
                            .show_ui(ui, |ui| {
                                for choice in [
                                    engine_core::save::visual::design::Constraint::Start,
                                    engine_core::save::visual::design::Constraint::End,
                                    engine_core::save::visual::design::Constraint::Center,
                                    engine_core::save::visual::design::Constraint::Both,
                                    engine_core::save::visual::design::Constraint::Scale,
                                ] {
                                    changed |= ui
                                        .selectable_value(constraint, choice, format!("{choice:?}"))
                                        .changed();
                                }
                            });
                    }
                });
            }
        }
        remove = ui.small_button("Remove visual").clicked();
    });
    if remove {
        *visual = None;
    }
    changed || remove
}
