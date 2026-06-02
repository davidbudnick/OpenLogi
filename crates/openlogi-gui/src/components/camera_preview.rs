//! Live webcam preview for the camera detail panel — fully automatic.
//!
//! The camera opens **only while its panel is on screen**: selecting the camera
//! starts an `AVCaptureSession` (which lights the hardware LED), and switching
//! to any other device closes it (LED off, zero camera cost). There's no manual
//! toggle. While streaming it captures at a reduced resolution, rebuilds the GPU
//! texture only when a new frame arrives, repaints ~15 fps, and frees the old
//! texture via [`Window::drop_image`].
//!
//! Stream lifecycle is driven from the global [`AppState`] observer (not
//! `render`), because the panel stops rendering when you navigate away — so
//! `render` alone could never close the camera.

use std::sync::Arc;
use std::time::Duration;

use gpui::{
    AnyElement, App, Context, IntoElement, ParentElement, Render, RenderImage, SharedString,
    Styled, Subscription, Task, Window, div, img, px,
};
use gpui_component::v_flex;
use image::{Frame as ImageFrame, RgbaImage};
use openlogi_camera::{CameraStream, Frame};
use openlogi_core::device::DeviceKind;

use crate::state::AppState;
use crate::theme::{self, Palette};

const PREVIEW_W: f32 = 480.;
const PREVIEW_H: f32 = 270.; // 16:9

/// Live preview view. Holds the capture stream + its texture only while a
/// camera is the active device.
pub struct CameraPreview {
    stream: Option<CameraStream>,
    streaming_uid: Option<String>,
    current_image: Option<Arc<RenderImage>>,
    last_generation: u64,
    /// ~15 fps repaint pump; exists only while streaming (dropping it cancels it).
    repaint_task: Option<Task<()>>,
    #[allow(dead_code, reason = "held to keep the AppState observer alive")]
    state_obs: Subscription,
}

impl CameraPreview {
    pub fn new(cx: &mut Context<Self>) -> Self {
        // Drive the stream lifecycle from here, not `render`: when the user
        // switches to another device this panel stops rendering, so `render`
        // could never observe the change to close the camera.
        let state_obs = cx.observe_global::<AppState>(|this, cx| {
            this.sync_stream(cx);
            cx.notify();
        });

        Self {
            stream: None,
            streaming_uid: None,
            current_image: None,
            last_generation: 0,
            repaint_task: None,
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

    /// Open the camera when its panel is active, close it otherwise. Spawns the
    /// repaint pump when starting; cancels it (drop) when stopping.
    fn sync_stream(&mut self, cx: &mut Context<Self>) {
        let target = Self::active_camera_uid(cx);
        if target == self.streaming_uid {
            return;
        }

        // Stop: dropping the stream closes the session (LED off); dropping the
        // task cancels the repaint pump. The texture is freed in `render`.
        self.stream = None;
        self.repaint_task = None;
        self.last_generation = 0;
        self.streaming_uid.clone_from(&target);

        if let Some(uid) = target {
            if openlogi_camera::camera_access_granted() {
                self.stream = openlogi_camera::start_stream(&uid).ok();
            }
            if self.stream.is_some() {
                self.repaint_task = Some(cx.spawn(async move |this, cx| {
                    loop {
                        cx.background_executor()
                            .timer(Duration::from_millis(16))
                            .await;
                        // Repaint only when a *new* frame has arrived, so gpui
                        // isn't re-rendering the window on idle ticks — keeps the
                        // preview smooth and the rest of the UI responsive.
                        let result = this.update(cx, |view, cx| {
                            let has_new = view
                                .stream
                                .as_ref()
                                .is_some_and(|s| s.frame_generation() != view.last_generation);
                            if has_new {
                                cx.notify();
                            }
                        });
                        if result.is_err() {
                            break;
                        }
                    }
                }));
            }
        }
    }
}

impl Render for CameraPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_stream(cx);
        let pal = theme::palette(cx);
        let granted = openlogi_camera::camera_access_granted();

        // Not streaming → make sure no texture is held.
        if self.stream.is_none() {
            if let Some(old) = self.current_image.take() {
                let _ = window.drop_image(old);
            }
        }

        // Rebuild the texture only when a new frame arrived; free the old one.
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

        let surface: AnyElement = if let Some(image) = self.current_image.as_ref() {
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
            .child(surface)
    }
}

/// Build a gpui image from one RGBA camera frame. gpui's [`RenderImage`] is
/// BGRA, so red/blue are swapped; the texture is uploaded at the camera's
/// (reduced) capture resolution and scaled to the panel by the GPU.
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
