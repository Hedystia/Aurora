//! Default six-area dock layout and persistent tab identifiers.

use egui::{Ui, WidgetText};
use egui_dock::{DockState, NodeIndex, TabViewer};
use serde::{Deserialize, Serialize};

use crate::{
    Editor, assets_browser, console, inspector, node_library, node_tree, strings, theme, timeline,
    viewport,
};

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Dock {
    Scene,
    NodeTree,
    Assets,
    Library,
    Inspector,
    Console,
    Timeline,
}

impl Dock {
    pub const ALL: [Self; 7] = [
        Self::Scene,
        Self::NodeTree,
        Self::Assets,
        Self::Library,
        Self::Inspector,
        Self::Console,
        Self::Timeline,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Scene => strings::SCENE,
            Self::NodeTree => strings::NODE_TREE,
            Self::Assets => strings::ASSETS,
            Self::Library => strings::LIBRARY,
            Self::Inspector => strings::INSPECTOR,
            Self::Console => strings::CONSOLE,
            Self::Timeline => strings::TIMELINE,
        }
    }
}

pub(super) fn initial_layout() -> DockState<Dock> {
    let mut state = DockState::new(vec![Dock::Scene]);
    let tree = state.main_surface_mut();
    let [workspace, left] =
        tree.split_left(NodeIndex::root(), theme::LEFT_WIDTH, vec![Dock::NodeTree]);
    let [center, right] = tree.split_right(workspace, theme::CENTER_WIDTH, vec![Dock::Library]);
    tree.split_below(left, theme::TOP_HEIGHT, vec![Dock::Assets]);
    tree.split_below(
        center,
        theme::TOP_HEIGHT,
        vec![Dock::Console, Dock::Timeline],
    );
    tree.split_below(right, theme::LIBRARY_HEIGHT, vec![Dock::Inspector]);
    state
}

pub(super) struct Viewer<'a>(pub &'a mut Editor);

impl TabViewer for Viewer<'_> {
    type Tab = Dock;

    fn title(&mut self, tab: &mut Dock) -> WidgetText {
        tab.label().into()
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Dock) {
        match tab {
            Dock::Scene => viewport::show(ui, self.0),
            Dock::NodeTree => node_tree::show(ui, self.0),
            Dock::Assets => assets_browser::show(ui, self.0),
            Dock::Library => node_library::show(ui, self.0),
            Dock::Inspector => inspector::show(ui, self.0),
            Dock::Console => console::show(ui, self.0),
            Dock::Timeline => timeline::show(ui, self.0),
        }
    }

    fn is_closeable(&self, _: &Dock) -> bool {
        false
    }
}
