use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::center::NotificationCenter;
use super::layout::{AVATAR_GAP, TOAST_PADDING};
use crate::app_state::AppState;
use crate::call::RingEntry;
use crate::theme;
use crate::views::avatar::person_avatar;
use crate::views::call_view::pulsing;
use crate::views::widgets::icon;

pub const RING_AVATAR: f32 = 56.;
const RING_BORDER: f32 = 2.;
const RING_WIDTH: f32 = 3.;
const RING_GAP: f32 = 3.;
const TOAST_RADIUS: f32 = 12.;
const BUTTON_HEIGHT: f32 = 32.;
pub const ENDS_CURRENT_CALL: &str = "Accept ends current call";

pub struct RingView {
    app: Entity<AppState>,
    ring_id: u64,
    _subscription: Subscription,
}

impl RingView {
    pub fn new(center: Entity<NotificationCenter>, ring_id: u64, cx: &mut Context<Self>) -> Self {
        let app = center.read(cx).app().clone();
        let subscription = cx.observe(&app, |_, _, cx| cx.notify());
        RingView {
            app,
            ring_id,
            _subscription: subscription,
        }
    }
}

fn answer_button(id: &'static str, label: &str, glyph: IconName, fill: Hsla) -> Stateful<gpui_kit::Div> {
    h_flex()
        .id(id)
        .h(px(BUTTON_HEIGHT))
        .px(px(14.))
        .gap(px(6.))
        .flex_none()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(fill)
        .text_color(theme::white())
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .hover(|button| button.opacity(0.85))
        .child(icon(glyph, 16., theme::white()))
        .child(label.to_owned())
}

fn caller_avatar(state: &AppState, entry: &RingEntry) -> AnyElement {
    let avatar = person_avatar(
        &state.directory,
        entry.caller_user_id().as_deref(),
        entry.caller_name(),
        RING_AVATAR,
    );
    let ringed = div()
        .p(px(RING_GAP))
        .rounded_full()
        .border(px(RING_WIDTH))
        .border_color(theme::green())
        .child(avatar);
    pulsing(ringed, "ring-avatar-pulse")
}

impl Render for RingView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.app.read(cx);
        let Some(entry) = state.rings.get(self.ring_id).cloned() else {
            return div().size_full().bg(theme::surface_raised()).into_any_element();
        };
        window.set_window_title(&format!("{}: {}", entry.caller_name(), entry.subtitle()));
        let ends_current = state.call.is_some();
        let avatar = caller_avatar(state, &entry);
        let ring_id = self.ring_id;
        let decline_app = self.app.clone();
        let accept_app = self.app.clone();
        let decline = answer_button("ring-decline", "Decline", IconName::PhoneOff, theme::red()).on_click(
            move |_, _, cx| {
                cx.stop_propagation();
                decline_app.update(cx, |state, cx| state.decline_ring(ring_id, cx));
            },
        );
        let accept = answer_button("ring-accept", entry.accept_label(), IconName::Phone, theme::green()).on_click(
            move |_, _, cx| {
                cx.stop_propagation();
                accept_app.update(cx, |state, cx| state.accept_ring(ring_id, cx));
            },
        );
        div()
            .id(("ring", ring_id))
            .size_full()
            .rounded(px(TOAST_RADIUS))
            .bg(theme::surface_raised())
            .border(px(RING_BORDER))
            .border_color(theme::green())
            .overflow_hidden()
            .child(
                div().size_full().p(px(TOAST_PADDING)).child(
                    h_flex()
                        .size_full()
                        .gap(px(AVATAR_GAP))
                        .items_start()
                        .child(avatar)
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(15.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme::text())
                                        .child(entry.caller_name().to_owned()),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.5))
                                        .text_color(theme::text_muted())
                                        .child(entry.subtitle()),
                                )
                                .when(ends_current, |column| {
                                    column.child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(theme::amber())
                                            .child(ENDS_CURRENT_CALL),
                                    )
                                })
                                .child(h_flex().mt(px(8.)).gap(px(8.)).child(decline).child(accept)),
                        ),
                ),
            )
            .into_any_element()
    }
}
