//! GPUI window listing connected Logitech HID++ devices.
//!
//! v0.0.1: static render. We enumerate devices once on startup (via tokio),
//! then hand the result to the GPUI view. Live polling lands when there is
//! something to react to (device connect/disconnect events).

use anyhow::{Context as _, Result};
use gpui::{
    AppContext, Bounds, Context, FontWeight, Hsla, IntoElement, ParentElement, Render,
    SharedString, Size, Styled, TitlebarOptions, Window, WindowBounds, WindowOptions, div,
    prelude::FluentBuilder, px,
};
use gpui_component::{ActiveTheme, Root, h_flex, v_flex};
use optminus_core::device::{
    BatteryInfo, BatteryLevel, BatteryStatus, DeviceInventory, DeviceKind, PairedDevice,
};
use tracing_subscriber::EnvFilter;

/// View backing the main window.
pub struct DeviceListView {
    inventories: Vec<DeviceInventory>,
}

impl Render for DeviceListView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let receiver_count = self.inventories.len();
        let device_count: usize = self.inventories.iter().map(|i| i.paired.len()).sum();
        let online_count: usize = self
            .inventories
            .iter()
            .flat_map(|i| &i.paired)
            .filter(|p| p.online)
            .count();

        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .p_6()
            .gap_5()
            .child(render_header(
                receiver_count,
                device_count,
                online_count,
                theme,
            ))
            .when(self.inventories.is_empty(), |this| {
                this.child(empty_state(theme))
            })
            .when(!self.inventories.is_empty(), |this| {
                this.child(
                    v_flex()
                        .gap_4()
                        .children(self.inventories.iter().map(|i| render_receiver(i, theme))),
                )
            })
    }
}

fn render_header(
    receivers: usize,
    devices: usize,
    online: usize,
    theme: &gpui_component::Theme,
) -> impl IntoElement {
    let subtitle = if receivers == 0 {
        "No receivers detected".to_string()
    } else {
        format!(
            "{online}/{devices} {} online on {receivers} {}",
            pluralize(devices, "device", "devices"),
            pluralize(receivers, "receiver", "receivers"),
        )
    };
    v_flex()
        .gap_1()
        .child(
            div()
                .text_2xl()
                .font_weight(FontWeight::BOLD)
                .child("Options−"),
        )
        .child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(subtitle),
        )
}

fn empty_state(theme: &gpui_component::Theme) -> impl IntoElement {
    v_flex()
        .flex_1()
        .items_center()
        .justify_center()
        .gap_2()
        .child(
            div()
                .text_color(theme.muted_foreground)
                .child("No Logitech HID++ receivers found."),
        )
        .child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child("Quit Logi Options+ if it is running, replug the receiver, and reopen."),
        )
}

fn render_receiver(inv: &DeviceInventory, theme: &gpui_component::Theme) -> impl IntoElement {
    let meta = format!(
        "{}  ·  vid={:04x}  pid={:04x}",
        inv.receiver.unique_id.as_deref().unwrap_or("—"),
        inv.receiver.vendor_id,
        inv.receiver.product_id,
    );

    v_flex()
        .p_4()
        .gap_3()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .child(
            v_flex()
                .gap_1()
                .child(
                    div()
                        .text_color(theme.popover_foreground)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(inv.receiver.name.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(meta),
                ),
        )
        .when(inv.paired.is_empty(), |this| {
            this.child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("No paired devices currently online."),
            )
        })
        .when(!inv.paired.is_empty(), |this| {
            this.child(
                v_flex()
                    .gap_2()
                    .children(inv.paired.iter().map(|d| render_device(d, theme))),
            )
        })
}

