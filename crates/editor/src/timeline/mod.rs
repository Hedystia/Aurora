//! Clip editing and deterministic timeline preview.

use egui::{DragValue, Ui};
use engine_core::{
    animation::{Clip, Interpolation, Key, Property, Track, Wrap},
    scene::{Transform2D, spatial::SpatialNode},
};

use crate::{Editor, strings, theme};

#[derive(Default)]
pub(super) struct Timeline {
    index: usize,
    cursor: f64,
    playing: bool,
    preview: bool,
}

impl Timeline {
    pub fn cursor_for_preview(&self) -> Option<(usize, f64)> {
        self.preview.then_some((self.index, self.cursor))
    }
}

pub(super) fn show(ui: &mut Ui, editor: &mut Editor) {
    if editor.project.prefab {
        ui.label(strings::PREFAB_ANIMATION);
        return;
    }
    let timeline = &mut editor.timeline;
    let selected = editor.project.selected;
    let document = &mut editor.project.document;
    let mut changed = false;
    if document.animations.is_empty() {
        ui.label(strings::ANIMATION_EMPTY);
        if ui.button(strings::ADD_ANIMATION).clicked() {
            document.animations.push(Clip {
                name: strings::DEFAULT_CLIP.into(),
                sample_rate: 60,
                duration: 60,
                speed: 1.0,
                wrap: Wrap::Once,
                tracks: Vec::new(),
                events: Vec::new(),
            });
            document.autoplay = Some(0);
            editor.project.changed();
        }
        return;
    }
    timeline.index = timeline.index.min(document.animations.len() - 1);
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("clip")
            .selected_text(&document.animations[timeline.index].name)
            .show_ui(ui, |ui| {
                for (i, clip) in document.animations.iter().enumerate() {
                    if ui
                        .selectable_value(&mut timeline.index, i, &clip.name)
                        .clicked()
                    {
                        timeline.cursor = 0.0;
                    }
                }
            });
        if ui
            .small_button(strings::ADD)
            .on_hover_text(strings::ADD_ANIMATION)
            .clicked()
        {
            let mut clip = document.animations[timeline.index].clone();
            clip.name.push_str(strings::COPY_SUFFIX);
            clip.tracks.clear();
            clip.events.clear();
            timeline.index = document.animations.len();
            timeline.cursor = 0.0;
            document.animations.push(clip);
            changed = true;
        }
        changed |= ui
            .text_edit_singleline(&mut document.animations[timeline.index].name)
            .changed();
        let mut autoplay = document.autoplay == Some(timeline.index);
        if ui.checkbox(&mut autoplay, strings::AUTOPLAY).changed() {
            document.autoplay = autoplay.then_some(timeline.index);
            changed = true;
        }
    });
    let clip = &mut document.animations[timeline.index];
    let duration_min = clip
        .tracks
        .iter()
        .flat_map(|track| &track.keys)
        .map(|key| key.frame)
        .chain(clip.events.iter().map(|event| event.frame))
        .max()
        .unwrap_or(1)
        .max(1);
    ui.horizontal_wrapped(|ui| {
        changed |= ui
            .add(
                DragValue::new(&mut clip.sample_rate)
                    .range(1..=240)
                    .prefix(strings::SAMPLE),
            )
            .changed();
        changed |= ui
            .add(
                DragValue::new(&mut clip.speed)
                    .range(0.01..=16.0)
                    .speed(0.05)
                    .prefix(strings::SPEED),
            )
            .changed();
        changed |= ui
            .add(
                DragValue::new(&mut clip.duration)
                    .range(duration_min..=u32::MAX)
                    .prefix(strings::DURATION),
            )
            .changed();
        let before = wrap_label(clip.wrap);
        egui::ComboBox::from_id_salt("wrap")
            .selected_text(before)
            .show_ui(ui, |ui| {
                for wrap in [Wrap::Once, Wrap::Loop, Wrap::PingPong] {
                    if ui
                        .selectable_label(before == wrap_label(wrap), wrap_label(wrap))
                        .clicked()
                    {
                        clip.wrap = wrap;
                        changed = true;
                    }
                }
            });
        ui.checkbox(&mut timeline.preview, strings::REALTIME);
    });
    if timeline.playing {
        let ticks =
            f64::from(ui.input(|input| input.stable_dt)) * f64::from(clip.sample_rate) * clip.speed;
        timeline.cursor += ticks;
        if timeline.cursor >= f64::from(clip.duration) {
            match clip.wrap {
                Wrap::Once => {
                    timeline.playing = false;
                    timeline.cursor = f64::from(clip.duration);
                }
                Wrap::Loop | Wrap::PingPong => timeline.cursor %= 2.0 * f64::from(clip.duration),
            }
        }
        timeline.preview = true;
        ui.ctx().request_repaint();
    }
    let duration = f64::from(clip.duration);
    let display = match clip.wrap {
        Wrap::Once => timeline.cursor.min(duration),
        Wrap::Loop => timeline.cursor.rem_euclid(duration),
        Wrap::PingPong => duration - (timeline.cursor.rem_euclid(2.0 * duration) - duration).abs(),
    };
    let mut at = if timeline.playing {
        display
    } else {
        timeline.cursor.min(duration)
    };
    ui.horizontal_wrapped(|ui| {
        if ui.small_button(strings::FIRST_KEY).clicked() {
            timeline.cursor = 0.0;
            timeline.preview = true;
            timeline.playing = false;
        }
        if ui.small_button(strings::PREV_KEY).clicked() {
            timeline.cursor = clip
                .tracks
                .iter()
                .flat_map(|track| &track.keys)
                .map(|key| f64::from(key.frame))
                .filter(|frame| *frame < at)
                .max_by(f64::total_cmp)
                .unwrap_or(0.0);
            timeline.preview = true;
            timeline.playing = false;
        }
        if ui
            .small_button(if timeline.playing {
                strings::PAUSE
            } else {
                strings::PLAY
            })
            .clicked()
        {
            if timeline.cursor >= duration {
                timeline.cursor = 0.0;
            }
            timeline.playing = !timeline.playing;
            timeline.preview = true;
        }
        if ui.small_button(strings::NEXT_KEY).clicked() {
            timeline.cursor = clip
                .tracks
                .iter()
                .flat_map(|track| &track.keys)
                .map(|key| f64::from(key.frame))
                .filter(|frame| *frame > at)
                .min_by(f64::total_cmp)
                .unwrap_or(duration);
            timeline.preview = true;
            timeline.playing = false;
        }
        if ui.small_button(strings::LAST_KEY).clicked() {
            timeline.cursor = duration;
            timeline.preview = true;
            timeline.playing = false;
        }
        if ui
            .add(
                egui::Slider::new(&mut at, 0.0..=duration)
                    .fixed_decimals(2)
                    .suffix(strings::FRAME_SUFFIX),
            )
            .changed()
        {
            timeline.cursor = at;
            timeline.preview = true;
            timeline.playing = false;
        }
    });
    if timeline.playing {
        editor.animation_display = Some((timeline.index, display));
    } else {
        editor.animation_display = None;
    }
    if let Some(node) = selected {
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("property")
                .selected_text(property_label(editor.key_property))
                .show_ui(ui, |ui| {
                    let available: &[Property] = if document.nodes[node].spatial.is_some() {
                        &[
                            Property::X,
                            Property::Y,
                            Property::Rotation,
                            Property::ScaleX,
                            Property::ScaleY,
                            Property::TranslationX,
                            Property::TranslationY,
                            Property::TranslationZ,
                            Property::RotationX,
                            Property::RotationY,
                            Property::RotationZ,
                            Property::SpatialScaleX,
                            Property::SpatialScaleY,
                            Property::SpatialScaleZ,
                        ]
                    } else {
                        &[
                            Property::X,
                            Property::Y,
                            Property::Rotation,
                            Property::ScaleX,
                            Property::ScaleY,
                        ]
                    };
                    for &property in available {
                        ui.selectable_value(
                            &mut editor.key_property,
                            property,
                            property_label(property),
                        );
                    }
                });
            if ui.small_button(strings::ADD_KEY).clicked() {
                let value = channel(
                    document.nodes[node].transform,
                    document.nodes[node].spatial.as_ref(),
                    editor.key_property,
                );
                record(clip, node, editor.key_property, at.round() as u32, value);
                changed = true;
            }
        });
    }
    let ruler = ruler(ui, clip, at);
    if let Some(frame) = ruler {
        timeline.cursor = frame;
        timeline.preview = true;
        timeline.playing = false;
    }
    egui::ScrollArea::both().show(ui, |ui| {
        for (index, track) in clip.tracks.iter_mut().enumerate() {
            ui.push_id(index, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "{} · {}",
                        document
                            .nodes
                            .get(track.node)
                            .map_or(strings::PREFAB_INSTANCE, |node| &node.name),
                        property_label(track.property)
                    ));
                    let label = interpolation_label(track.interpolation);
                    egui::ComboBox::from_id_salt("interpolation")
                        .selected_text(label)
                        .show_ui(ui, |ui| {
                            for curve in [
                                Interpolation::Step,
                                Interpolation::Linear,
                                Interpolation::Cubic,
                            ] {
                                let option = interpolation_label(curve);
                                if ui.selectable_label(label == option, option).clicked() {
                                    track.interpolation = curve;
                                    changed = true;
                                }
                            }
                        });
                    let mut remove = None;
                    for (i, key) in track.keys.iter_mut().enumerate() {
                        ui.label(format!(
                            "{:.2}",
                            f64::from(key.frame) / f64::from(clip.sample_rate)
                        ));
                        changed |= ui.add(DragValue::new(&mut key.value)).changed();
                        if matches!(track.interpolation, Interpolation::Cubic) {
                            let mut slopes = key.tangents.unwrap_or_default();
                            let incoming = ui
                                .add(DragValue::new(&mut slopes[0]).speed(0.01).prefix("In "))
                                .on_hover_text("Incoming slope per frame");
                            let outgoing = ui
                                .add(DragValue::new(&mut slopes[1]).speed(0.01).prefix("Out "))
                                .on_hover_text("Outgoing slope per frame");
                            if incoming.changed() || outgoing.changed() {
                                key.tangents = (slopes != [0.0; 2]).then_some(slopes);
                                changed = true;
                            }
                        }
                        if ui
                            .small_button(strings::CLEAR)
                            .on_hover_text(strings::REMOVE_KEY)
                            .clicked()
                        {
                            remove = Some(i);
                        }
                    }
                    if let Some(i) = remove {
                        track.keys.remove(i);
                        changed = true;
                    }
                });
                ui.collapsing("Curve editor", |ui| {
                    changed |= curve_editor(ui, track, clip.duration);
                });
            });
        }
    });
    if changed {
        clip.tracks.retain(|track| !track.keys.is_empty());
        editor.project.changed();
    }
}

