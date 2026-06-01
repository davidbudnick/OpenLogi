//! Generic detail panel for a non-pointer device.
//!
//! Keyboards, numpads, headsets and the like enumerate and appear in the
//! carousel, but the mouse model + DPI panel don't describe them. Show the
//! device identity, its connection state (battery, or "Wired" for a USB-cabled
//! keyboard), and — for keyboards — basic lighting controls. Other kinds get an
//! honest "not configurable yet" note instead of a stuck mouse silhouette
//! (issue #19).

use gpui::{
    AnyElement, Entity, FontWeight, IntoElement, ParentElement, SharedString, Styled, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::v_flex;
use openlogi_core::device::DeviceKind;
use openlogi_hid::DeviceRoute;

use crate::components::lighting_panel::LightingPanel;
use crate::state::DeviceRecord;
use crate::theme::Palette;

pub fn device_view(
    record: &DeviceRecord,
    lighting_panel: &Entity<LightingPanel>,
    pal: Palette,
) -> AnyElement {
    // Wired G-series keyboards arrive through the direct (USB) path as
    // `Unknown`, so treat *direct* unknown devices as keyboards and offer the
    // lighting controls. A receiver-side `Unknown` (e.g. a future wireless
    // peripheral) is left out — it shouldn't get a keyboard RGB panel.
    let is_keyboard = matches!(record.kind, DeviceKind::Keyboard)
        || (matches!(record.kind, DeviceKind::Unknown)
            && matches!(record.route, Some(DeviceRoute::Direct { .. })));
    v_flex()
        .flex_1()
        .w_full()
        .min_h_0()
        .items_center()
        .justify_center()
        .gap_3()
        .p_8()
        .child(
            div()
                .text_xl()
                .font_weight(FontWeight::SEMIBOLD)
                .child(record.display_name.clone()),
        )
        .child(
            div()
                .text_sm()
                .text_color(pal.text_muted)
                .child(connection_label(record)),
        )
        .when(is_keyboard, |this| this.child(lighting_panel.clone()))
        .when(!is_keyboard, |this| {
            this.child(
                div()
                    .max_w(px(440.))
                    .text_sm()
                    .text_color(pal.text_muted)
                    .child(tr!(
                        "This device is detected, but configuration for it isn't available yet."
                    )),
            )
        })
        .into_any_element()
}

/// A device's connection state for the detail panel: its battery percentage
/// when it reports one, "Wired" for a USB-cabled device that doesn't, or
/// "Offline" when unreachable. Wired keyboards never report a battery, so the
/// battery's absence on a directly-attached device is the "plugged in" signal.
fn connection_label(record: &DeviceRecord) -> SharedString {
    if !record.online {
        tr!("Offline")
    } else if record.battery.is_none() && matches!(&record.route, Some(DeviceRoute::Direct { .. }))
    {
        tr!("Connected · Wired")
    } else {
        tr!("Connected")
    }
}
