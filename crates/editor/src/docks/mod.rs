//! Godot-inspired dock layout and persistent tab identifiers.

use egui::{Ui, WidgetText};
use egui_dock::{DockState, NodeIndex, TabViewer};
use serde::{Deserialize, Serialize};

use crate::{
    Editor, assets_browser, console, inspector, node_library, node_tree, strings, theme, timeline,
    viewport,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
            Self::Scene => strings::VIEWPORT,
            Self::NodeTree => strings::SCENE_TREE,
            Self::Assets => strings::FILE_SYSTEM,
            Self::Library => strings::LIBRARY,
            Self::Inspector => strings::INSPECTOR,
            Self::Console => strings::OUTPUT,
            Self::Timeline => strings::ANIMATION,
        }
    }
}

pub(super) fn initial_layout() -> DockState<Dock> {
    let mut state = DockState::new(vec![Dock::Scene]);
    let tree = state.main_surface_mut();
    // Godot-style workspace: hierarchy on the left, viewport in the centre,
    // inspector above the file system on the right, output below the viewport.
    let [workspace, left] =
        tree.split_left(NodeIndex::root(), theme::LEFT_WIDTH, vec![Dock::NodeTree]);
    let [workspace, right] =
        tree.split_right(workspace, theme::CENTER_WIDTH, vec![Dock::Inspector]);
    tree.split_below(left, theme::LEFT_TOP_HEIGHT, vec![Dock::Library]);
    tree.split_below(right, theme::RIGHT_BOTTOM_HEIGHT, vec![Dock::Assets]);
    tree.split_below(
        workspace,
        theme::TOP_HEIGHT,
        vec![Dock::Console, Dock::Timeline],
    );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_exposes_every_dock_role() {
        let state = initial_layout();
        for tab in Dock::ALL {
            assert!(state.find_tab(&tab).is_some(), "Missing dock {tab:?}");
        }
        assert_eq!(Dock::Scene.label(), strings::VIEWPORT);
        assert_eq!(Dock::NodeTree.label(), strings::SCENE_TREE);
        assert_eq!(Dock::Assets.label(), strings::FILE_SYSTEM);
    }
}
