//! Centre-of-screen mouse silhouette with clickable hotspots and side
//! labels connected by leader lines.
//!
//! Per UI.md phases 6 (silhouette + hotspots) and 7 (labels + leader lines).
//! The base art is drawn from positioned divs rather than shipping
//! placeholder SVGs — keeps the asset pipeline empty until a real
//! illustrator is in the loop, and the silhouette is simple enough that
//! shapes are fine. Each hotspot is a `Popover` whose trigger is a custom
//! `HotspotTrigger` element that highlights on hover *and* while the popover
//! is open.

use std::time::Duration;

use gpui::{
    Anchor, Animation, AnimationExt as _, AnyElement, App, Context, ElementId, Entity, FontWeight,
    InteractiveElement, IntoElement, MouseButton, ParentElement, Render, RenderOnce,
    StatefulInteractiveElement as _, Styled, Window, canvas, div, ease_in_out, hsla, px, rgb,
};
use gpui_component::{Selectable, popover::Popover, v_flex};

use crate::data::mouse_buttons::{ButtonId, Hotspot, MOUSE_MODEL_SIZE, default_hotspots};
use crate::mouse_model::leader_lines::{Label, Side, paint as paint_leader_lines};
use crate::mouse_model::picker::action_picker;
use crate::state::AppState;
use crate::theme::{ACCENT_BLUE, BORDER, SURFACE, SURFACE_HOVER, TEXT_MUTED, TEXT_PRIMARY};

const SIDE_W: f32 = 200.;
const SIDE_GAP: f32 = 32.;
const LABEL_W: f32 = 160.;
const LABEL_H: f32 = 44.;

pub struct MouseModelView {
    hotspots: Vec<Hotspot>,
    labels: Vec<Label>,
    hovered: Option<ButtonId>,
}

impl MouseModelView {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            hotspots: default_hotspots(),
            labels: default_labels(),
            hovered: None,
        }
    }
}

impl Render for MouseModelView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (mouse_w, mouse_h) = MOUSE_MODEL_SIZE;
        let canvas_w = mouse_w + (SIDE_W + SIDE_GAP) * 2.;
        let canvas_h = mouse_h;
        let mouse_left = SIDE_W + SIDE_GAP;
        let active = cx.try_global::<AppState>().and_then(|s| s.active_button);
        let highlight = self.hovered.or(active);
        let bindings = cx
            .try_global::<AppState>()
            .map(|s| s.button_bindings.clone())
            .unwrap_or_default();
        let view = cx.entity();
        let hovered = self.hovered;

        // Leader lines are painted via a full-canvas overlay positioned
        // behind the labels and hotspots so highlights stay readable.
        let hotspots = self.hotspots.clone();
        let labels = self.labels.clone();
        let highlight_for_canvas = highlight;
        let leader_canvas = canvas(
            move |_bounds, _, _| (hotspots, labels, highlight_for_canvas),
            move |bounds, payload, window, _app| {
                let (hotspots, labels, highlight) = payload;
                paint_leader_lines(
                    bounds,
                    gpui::point(px(mouse_left), px(0.)),
                    mouse_w,
                    &hotspots,
                    &labels,
                    highlight,
                    window,
                );
            },
        )
        .size_full();

        div()
            .relative()
            .w(px(canvas_w))
            .h(px(canvas_h))
            .child(leader_canvas)
            // Mouse silhouette + hotspots inside their own positioned
            // sub-container so the absolute hotspot coords stay
            // mouse-local. Wrapped in `with_animation` for the ambient
            // breathing rise/fall (UI.md Phase 8). The container is
            // absolute-positioned so vertical breathing happens via
            // `.top(px(mouse_left + dy))` — `.mt` wouldn't take effect.
            .child(
                div()
                    .absolute()
                    .left(px(mouse_left))
                    .top(px(0.))
                    .w(px(mouse_w))
                    .h(px(mouse_h))
                    .child(silhouette(mouse_w, mouse_h))
                    .children(self.hotspots.iter().enumerate().map(|(idx, hotspot)| {
                        hotspot_popover(idx, *hotspot, hovered, active, &view)
                    }))
                    .with_animation(
                        "mouse-breath",
                        Animation::new(Duration::from_secs(4))
                            .repeat()
                            .with_easing(ease_in_out),
                        |this, delta| {
                            let dy = (delta * std::f32::consts::TAU).sin() * BREATH_AMPLITUDE;
                            this.top(px(dy))
                        },
                    ),
            )
            // Labels, positioned in canvas-local coords on each side.
            .children(self.labels.iter().map(|label| {
                let binding = bindings
                    .get(&label.id)
                    .map_or("Unbound".to_string(), |a| a.label().to_string());
                label_card(label, binding, highlight == Some(label.id), mouse_left, mouse_w)
            }))
    }
}

/// Vertical amplitude of the breathing loop. Two pixels reads as a soft
/// rise/fall without feeling unstable.
const BREATH_AMPLITUDE: f32 = 2.0;

