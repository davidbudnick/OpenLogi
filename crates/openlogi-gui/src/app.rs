//! Root view: header (device carousel), body (mouse model + side config),
//! footer (settings / version).
//!
//! The body is now arranged per UI.md §1: the [`MouseModelView`] sits on
//! the left, and the right column stacks the DPI panel + gesture pad
//! (placeholders for the eventual multi-tab config panel).

use gpui::{
    AppContext as _, Context, Entity, FontWeight, IntoElement, ParentElement, Render, Styled,
    Window, div, px, rgb,
};
use gpui_component::{ActiveTheme, h_flex, v_flex};
use openlogi_core::config::Config;
use openlogi_core::device::DeviceInventory;
use tracing::{info, warn};

use crate::asset::AssetCache;
use crate::components::device_carousel::DeviceCarousel;
use crate::components::dpi_panel::{DpiPanel, DpiTarget};
use crate::components::gesture_pad::GesturePad;
use crate::mouse_model::view::MouseModelView;
use crate::state::AppState;
use crate::theme::{BG_DARK, BORDER, FOOTER_H, HEADER_H, TEXT_MUTED, TEXT_PRIMARY};

pub struct AppView {
    carousel: Entity<DeviceCarousel>,
    mouse_model: Entity<MouseModelView>,
    dpi_panel: Entity<DpiPanel>,
    gesture_pad: Entity<GesturePad>,
}

impl AppView {
    pub fn new(inventories: &[DeviceInventory], cx: &mut Context<Self>) -> Self {
        // Load persisted config first so the initial AppState reflects any
        // saved bindings. A malformed or unreadable file falls back to
        // defaults with a warning rather than crashing the UI.
        let config = match Config::load_or_default() {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, "could not load config.toml — starting with defaults");
                Config::default()
            }
        };

        // Resolve the device asset for the first online paired device that
        // reports HID++ model info. Carousel-driven re-resolution comes in
        // a later phase.
        let cache = AssetCache::new();
        let asset = inventories
            .iter()
            .flat_map(|inv| inv.paired.iter())
            .find_map(|p| p.model_info.as_ref().and_then(|m| cache.resolve(m)));
        if let Some(a) = asset.as_ref() {
            info!(depot = %a.depot, display = %a.display_name, "device asset resolved");
        } else {
            info!(
                root = ?cache.cache_root(),
                "no asset match for connected devices — using synthetic silhouette"
            );
        }

        // Config-persistence key for the active device. Uses the HID++
        // model+ext identifier so the config file can scope settings per
        // physical device. Same selection rule as the DPI target below.
        let device_key = inventories
            .iter()
            .flat_map(|inv| inv.paired.iter())
            .find_map(|p| p.model_info.as_ref().map(|m| m.config_key()));
        if let Some(k) = device_key.as_deref() {
            info!(device_key = k, "config bindings scoped to device");
        }

        if !cx.has_global::<AppState>() {
            cx.set_global(AppState::from_config(config, device_key));
        }

        // Route the DPI slider's HID writes to the first online paired
        // device that reports a receiver `unique_id`. The carousel
        // doesn't drive selection yet (v0.0.1 takes whatever's first).
        let dpi_target = inventories.iter().find_map(|inv| {
            let receiver_uid = inv.receiver.unique_id.clone()?;
            let paired = inv.paired.iter().find(|p| p.online)?;
            Some(DpiTarget {
                receiver_uid,
                slot: paired.slot,
            })
        });
        if let Some(t) = dpi_target.as_ref() {
            info!(slot = t.slot, receiver = %t.receiver_uid, "DPI slider targets device");
        }

        let carousel = cx.new(|cx| DeviceCarousel::new(inventories, cx));
        let mouse_model = cx.new(|cx| MouseModelView::new(asset, cx));
        let dpi_panel = cx.new(|cx| DpiPanel::new(dpi_target, cx));
        let gesture_pad = cx.new(GesturePad::new);
        Self {
            carousel,
            mouse_model,
            dpi_panel,
            gesture_pad,
        }
    }
}

impl Render for AppView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(rgb(BG_DARK))
            .text_color(rgb(TEXT_PRIMARY))
            .child(header(&self.carousel))
            .child(body(&self.mouse_model, &self.dpi_panel, &self.gesture_pad))
            .child(footer(cx))
    }
}

fn header(carousel: &Entity<DeviceCarousel>) -> impl IntoElement {
    h_flex()
        .h(px(HEADER_H))
        .w_full()
        .px_5()
        .gap_4()
        .items_center()
        .border_b_1()
        .border_color(rgb(BORDER))
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
                .child(panel_label("Gestures"))
                .child(gesture_pad.clone()),
        )
}

fn panel_label(text: &'static str) -> impl IntoElement {
    div().text_sm().text_color(rgb(TEXT_MUTED)).child(text)
}

fn footer(cx: &Context<AppView>) -> impl IntoElement {
    let theme = cx.theme();
    h_flex()
        .h(px(FOOTER_H))
        .w_full()
        .px_5()
        .gap_4()
        .items_center()
        .justify_between()
        .border_t_1()
        .border_color(rgb(BORDER))
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child("Settings · About"),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(concat!("v", env!("CARGO_PKG_VERSION"))),
        )
}
