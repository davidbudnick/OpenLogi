//! Popover content for binding a [`ButtonId`] to an [`Action`].
//!
//! Generic over the entity that should be notified after the binding changes
//! — that lets both the Phase 4 row and the Phase 6 mouse model reuse the
//! same picker.

use std::rc::Rc;

use gpui::{
    AnyElement, BorrowAppContext as _, Context, Entity, InteractiveElement, IntoElement,
    ParentElement, StatefulInteractiveElement as _, Styled, div, px, rgb,
};
use gpui_component::{popover::PopoverState, v_flex};

use crate::data::mouse_buttons::{Action, ButtonId};
use crate::state::AppState;
use crate::theme::{ACCENT_BLUE, SURFACE, SURFACE_HOVER, TEXT_MUTED, TEXT_PRIMARY};

const POPOVER_W: f32 = 200.;

/// Build the popover body that lets the user re-bind `btn`.
///
/// `observer` is whatever entity wraps the trigger — it'll be notified after
/// the global is updated so the trigger re-renders with the new label.
pub fn action_picker<T: 'static>(
    btn: ButtonId,
    observer: &Entity<T>,
    cx: &mut Context<PopoverState>,
) -> AnyElement {
    let popover = cx.entity().downgrade();
    let current = cx
        .try_global::<AppState>()
        .and_then(|s| s.button_bindings.get(&btn).cloned());

    let items: Vec<AnyElement> = Action::catalog()
        .into_iter()
        .enumerate()
        .map(|(item_idx, action)| {
            let is_selected = current.as_ref() == Some(&action);
            let label = action.label().to_string();
            let observer = observer.clone();
            let popover = popover.clone();
            let action = Rc::new(action);
            div()
                .id(("action-item", item_idx))
                .w_full()
                .px_3()
                .py_1p5()
                .rounded_md()
                .text_sm()
                .text_color(rgb(if is_selected {
                    ACCENT_BLUE
                } else {
                    TEXT_PRIMARY
                }))
                .bg(rgb(if is_selected { SURFACE_HOVER } else { SURFACE }))
                .hover(|s| s.bg(rgb(SURFACE_HOVER)))
                .child(label)
                .on_click(move |_event, window, cx| {
                    let action = (*action).clone();
                    cx.update_global::<AppState, _>(|state, _| {
                        state.commit_binding(btn, action);
                    });
                    observer.update(cx, |_, cx| cx.notify());
                    if let Some(p) = popover.upgrade() {
                        p.update(cx, |s, cx| s.dismiss(window, cx));
                    }
                })
                .into_any_element()
        })
        .collect();

    v_flex()
        .min_w(px(POPOVER_W))
        .gap_1()
        .p_2()
        .child(
            div()
                .text_xs()
                .text_color(rgb(TEXT_MUTED))
                .px_2()
                .pb_1()
                .child(format!("Bind {}", btn.label())),
        )
        .children(items)
        .into_any_element()
}
