//! Clip editing and deterministic timeline preview.

use egui::{DragValue, Ui};
use engine_core::{
    animation::{Clip, Interpolation, Key, Property, Track, Wrap},
    scene::Transform2D,
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
                    for property in [
                        Property::X,
                        Property::Y,
                        Property::Rotation,
                        Property::ScaleX,
                        Property::ScaleY,
                    ] {
                        ui.selectable_value(
                            &mut editor.key_property,
                            property,
                            property_label(property),
                        );
                    }
                });
            if ui.small_button(strings::ADD_KEY).clicked() {
                let value = channel(document.nodes[node].transform, editor.key_property);
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
            });
        }
    });
    if changed {
        clip.tracks.retain(|track| !track.keys.is_empty());
        editor.project.changed();
    }
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

fn channel(pose: Transform2D, property: Property) -> f32 {
    match property {
        Property::X => pose.position[0],
        Property::Y => pose.position[1],
        Property::Rotation => pose.rotation,
        Property::ScaleX => pose.scale[0],
        Property::ScaleY => pose.scale[1],
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
        Err(index) => keys.insert(index, Key { frame, value }),
    }
}

fn property_label(property: Property) -> &'static str {
    match property {
        Property::X => strings::X,
        Property::Y => strings::Y,
        Property::Rotation => strings::ROTATION,
        Property::ScaleX => strings::SCALE_X,
        Property::ScaleY => strings::SCALE_Y,
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
