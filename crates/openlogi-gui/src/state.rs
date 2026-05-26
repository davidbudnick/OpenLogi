//! App-wide UI state stored as a GPUI global.
//!
//! Anything that more than one view needs to read (current device, currently
//! armed button, the DPI value the panel and the dot-preview share) lives
//! here. Per-component scratch state (hover index, gesture point buffer) stays
//! in the owning entity.

#![allow(
    dead_code,
    reason = "fields are read once their owning component lands in UI.md phases 2–4"
)]

use std::collections::BTreeMap;

use gpui::Global;
use openlogi_core::config::Config;
use tracing::{debug, warn};

use crate::data::mouse_buttons::{Action, ButtonId, default_binding};

/// Default DPI value applied to a fresh AppState. Matches a common Logitech
/// mid-range mouse and keeps the dot-preview visually obvious from frame one.
pub const DEFAULT_DPI: u32 = 1600;

pub struct AppState {
    /// Index into the carousel's device list. May briefly point past the end
    /// while devices are being enumerated; views must bounds-check.
    pub current_device: usize,
    /// HID++ config key of the active device (e.g. `"2b042"`), used to scope
    /// persisted bindings. `None` when no HID++-identifiable device is
    /// connected — in that case `commit_binding` only updates the in-memory
    /// map.
    pub current_device_key: Option<String>,
    /// The hotspot the user most recently armed by clicking. Drives the
    /// "selected button" outline on the mouse model and the popover content.
    pub active_button: Option<ButtonId>,
    pub button_bindings: BTreeMap<ButtonId, Action>,
    pub dpi: u32,
    /// Live config — kept in sync with disk via [`Self::commit_binding`] so
    /// restarts preserve user bindings without a separate sync step.
    config: Config,
}

impl AppState {
    /// Build the global from a loaded config and the active device's key.
    ///
    /// Bindings are seeded from `config[device_key]` if present, otherwise
    /// from [`default_binding`]. When `device_key` is `None`, defaults are
    /// used and writes don't persist.
    #[must_use]
    pub fn from_config(config: Config, device_key: Option<String>) -> Self {
        let stored = device_key
            .as_deref()
            .map(|k| config.bindings_for(k))
            .unwrap_or_default();
        let mut bindings: BTreeMap<ButtonId, Action> = ButtonId::ALL
            .iter()
            .copied()
            .map(|b| (b, default_binding(b)))
            .collect();
        for (k, v) in stored {
            bindings.insert(k, v);
        }
        Self {
            current_device: 0,
            current_device_key: device_key,
            active_button: None,
            button_bindings: bindings,
            dpi: DEFAULT_DPI,
            config,
        }
    }

    /// Update a single binding both in memory and on disk.
    ///
    /// Disk failures are logged at `warn` instead of bubbling up: the UI
    /// thread shouldn't crash because the user's home volume is full. A
    /// future retry / banner UI can read the most recent error from
    /// [`tracing`].
    pub fn commit_binding(&mut self, button: ButtonId, action: Action) {
        self.button_bindings.insert(button, action.clone());
        let Some(key) = self.current_device_key.as_deref() else {
            debug!(?button, "no active device key — binding kept in memory only");
            return;
        };
        self.config.set_binding(key, button, action);
        if let Err(e) = self.config.save_atomic() {
            warn!(error = %e, "could not persist binding to config.toml");
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::from_config(Config::default(), None)
    }
}

impl Global for AppState {}