fn curve_editor(ui: &mut Ui, track: &mut Track, duration: u32) -> bool {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().clamp(180.0, 800.0), 100.0),
        egui::Sense::hover(),
    );
    let rect = rect.shrink(6.0);
    let painter = ui.painter_at(rect.expand(6.0));
    painter.rect_filled(rect.expand(6.0), theme::ROUND, theme::VIEWPORT);
    let minimum = track
        .keys
        .iter()
        .map(|key| key.value as f64)
        .fold(f64::INFINITY, f64::min);
    let maximum = track
        .keys
        .iter()
        .map(|key| key.value as f64)
        .fold(f64::NEG_INFINITY, f64::max);
    if track.keys.is_empty() {
        return false;
    }
    let padding = ((maximum - minimum) * 0.15).max(1.0);
    // Keep the coordinate system fixed while a handle is moving.
    let bounds_id = ui.id().with("drag-bounds");
    let (lo, hi) = ui
        .data(|data| data.get_temp::<(f64, f64)>(bounds_id))
        .unwrap_or((minimum - padding, maximum + padding));
    let point = |frame: f64, value: f32| {
        egui::pos2(
            rect.left() + rect.width() * (frame / f64::from(duration)) as f32,
            rect.bottom() - rect.height() * ((f64::from(value) - lo) / (hi - lo)) as f32,
        )
    };
    let mut previous = None;
    for pixel in 0..=rect.width().ceil() as u32 {
        let frame = (f64::from(pixel) / f64::from(rect.width())).min(1.0) * f64::from(duration);
        if let Some(value) = track.sample(frame) {
            let next = point(frame, value);
            if let Some(last) = previous {
                painter.line_segment([last, next], (1.5, theme::ANIMATION));
            }
            previous = Some(next);
        }
    }
    let mut changed = false;
    for i in 0..track.keys.len() {
        let key = &track.keys[i];
        let pos = point(f64::from(key.frame), key.value);
        let response = ui.interact(
            egui::Rect::from_center_size(pos, egui::vec2(12.0, 12.0)),
            ui.id().with(i),
            egui::Sense::drag(),
        );
        painter.circle_filled(
            pos,
            4.0,
            if response.dragged() {
                theme::ACCENT
            } else {
                theme::WARNING
            },
        );
        if response.drag_started() {
            ui.data_mut(|data| data.insert_temp(bounds_id, (lo, hi)));
        }
        if response.dragged()
            && let Some(pointer) = response.interact_pointer_pos()
        {
            let first = i
                .checked_sub(1)
                .map_or(0, |prev| track.keys[prev].frame + 1);
            let last = track
                .keys
                .get(i + 1)
                .map_or(duration, |next| next.frame - 1);
            let frame = f64::from((pointer.x - rect.left()) / rect.width()) * f64::from(duration);
            let value = lo + f64::from((rect.bottom() - pointer.y) / rect.height()) * (hi - lo);
            track.keys[i].frame = (frame.round() as u32).clamp(first, last);
            track.keys[i].value = value.clamp(f64::from(f32::MIN), f64::from(f32::MAX)) as f32;
            changed = true;
        }
        if response.drag_stopped() {
            ui.data_mut(|data| data.remove::<(f64, f64)>(bounds_id));
        }
    }
    changed
}

