//! Component lifecycle state tracked by the player for one scene binding.

use script_api::Kind;

pub(super) struct Binding {
    loaded: bool,
    started: bool,
    active: bool,
    pub(super) enabled: bool,
}

impl Default for Binding {
    fn default() -> Self {
        Self {
            loaded: false,
            started: false,
            active: false,
            enabled: true,
        }
    }
}

impl Binding {
    pub(super) fn load(&mut self, node_active: bool) -> Option<Kind> {
        if self.loaded || !node_active {
            return None;
        }
        self.loaded = true;
        Some(Kind::Load)
    }

    pub(super) fn change_activation(&mut self, node_active: bool) -> Option<Kind> {
        let next = self.loaded && self.enabled && node_active;
        if self.active == next {
            return None;
        }
        self.active = next;
        Some(if next { Kind::Enable } else { Kind::Disable })
    }

    pub(super) fn start(&mut self, node_active: bool) -> Option<Kind> {
        if self.started || !self.active || !self.enabled || !node_active {
            return None;
        }
        self.started = true;
        Some(Kind::Start)
    }

    pub(super) fn runs(&self, node_active: bool) -> bool {
        self.started && self.active && self.enabled && node_active
    }

    pub(super) fn destroy(&mut self) -> Option<Kind> {
        self.loaded.then(|| {
            *self = Self::default();
            Kind::Destroy
        })
    }
}
