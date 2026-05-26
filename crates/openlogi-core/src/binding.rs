//! Logical mouse button identifiers and the action vocabulary each one can
//! bind to. Lives in `openlogi-core` because the [`config`](crate::config)
//! schema serializes these directly — the GUI re-exports them.
//!
//! When [`Action`] gains new variants (P0.2 action catalog expansion), keep
//! the existing variant names stable: the TOML config keys/values use the
//! enum variant identifiers verbatim, so renames are migration events.

use std::fmt;

use serde::{Deserialize, Serialize};

/// One of the user-rebindable hotspots on a Logi mouse. The order matches the
/// physical layout from front to side; [`ButtonId::ALL`] is consumed by the
/// default-binding generator and the popover trigger list.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub enum ButtonId {
    LeftClick,
    RightClick,
    MiddleClick,
    Back,
    Forward,
    DpiToggle,
}

impl ButtonId {
    pub const ALL: [ButtonId; 6] = [
        ButtonId::LeftClick,
        ButtonId::RightClick,
        ButtonId::MiddleClick,
        ButtonId::Back,
        ButtonId::Forward,
        ButtonId::DpiToggle,
    ];

    /// Human-readable label for popovers and tooltips.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            ButtonId::LeftClick => "Left Click",
            ButtonId::RightClick => "Right Click",
            ButtonId::MiddleClick => "Middle Click",
            ButtonId::Back => "Back",
            ButtonId::Forward => "Forward",
            ButtonId::DpiToggle => "DPI Toggle",
        }
    }
}

impl fmt::Display for ButtonId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// What pressing a [`ButtonId`] should do. Kept open-ended so the P0.2 catalog
/// expansion can grow new variants without breaking the config schema.
///
/// Serialization uses serde's default external tagging: unit variants
/// serialize as a bare string (`"BrowserBack"`) and the tuple variant
/// serializes as a single-key table (`{ CustomShortcut = "Toggle DPI" }`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    LeftClick,
    RightClick,
    MiddleClick,
    Copy,
    Paste,
    Screenshot,
    BrowserBack,
    BrowserForward,
    CustomShortcut(String),
}

impl Action {
    /// Display label for the popover row.
    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            Action::LeftClick => "Left Click",
            Action::RightClick => "Right Click",
            Action::MiddleClick => "Middle Click",
            Action::Copy => "Copy",
            Action::Paste => "Paste",
            Action::Screenshot => "Screenshot",
            Action::BrowserBack => "Browser Back",
            Action::BrowserForward => "Browser Forward",
            Action::CustomShortcut(s) => s.as_str(),
        }
    }

    /// The picker list shown inside the action popover.
    #[must_use]
    pub fn catalog() -> Vec<Action> {
        vec![
            Action::LeftClick,
            Action::RightClick,
            Action::MiddleClick,
            Action::Copy,
            Action::Paste,
            Action::Screenshot,
            Action::BrowserBack,
            Action::BrowserForward,
        ]
    }
}

/// Sensible defaults for a fresh device so the panel isn't empty on first run.
#[must_use]
pub fn default_binding(button: ButtonId) -> Action {
    match button {
        ButtonId::LeftClick => Action::LeftClick,
        ButtonId::RightClick => Action::RightClick,
        ButtonId::MiddleClick => Action::MiddleClick,
        ButtonId::Back => Action::BrowserBack,
        ButtonId::Forward => Action::BrowserForward,
        ButtonId::DpiToggle => Action::CustomShortcut("Toggle DPI".into()),
    }
}