fn ruler(ui: &mut Ui, clip: &Clip, frame: f64) -> Option<f64> {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), theme::TIMELINE_RULER),
        egui::Sense::click_and_drag(),
    );
    let painter = ui.painter_at(rect);
    let duration = f64::from(clip.duration);
    let x = |frame: f64| rect.left() + rect.width() * (frame / duration) as f32;
    painter.rect_filled(rect, theme::ROUND, theme::VIEWPORT);
    for tick in 0..=5 {
        let sample = duration * f64::from(tick) / 5.0;
        let left = x(sample);
        painter.line_segment(
            [
                egui::pos2(left, rect.top()),
                egui::pos2(left, rect.bottom()),
            ],
            (theme::LINE, theme::GRID),
        );
        painter.text(
            egui::pos2(left, rect.top()),
            egui::Align2::LEFT_TOP,
            format!("{:.2}", sample / f64::from(clip.sample_rate)),
            theme::font(),
            theme::TEXT,
        );
    }
    for key in clip.tracks.iter().flat_map(|track| &track.keys) {
        let position = egui::pos2(x(f64::from(key.frame)), rect.bottom() - theme::PAD);
        painter.circle_filled(position, theme::KEY_RADIUS, theme::WARNING);
    }
    let cursor = x(frame);
    painter.line_segment(
        [
            egui::pos2(cursor, rect.top()),
            egui::pos2(cursor, rect.bottom()),
        ],
        (theme::OUTLINE, theme::ACCENT),
    );
    if (response.clicked() || response.dragged())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        return Some(
            f64::from(((pointer.x - rect.left()) / rect.width()).clamp(0.0, 1.0)) * duration,
        );
    }
    None
}

