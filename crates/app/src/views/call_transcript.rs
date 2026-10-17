use gpui_kit::assets::IconName;
use gpui_kit::component::{
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::call_view::side_panel;
use super::scroll_edge::with_top_edge;
use super::widgets::icon;
use crate::app_state::AppState;
use crate::call::clock_label;
use crate::theme;

const TRANSCRIPT_SCROLL_DIVIDER: &str = "shell/islands/main/call-transcript/divider-scroll";
const SEARCH_HEIGHT: f32 = 34.;
const AT_BOTTOM_SLACK: f32 = 4.;
const EMPTY_TEXT: &str = "Nothing said yet. Spoken words appear here.";
const NO_MATCH_TEXT: &str = "No entry matches the search.";

pub struct TranscriptPanel {
    app: Entity<AppState>,
    search: Entity<InputState>,
    scroll: ScrollHandle,
    following: bool,
    scrolled: bool,
    _subscriptions: Vec<Subscription>,
}

impl TranscriptPanel {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search transcript"));
        let subscriptions = vec![
            cx.observe(&app, |_, _, cx| cx.notify()),
            cx.subscribe_in(&search, window, |_, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
        ];
        TranscriptPanel {
            app,
            search,
            scroll: ScrollHandle::new(),
            following: true,
            scrolled: false,
            _subscriptions: subscriptions,
        }
    }

    fn at_bottom(&self) -> bool {
        let offset = self.scroll.offset().y;
        let reach = self.scroll.max_offset().y;
        f32::from(offset + reach) <= AT_BOTTOM_SLACK
    }

    fn jump_to_latest(&mut self, cx: &mut Context<Self>) {
        self.following = true;
        self.scroll.scroll_to_bottom();
        cx.notify();
    }

    fn jump_chip(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("call-transcript-jump")
            .absolute()
            .bottom(px(12.))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(
                h_flex()
                    .id("call-transcript-jump-chip")
                    .h(px(28.))
                    .px(px(12.))
                    .gap(px(6.))
                    .items_center()
                    .rounded_full()
                    .bg(theme::accent())
                    .text_color(theme::white())
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .child(icon(IconName::ArrowDown, 13., theme::white()))
                    .child("Jump to latest")
                    .on_click(cx.listener(|this, _, _, cx| this.jump_to_latest(cx))),
            )
    }
}

impl Render for TranscriptPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.scrolled) {
            self.following = self.at_bottom();
        }
        if self.following {
            self.scroll.scroll_to_bottom();
        }
        let query = self.search.read(cx).value().to_string();
        let state = self.app.read(cx);
        let transcript = state.call.as_ref().map(|call| &call.model.transcript);
        let empty = transcript.is_none_or(|transcript| transcript.is_empty());
        let rows: Vec<AnyElement> = transcript
            .map(|transcript| transcript.filtered(&query))
            .unwrap_or_default()
            .into_iter()
            .map(|entry| {
                v_flex()
                    .gap(px(2.))
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .items_baseline()
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::BOLD)
                                    .child(entry.speaker.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(11.))
                                    .text_color(theme::text_muted())
                                    .child(clock_label(entry.offset)),
                            ),
                    )
                    .child(div().text_size(px(13.)).child(entry.text.clone()))
                    .into_any_element()
            })
            .collect();
        let hint = rows.is_empty().then_some(if empty { EMPTY_TEXT } else { NO_MATCH_TEXT });
        let app = self.app.clone();
        let search = h_flex()
            .h(px(SEARCH_HEIGHT))
            .mx(px(12.))
            .px(px(10.))
            .gap(px(8.))
            .flex_none()
            .items_center()
            .rounded(px(6.))
            .border_1()
            .border_color(theme::border())
            .child(icon(IconName::Search, 14., theme::text_muted()))
            .child(
                div()
                    .flex_1()
                    .child(Input::new(&self.search).appearance(false).bordered(false)),
            );
        let list = div()
            .id("call-transcript-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                this.scrolled = true;
                cx.notify();
            }))
            .child(
                v_flex()
                    .px(px(20.))
                    .py(px(12.))
                    .gap(px(12.))
                    .children(rows)
                    .children(hint.map(|text| {
                        div()
                            .text_size(px(13.))
                            .text_color(theme::text_muted())
                            .child(text)
                    })),
            );
        let body = v_flex()
            .size_full()
            .min_h_0()
            .gap(px(8.))
            .child(search)
            .child(
                with_top_edge(list, &self.scroll, TRANSCRIPT_SCROLL_DIVIDER)
                    .when(!self.following, |area| area.child(self.jump_chip(cx))),
            );
        side_panel(
            "call-transcript-panel",
            "Transcript",
            "Close the transcript",
            move |cx| app.update(cx, |state, cx| state.close_call_transcript(cx)),
            body,
        )
        .debug_selector(|| "shell/islands/main/call-transcript".into())
    }
}
