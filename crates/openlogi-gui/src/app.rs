//! Root view: header (device carousel), body (mouse model + side config),
//! footer (settings / version).
//!
//! The body is now arranged per UI.md §1: the [`MouseModelView`] sits on
//! the left, and the right column stacks the DPI panel + gesture pad
//! (placeholders for the eventual multi-tab config panel).

use gpui::{
    AppContext as _, Context, Entity, FontWeight, InteractiveElement, IntoElement, ParentElement,
    Render, Styled, Subscription, Window, div, px,
};
use gpui_component::{h_flex, v_flex};
use openlogi_core::config::Config;
use openlogi_core::device::DeviceInventory;
use tracing::{info, warn};

use crate::app_menu::{Minimize, Zoom};
use crate::asset::AssetCache;
use crate::components::device_carousel::DeviceCarousel;
use crate::components::dpi_panel::DpiPanel;
use crate::components::gesture_pad::GesturePad;
use crate::mouse_model::view::MouseModelView;
use crate::state::AppState;
use crate::theme::{self, FOOTER_H, HEADER_H, Palette};

pub struct AppView {
    carousel: Entity<DeviceCarousel>,
    mouse_model: Entity<MouseModelView>,
    dpi_panel: Entity<DpiPanel>,
    gesture_pad: Entity<GesturePad>,
    /// Keeps the OS-appearance observer alive for the window's lifetime.
    /// Set once by `main` right after the view is constructed (it needs the
    /// `Window`, which `new` doesn't have). Never read — held only so the
    /// subscription isn't dropped.
    #[allow(dead_code, reason = "held to keep the appearance observer alive")]
    appearance_obs: Option<Subscription>,
}

impl AppView {
    pub fn new(inventories: &[DeviceInventory], cx: &mut Context<Self>) -> Self {
        // Load persisted config first so the initial AppState reflects any
        // saved bindings + the last-selected device. Malformed/unreadable
        // files fall back to defaults with a warning rather than crash.
        let config = match Config::load_or_default() {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, "could not load config.toml — starting with defaults");
                Config::default()
            }
        };

        let cache = AssetCache::new();

        if !cx.has_global::<AppState>() {
            cx.set_global(AppState::with_runtime(config, inventories, &cache));
        }

        if let Some(state) = cx.try_global::<AppState>() {
            if let Some(record) = state.current_record() {
                info!(
                    device_key = %record.config_key,
                    display = %record.display_name,
                    "initial device selected"
                );
            } else {
                info!(
                    root = ?cache.cache_root(),
                    "no devices with HID++ model info — using synthetic silhouette"
                );
            }
        }

        let carousel = cx.new(|cx| DeviceCarousel::new(inventories, cx));
        let mouse_model = cx.new(MouseModelView::new);
        let dpi_panel = cx.new(DpiPanel::new);
        let gesture_pad = cx.new(GesturePad::new);
        Self {
            carousel,
            mouse_model,
            dpi_panel,
            gesture_pad,
            appearance_obs: None,
        }
    }

    /// Park the OS-appearance observer here so it outlives the call that
    /// created it. Called once from `main` after the window exists.
    pub fn set_appearance_obs(&mut self, sub: Subscription) {
        self.appearance_obs = Some(sub);
    }
}

impl Render for AppView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pal = theme::palette(cx);
        v_flex()
            .size_full()
            .bg(pal.bg)
            .text_color(pal.text_primary)
            .on_action(|_: &Minimize, window, _| window.minimize_window())
            .on_action(|_: &Zoom, window, _| window.zoom_window())
            .child(header(&self.carousel, pal))
            .child(body(
                &self.mouse_model,
                &self.dpi_panel,
                &self.gesture_pad,
                pal,
            ))
            .child(footer(pal))
    }
}

fn header(carousel: &Entity<DeviceCarousel>, pal: Palette) -> impl IntoElement {
    h_flex()
        .h(px(HEADER_H))
        .w_full()
        .px_5()
        .gap_4()
        .items_center()
        .border_b_1()
        .border_color(pal.border)
        .child(
            div()
                .text_lg()
                .font_weight(FontWeight::SEMIBOLD)
                .child("OpenLogi"),
        )
        .child(div().flex_1().min_w_0().child(carousel.clone()))
}

fn body(
    mouse_model: &Entity<MouseModelView>,
    dpi_panel: &Entity<DpiPanel>,
    gesture_pad: &Entity<GesturePad>,
    pal: Palette,
) -> impl IntoElement {
    h_flex()
        .flex_1()
        .w_full()
        .min_h_0()
        .items_start()
        .justify_center()
        .gap_6()
        .p_6()
        .child(mouse_model.clone())
        .child(
            v_flex()
                .gap_6()
                .child(dpi_panel.clone())
                .child(panel_label("Gestures", pal))
                .child(gesture_pad.clone()),
        )
}

fn panel_label(text: &'static str, pal: Palette) -> impl IntoElement {
    div().text_sm().text_color(pal.text_muted).child(text)
}

fn footer(pal: Palette) -> impl IntoElement {
    h_flex()
        .h(px(FOOTER_H))
        .w_full()
        .px_5()
        .gap_4()
        .items_center()
        .justify_between()
        .border_t_1()
        .border_color(pal.border)
        .child(
            div()
                .text_xs()
                .text_color(pal.text_muted)
                .child("Settings · About"),
        )
        .child(
            div()
                .text_xs()
                .text_color(pal.text_muted)
                .child(concat!("v", env!("CARGO_PKG_VERSION"))),
        )
}