fn render_device(d: &PairedDevice, theme: &gpui_component::Theme) -> impl IntoElement {
    let (dot, dot_color) = if d.online {
        ("●", theme.success)
    } else {
        ("○", theme.muted_foreground)
    };

    let name = d
        .codename
        .as_deref()
        .unwrap_or("Unknown device")
        .to_string();
    let kind = kind_label(d.kind);
    let wpid = d
        .wpid
        .map_or_else(|| "wpid=?".to_string(), |w| format!("wpid={w:04x}"));
    let meta = format!("{kind}  ·  slot {}  ·  {wpid}", d.slot);

    h_flex()
        .gap_3()
        .items_center()
        .child(div().text_lg().text_color(dot_color).child(dot))
        .child(
            v_flex()
                .gap_0p5()
                .flex_1()
                .child(div().text_color(theme.foreground).child(name))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(meta),
                ),
        )
        .child(render_battery(d.battery.as_ref(), theme))
}

fn render_battery(info: Option<&BatteryInfo>, theme: &gpui_component::Theme) -> impl IntoElement {
    let (text, color) = match info {
        Some(b) => {
            let prefix = match b.status {
                BatteryStatus::Charging | BatteryStatus::ChargingSlow => "⚡ ",
                BatteryStatus::Full => "✓ ",
                BatteryStatus::Error => "⚠ ",
                _ => "",
            };
            (
                format!("{prefix}{}%", b.percentage),
                battery_color(b, theme),
            )
        }
        None => ("—".to_string(), theme.muted_foreground),
    };
    div()
        .px_2()
        .py_0p5()
        .rounded_md()
        .text_sm()
        .text_color(color)
        .child(text)
}

fn battery_color(b: &BatteryInfo, theme: &gpui_component::Theme) -> Hsla {
    // Status takes priority over level — charging colors win even on a low cell.
    match b.status {
        BatteryStatus::Charging | BatteryStatus::ChargingSlow => theme.info,
        BatteryStatus::Error => theme.danger,
        BatteryStatus::Full => theme.success,
        _ => match b.level {
            BatteryLevel::Critical => theme.danger,
            BatteryLevel::Low => theme.warning,
            BatteryLevel::Good | BatteryLevel::Full => theme.success,
            BatteryLevel::Unknown => theme.muted_foreground,
        },
    }
}

fn kind_label(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Mouse => "Mouse",
        DeviceKind::Keyboard => "Keyboard",
        DeviceKind::Numpad => "Numpad",
        DeviceKind::Presenter => "Presenter",
        DeviceKind::Remote => "Remote",
        DeviceKind::Trackball => "Trackball",
        DeviceKind::Touchpad => "Touchpad",
        DeviceKind::Tablet => "Tablet",
        DeviceKind::Gamepad => "Gamepad",
        DeviceKind::Joystick => "Joystick",
        DeviceKind::Headset => "Headset",
        DeviceKind::Unknown => "Device",
    }
}

fn pluralize(count: usize, singular: &'static str, plural: &'static str) -> &'static str {
    if count == 1 { singular } else { plural }
}

fn main() -> Result<()> {
    init_tracing();

    // GPUI owns the main thread, so run the one-shot HID probe synchronously
    // first and pass the result into the application closure.
    let inventories = enumerate_blocking().context("HID enumeration failed")?;

    gpui_platform::application().run(move |cx| {
        gpui_component::init(cx);
        cx.spawn(async move |cx| {
            let bounds = cx.update(|cx| Bounds::centered(None, Size::new(px(720.), px(560.)), cx));
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(Size::new(px(480.), px(320.))),
                titlebar: Some(TitlebarOptions {
                    title: Some(SharedString::from("Options−")),
                    appears_transparent: false,
                    traffic_light_position: None,
                }),
                ..WindowOptions::default()
            };

            #[allow(
                clippy::expect_used,
                reason = "failure to open the main window is fatal; nothing useful to recover to"
            )]
            cx.open_window(options, move |window, cx| {
                let view = cx.new(|_| DeviceListView { inventories });
                cx.new(|cx| Root::new(view, window, cx).bg(cx.theme().background))
            })
            .expect("opening the main window should not fail");
        })
        .detach();
    });

    Ok(())
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_env("OPTMINUS_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
}

fn enumerate_blocking() -> Result<Vec<DeviceInventory>> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("tokio runtime init")?;
    rt.block_on(optminus_hid::enumerate())
        .context("optminus_hid::enumerate")
}