fn default_labels() -> Vec<Label> {
    vec![
        Label {
            id: ButtonId::LeftClick,
            side: Side::Left,
            y: 80.,
        },
        Label {
            id: ButtonId::Back,
            side: Side::Left,
            y: 230.,
        },
        Label {
            id: ButtonId::Forward,
            side: Side::Left,
            y: 300.,
        },
        Label {
            id: ButtonId::RightClick,
            side: Side::Right,
            y: 80.,
        },
        Label {
            id: ButtonId::MiddleClick,
            side: Side::Right,
            y: 180.,
        },
        Label {
            id: ButtonId::DpiToggle,
            side: Side::Right,
            y: 260.,
        },
    ]
}

fn label_card(
    label: &Label,
    binding: String,
    highlighted: bool,
    mouse_left: f32,
    mouse_w: f32,
) -> impl IntoElement {
    let x = match label.side {
        Side::Left => mouse_left - SIDE_GAP - SIDE_W,
        Side::Right => mouse_left + mouse_w + SIDE_GAP,
    };

    div()
        .absolute()
        .left(px(x))
        .top(px(label.y - LABEL_H / 2.))
        .w(px(LABEL_W))
        .h(px(LABEL_H))
        .px_3()
        .py_2()
        .rounded_md()
        .border_1()
        .border_color(rgb(if highlighted { ACCENT_BLUE } else { BORDER }))
        .bg(rgb(SURFACE))
        .child(
            v_flex()
                .gap_0p5()
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(TEXT_MUTED))
                        .child(label.id.label()),
                )
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(if highlighted {
                            ACCENT_BLUE
                        } else {
                            TEXT_PRIMARY
                        }))
                        .child(binding),
                ),
        )
}

/// The static "mouse body" art.
fn silhouette(w: f32, h: f32) -> impl IntoElement {
    div()
        .absolute()
        .inset_0()
        .w(px(w))
        .h(px(h))
        .rounded_3xl()
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(SURFACE))
        .child(
            div()
                .absolute()
                .left(px(w / 2. - 14.))
                .top(px(90.))
                .w(px(28.))
                .h(px(110.))
                .rounded_md()
                .bg(rgb(SURFACE_HOVER)),
        )
        .child(
            div()
                .absolute()
                .left(px(w / 2.))
                .top(px(20.))
                .w(px(1.))
                .h(px(240.))
                .bg(rgb(BORDER)),
        )
        .child(
            div()
                .absolute()
                .left(px(8.))
                .top(px(210.))
                .w(px(34.))
                .h(px(150.))
                .rounded_md()
                .bg(rgb(SURFACE_HOVER)),
        )
}

fn hotspot_popover(
    idx: usize,
    hotspot: Hotspot,
    hovered: Option<ButtonId>,
    active: Option<ButtonId>,
    view: &Entity<MouseModelView>,
) -> AnyElement {
    let view = view.clone();
    let trigger = HotspotTrigger {
        id: ("hotspot-trigger", idx).into(),
        hotspot,
        hovered: hovered == Some(hotspot.id) || active == Some(hotspot.id),
        view: view.clone(),
        selected: false,
    };
    Popover::new(("hotspot-popover", idx))
        .anchor(Anchor::TopRight)
        .mouse_button(MouseButton::Left)
        .trigger(trigger)
        .content(move |_state, _window, cx| action_picker(hotspot.id, &view, cx))
        .into_any_element()
}

/// Transparent click target + glow. Implements [`Selectable`] so the
/// surrounding [`Popover`] can colour it while open.
#[derive(IntoElement)]
struct HotspotTrigger {
    id: ElementId,
    hotspot: Hotspot,
    hovered: bool,
    view: Entity<MouseModelView>,
    selected: bool,
}

impl Selectable for HotspotTrigger {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl RenderOnce for HotspotTrigger {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let highlighted = self.hovered || self.selected;
        let hotspot = self.hotspot;
        let view = self.view;
        let btn = hotspot.id;

        div()
            .id(self.id)
            .absolute()
            .left(px(hotspot.x))
            .top(px(hotspot.y))
            .w(px(hotspot.w))
            .h(px(hotspot.h))
            .rounded_md()
            .border_2()
            .border_color(if highlighted {
                rgb(ACCENT_BLUE).into()
            } else {
                hsla(0., 0., 0., 0.)
            })
            .bg(if highlighted {
                hsla(0.6, 0.85, 0.6, 0.18)
            } else {
                hsla(0., 0., 0., 0.)
            })
            .on_hover(move |hovered, _window, cx| {
                let is_hovered = *hovered;
                view.update(cx, |this, cx| {
                    if is_hovered {
                        this.hovered = Some(btn);
                    } else if this.hovered == Some(btn) {
                        this.hovered = None;
                    }
                    cx.notify();
                });
            })
    }
}
