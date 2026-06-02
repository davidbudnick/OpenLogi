//! Live webcam preview for the camera detail panel.
//!
//! Streams frames from the active Logitech camera via `openlogi-camera` and
//! renders them into a gpui image. To stay memory- and CPU-flat: the preview
//! stream captures at a reduced resolution, the GPU texture is rebuilt only when
//! a *new* frame has arrived (not every repaint), and the previous texture is
//! freed via [`Window::drop_image`]. The stream starts only when Camera
//! permission is already granted, so the UI thread never blocks on the dialog.

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

/// Live preview view. Owns the capture stream and the current frame's texture,
/// swapping both as the active camera changes or new frames arrive.
pub struct CameraPreview {
    stream: Option<CameraStream>,
    streaming_uid: Option<String>,
    /// The frame currently uploaded as a GPU texture. Replaced (and the old one
    /// freed via `Window::drop_image`) only when a new frame arrives, so memory
    /// stays flat instead of leaking a texture per repaint.
    current_image: Option<Arc<RenderImage>>,
    last_generation: u64,
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
                let alive = this
                    .update(cx, |view, cx| {
                        if view.stream.is_some() {
                            cx.notify();
                        }
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        })
        .detach();

        Self {
            stream: None,
            streaming_uid: None,
            current_image: None,
            last_generation: 0,
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
        self.last_generation = 0;
        self.streaming_uid.clone_from(&target);
        if let Some(uid) = target {
            if openlogi_camera::camera_access_granted() {
                self.stream = openlogi_camera::start_stream(&uid).ok();
            }
        }
    }
}

impl Render for CameraPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_stream(cx);
        let pal = theme::palette(cx);
        let granted = openlogi_camera::camera_access_granted();

        // Switched away from a camera — free the texture.
        if self.stream.is_none() {
            if let Some(old) = self.current_image.take() {
                let _ = window.drop_image(old);
            }
        }

        // Rebuild the texture only when a new frame has arrived, freeing the old.
        if let Some(stream) = self.stream.as_ref() {
            let generation = stream.frame_generation();
            if generation != self.last_generation {
                if let Some(image) = stream.latest_frame().and_then(|frame| build_image(&frame)) {
                    if let Some(old) = self.current_image.take() {
                        let _ = window.drop_image(old);
                    }
                    self.current_image = Some(image);
                    self.last_generation = generation;
                }
            }
        }

        let inner: AnyElement = if let Some(image) = self.current_image.as_ref() {
            img(image.clone())
                .w(px(PREVIEW_W))
                .h(px(PREVIEW_H))
                .into_any_element()
        } else if granted {
            note(tr!("Starting preview…"), pal)
        } else {
            note(tr!("Enable Camera access in Settings to preview."), pal)
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

/// Build a gpui image from one RGBA camera frame. gpui's [`RenderImage`] is
/// BGRA, so red/blue are swapped; the texture is uploaded at the camera's
/// (already-reduced) capture resolution and scaled to the panel by the GPU.
fn build_image(frame: &Frame) -> Option<Arc<RenderImage>> {
    let mut bgra = frame.rgba.clone();
    for chunk in bgra.chunks_exact_mut(4) {
        chunk.swap(0, 2);
    }
    let buffer = RgbaImage::from_raw(frame.width, frame.height, bgra)?;
    Some(Arc::new(RenderImage::new(vec![ImageFrame::new(buffer)])))
}

fn note(text: impl Into<SharedString>, pal: Palette) -> AnyElement {
    div()
        .text_sm()
        .text_color(pal.text_muted)
        .child(text.into())
        .into_any_element()
}