fn channel(pose: Transform2D, spatial: Option<&SpatialNode>, property: Property) -> f32 {
    use Property as P;
    match property {
        P::X => pose.position[0],
        P::Y => pose.position[1],
        P::Rotation => pose.rotation,
        P::ScaleX => pose.scale[0],
        P::ScaleY => pose.scale[1],
        P::TranslationX => spatial.map_or(0.0, |node| node.position[0]),
        P::TranslationY => spatial.map_or(0.0, |node| node.position[1]),
        P::TranslationZ => spatial.map_or(0.0, |node| node.position[2]),
        P::RotationX => spatial.map_or(0.0, |node| node.rotation[0]),
        P::RotationY => spatial.map_or(0.0, |node| node.rotation[1]),
        P::RotationZ => spatial.map_or(0.0, |node| node.rotation[2]),
        P::SpatialScaleX => spatial.map_or(1.0, |node| node.scale[0]),
        P::SpatialScaleY => spatial.map_or(1.0, |node| node.scale[1]),
        P::SpatialScaleZ => spatial.map_or(1.0, |node| node.scale[2]),
    }
}

fn record(clip: &mut Clip, node: usize, property: Property, frame: u32, value: f32) {
    let index = clip
        .tracks
        .iter()
        .position(|track| track.node == node && track.property == property)
        .unwrap_or_else(|| {
            clip.tracks.push(Track {
                node,
                property,
                interpolation: Interpolation::Linear,
                keys: Vec::new(),
            });
            clip.tracks.len() - 1
        });
    let keys = &mut clip.tracks[index].keys;
    match keys.binary_search_by_key(&frame, |key| key.frame) {
        Ok(index) => keys[index].value = value,
        Err(index) => keys.insert(
            index,
            Key {
                frame,
                value,
                tangents: None,
            },
        ),
    }
}

