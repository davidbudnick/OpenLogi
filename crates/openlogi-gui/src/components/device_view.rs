//! Detail panel for a non-pointer device (today: a Logitech webcam).
//!
//! The mouse model + DPI panel only describe pointing devices. Anything else
//! gets this generic panel instead of a mouse silhouette that doesn't fit. For
//! a camera it shows the device identity and a 16:9 preview area; live preview
//! and per-camera controls are the planned next step — they need Camera
//! permission and an AVFoundation capture session.

use gpui::{
    AnyElement, FontWeight, IntoElement, ParentElement, SharedString, Styled, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::v_flex;
use openlogi_core::device::DeviceKind;

use crate::state::DeviceRecord;
use crate::theme::Palette;

const PREVIEW_W: f32 = 384.;
const PREVIEW_H: f32 = 216.; // 16:9

/// Generic detail panel for `record`. Cameras get the preview area; any other
/// non-pointer device gets an honest "detected, not configurable yet" note.
pub fn device_view(record: &DeviceRecord, pal: Palette) -> AnyElement {
    let is_camera = matches!(record.kind, DeviceKind::Camera);
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
                .child(subtitle(record)),
        )
        .when(is_camera, |this| {
            this.child(preview_placeholder(record, pal))
        })
        .when(!is_camera, |this| {
            this.child(
                div()
                    .max_w(px(440.))
                    .text_sm()
                    .text_center()
                    .text_color(pal.text_muted)
                    .child(tr!(
                        "This device is detected, but configuration for it isn't available yet."
                    )),
            )
        })
        .into_any_element()
}

fn subtitle(record: &DeviceRecord) -> SharedString {
    match record.kind {
        DeviceKind::Camera => tr!("Logitech webcam · USB Video Class"),
        _ if record.online => tr!("Connected"),
        _ => tr!("Offline"),
    }
}

/// A 16:9 placeholder where the live preview will render. Live capture needs
/// Camera permission + an AVFoundation session, so until that lands this states
/// plainly that the camera is detected and ready.
fn preview_placeholder(record: &DeviceRecord, pal: Palette) -> AnyElement {
    v_flex()
        .gap_2()
        .items_center()
        .child(
            v_flex()
                .w(px(PREVIEW_W))
                .h(px(PREVIEW_H))
                .items_center()
                .justify_center()
                .gap_1()
                .rounded_md()
                .border_1()
                .border_color(pal.border)
                .bg(pal.surface)
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(tr!("Live preview")),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(pal.text_muted)
                        .child(record.display_name.clone()),
                ),
        )
        .child(
            div()
                .max_w(px(PREVIEW_W))
                .text_xs()
                .text_center()
                .text_color(pal.text_muted)
                .child(tr!(
                    "Detected and ready. Enabling live preview will ask for Camera permission."
                )),
        )
        .into_any_element()
}
