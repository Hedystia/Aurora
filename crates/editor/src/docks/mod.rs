//! Startup layout and tab view for the editor.

use egui::{Ui, WidgetText};
use egui_dock::{DockState, NodeIndex, TabViewer};

use crate::{strings, theme};

#[derive(Clone, Copy)]
pub(super) enum Dock {
    Scene,
    NodeTree,
    Assets,
    Library,
    Inspector,
    Bottom,
}

pub(super) fn initial_layout() -> DockState<Dock> {
    let mut state = DockState::new(vec![Dock::Scene]);
    let tree = state.main_surface_mut();
    let [workspace, left] =
        tree.split_left(NodeIndex::root(), theme::LEFT_WIDTH, vec![Dock::NodeTree]);
    let [center, right] = tree.split_right(workspace, theme::CENTER_WIDTH, vec![Dock::Library]);
    tree.split_below(left, theme::TOP_HEIGHT, vec![Dock::Assets]);
    tree.split_below(center, theme::TOP_HEIGHT, vec![Dock::Bottom]);
    tree.split_below(right, theme::LIBRARY_HEIGHT, vec![Dock::Inspector]);
    state
}

pub(super) struct Viewer;

impl TabViewer for Viewer {
    type Tab = Dock;

    fn title(&mut self, tab: &mut Dock) -> WidgetText {
        match tab {
            Dock::Scene => strings::SCENE,
            Dock::NodeTree => strings::NODE_TREE,
            Dock::Assets => strings::ASSETS,
            Dock::Library => strings::LIBRARY,
            Dock::Inspector => strings::INSPECTOR,
            Dock::Bottom => strings::BOTTOM,
        }
        .into()
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Dock) {
        if matches!(tab, Dock::Scene) {
            let rect = ui.available_rect_before_wrap();
            ui.painter().rect_filled(rect, 0.0, theme::VIEWPORT);
        }
    }

    fn is_closeable(&self, _: &Dock) -> bool {
        false
    }
}
