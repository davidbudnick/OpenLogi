//! Detail panel for a non-pointer device (today: a Logitech webcam).
//!
//! The mouse model + DPI panel only describe pointing devices. Anything else
//! gets this generic panel instead of a mouse silhouette that doesn't fit. A
//! camera gets the live [`CameraPreview`]; any other non-pointer device gets an
//! honest "detected, not configurable yet" note.

use gpui::{
    AnyElement, Entity, FontWeight, IntoElement, ParentElement, SharedString, Styled, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::v_flex;
use openlogi_core::device::DeviceKind;

use crate::components::camera_preview::CameraPreview;
use crate::state::DeviceRecord;
use crate::theme::Palette;

/// Generic detail panel for `record`. Cameras get the live preview; any other
/// non-pointer device gets a "detected, not configurable yet" note.
pub fn device_view(
    record: &DeviceRecord,
    camera_preview: &Entity<CameraPreview>,
    pal: Palette,
) -> AnyElement {
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
        .when(is_camera, |this| this.child(camera_preview.clone()))
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
