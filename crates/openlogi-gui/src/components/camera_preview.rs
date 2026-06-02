//! Live webcam preview for the camera detail panel.
//!
//! Streams frames from the active Logitech camera via `openlogi-camera` and
//! renders them into a gpui image, repainting ~30 fps while a camera is
//! selected. The stream is started only when Camera permission is already
//! granted, so the UI thread never blocks on the system permission dialog.

use std::sync::Arc;
use std::time::Duration;

use gpui::{
    AnyElement, App, Context, IntoElement, ParentElement, Render, RenderImage, SharedString,
    Styled, Subscription, Window, div, img, px,
};
use gpui_component::v_flex;
use image::{Frame as ImageFrame, RgbaImage};
use openlogi_camera::{CameraStream, Frame};
use openlogi_core::device::DeviceKind;

use crate::state::AppState;
use crate::theme::{self, Palette};

const PREVIEW_W: f32 = 480.;
const PREVIEW_H: f32 = 270.; // 16:9

/// Live preview view. Owns the capture stream and swaps it whenever the active
/// camera changes (or stops it when a non-camera device is selected).
pub struct CameraPreview {
    stream: Option<CameraStream>,
    streaming_uid: Option<String>,
    #[allow(dead_code, reason = "held to keep the AppState observer alive")]
    state_obs: Subscription,
}

impl CameraPreview {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state_obs = cx.observe_global::<AppState>(|_, cx| cx.notify());
        // Repaint ~30 fps while streaming so freshly-delivered frames show.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;
                let still_alive = this
                    .update(cx, |view, cx| {
                        if view.stream.is_some() {
                            cx.notify();
                        }
                    })
                    .is_ok();
                if !still_alive {
                    break;
                }
            }
        })
        .detach();

        Self {
            stream: None,
            streaming_uid: None,
            state_obs,
        }
    }

    /// Unique id of the active camera, if a camera is currently selected. The
    /// id is recovered from the `"camera-<uid>"` config key built in
    /// `state::devices::camera_record`.
    fn active_camera_uid(cx: &App) -> Option<String> {
        let record = cx.try_global::<AppState>()?.current_record()?;
        if !matches!(record.kind, DeviceKind::Camera) {
            return None;
        }
        record
            .config_key
            .strip_prefix("camera-")
            .map(ToOwned::to_owned)
    }

    /// Start/stop the capture stream so it tracks the active camera.
    fn sync_stream(&mut self, cx: &App) {
        let target = Self::active_camera_uid(cx);
        if target == self.streaming_uid {
            return;
        }
        self.stream = None;
        self.streaming_uid.clone_from(&target);
        if let Some(uid) = target {
            if openlogi_camera::camera_access_granted() {
                self.stream = openlogi_camera::start_stream(&uid).ok();
            }
        }
    }
}

impl Render for CameraPreview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_stream(cx);
        let pal = theme::palette(cx);
        let granted = openlogi_camera::camera_access_granted();
        let frame = self.stream.as_ref().and_then(CameraStream::latest_frame);

        let inner: AnyElement = match (granted, frame) {
            (true, Some(frame)) => frame_image(&frame),
            (true, None) => note(tr!("Starting preview…"), pal),
            (false, _) => note(tr!("Enable Camera access in Settings to preview."), pal),
        };

        v_flex()
            .w(px(PREVIEW_W))
            .h(px(PREVIEW_H))
            .items_center()
            .justify_center()
            .rounded_md()
            .border_1()
            .border_color(pal.border)
            .bg(pal.surface)
            .child(inner)
    }
}

/// Build a gpui image element from one RGBA camera frame. gpui's [`RenderImage`]
/// expects **BGRA**, so the red/blue channels are swapped first.
fn frame_image(frame: &Frame) -> AnyElement {
    let mut bgra = frame.rgba.clone();
    for chunk in bgra.chunks_exact_mut(4) {
        chunk.swap(0, 2);
    }
    let Some(buffer) = RgbaImage::from_raw(frame.width, frame.height, bgra) else {
        return div().into_any_element();
    };
    let image = Arc::new(RenderImage::new(vec![ImageFrame::new(buffer)]));
    img(image)
        .w(px(PREVIEW_W))
        .h(px(PREVIEW_H))
        .into_any_element()
}

fn note(text: impl Into<SharedString>, pal: Palette) -> AnyElement {
    div()
        .text_sm()
        .text_color(pal.text_muted)
        .child(text.into())
        .into_any_element()
}