fn property_label(property: Property) -> &'static str {
    use Property as P;
    match property {
        P::X => strings::X,
        P::Y => strings::Y,
        P::Rotation => strings::ROTATION,
        P::ScaleX => strings::SCALE_X,
        P::ScaleY => strings::SCALE_Y,
        P::TranslationX => strings::POSITION_X_3D,
        P::TranslationY => strings::POSITION_Y_3D,
        P::TranslationZ => strings::POSITION_Z_3D,
        P::RotationX => strings::ROTATION_X,
        P::RotationY => strings::ROTATION_Y,
        P::RotationZ => strings::ROTATION_Z,
        P::SpatialScaleX => strings::SCALE_X_3D,
        P::SpatialScaleY => strings::SCALE_Y_3D,
        P::SpatialScaleZ => strings::SCALE_Z_3D,
    }
}

fn interpolation_label(curve: Interpolation) -> &'static str {
    match curve {
        Interpolation::Step => strings::STEP,
        Interpolation::Linear => strings::LINEAR,
        Interpolation::Cubic => strings::CUBIC,
    }
}

fn wrap_label(wrap: Wrap) -> &'static str {
    match wrap {
        Wrap::Once => strings::ONCE,
        Wrap::Loop => strings::LOOP,
        Wrap::PingPong => strings::PING_PONG,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_inserts_and_replaces_keys_in_sample_order() {
        let mut clip = Clip {
            name: "Test".into(),
            sample_rate: 60,
            duration: 60,
            speed: 1.0,
            wrap: Wrap::Loop,
            tracks: Vec::new(),
            events: Vec::new(),
        };
        record(&mut clip, 0, Property::X, 30, 3.0);
        record(&mut clip, 0, Property::X, 4, 1.0);
        record(&mut clip, 0, Property::X, 30, 5.0);
        assert!(clip.validate(1).is_ok());
        assert_eq!(
            clip.tracks[0]
                .keys
                .iter()
                .map(|key| (key.frame, key.value))
                .collect::<Vec<_>>(),
            [(4, 1.0), (30, 5.0)]
        );
    }
}
