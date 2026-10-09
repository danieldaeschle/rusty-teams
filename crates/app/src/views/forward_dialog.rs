use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::switcher::{Candidate, candidate_avatar, clamp_highlight};
use super::widgets::{icon, symbol};
use crate::app_state::{AppState, Selection};
use crate::fuzzy;
use crate::message_actions::ForwardSource;
use crate::notice::short_error;
use crate::runtime;
use crate::theme;

const MAX_RESULTS: usize = 8;
const CARD_WIDTH: f32 = 480.;
const CARD_RADIUS: f32 = 12.;
const CARD_PADDING: f32 = 16.;
const CARD_MARGIN: f32 = 32.;
const SECTION_GAP: f32 = 12.;
const BACKDROP_OPACITY: f32 = 0.55;
const ROW_HEIGHT: f32 = 40.;
const AVATAR_SIZE: f32 = 28.;
const PREVIEW_LINES: usize = 4;
const CLOSE_SIZE: f32 = 28.;
const FIELD_HEIGHT: f32 = 36.;
const BUTTON_HEIGHT: f32 = 32.;
const DISABLED_OPACITY: f32 = 0.4;
const EMPTY_PREVIEW: &str = "Attachment";

pub enum ForwardDialogEvent {
    Close,
    Sent { target: Selection, title: String },
}

pub struct ForwardDialog {
    app: Entity<AppState>,
    source: ForwardSource,
    candidates: Vec<(String, Candidate)>,
    search: Entity<InputState>,
    comment: Entity<InputState>,
    results: Vec<Candidate>,
    highlighted: usize,
    target: Option<Candidate>,
    sending: bool,
    error: Option<String>,
    _subscriptions: [Subscription; 2],
}

impl EventEmitter<ForwardDialogEvent> for ForwardDialog {}

pub fn visible_candidates(candidates: &[(String, Candidate)], query: &str) -> Vec<Candidate> {
    fuzzy::rank(query, candidates, MAX_RESULTS)
}

pub fn can_forward(target: Option<&Candidate>, sending: bool) -> bool {
    target.is_some() && !sending
}

impl ForwardDialog {
    pub fn new(
        app: Entity<AppState>,
        source: ForwardSource,
        candidates: Vec<(String, Candidate)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search chats and channels"));
        let comment =
            cx.new(|cx| InputState::new(window, cx).placeholder("Add a message (optional)"));
        let subscriptions = [
            cx.subscribe_in(&search, window, Self::on_search_event),
            cx.subscribe_in(&comment, window, Self::on_comment_event),
        ];
        search.update(cx, |input, cx| input.focus(window, cx));
        let results = visible_candidates(&candidates, "");
        ForwardDialog {
            app,
            source,
            candidates,
            search,
            comment,
            results,
            highlighted: 0,
            target: None,
            sending: false,
            error: None,
            _subscriptions: subscriptions,
        }
    }

    fn on_search_event(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                let query = input.read(cx).value().to_string();
                self.results = visible_candidates(&self.candidates, &query);
                self.highlighted = 0;
                cx.notify();
            }
            InputEvent::PressEnter { .. } => self.choose(self.highlighted, window, cx),
            _ => {}
        }
    }

    fn on_comment_event(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::PressEnter { .. }) {
            self.send(cx);
        }
    }

    fn choose(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(candidate) = self.results.get(index).cloned() else {
            return;
        };
        self.highlighted = index;
        self.target = Some(candidate);
        self.error = None;
        self.comment.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    fn clear_target(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.target = None;
        self.error = None;
        self.search.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => cx.emit(ForwardDialogEvent::Close),
            "down" if self.target.is_none() => {
                self.highlighted = clamp_highlight(self.highlighted, 1, self.results.len());
                cx.notify();
            }
            "up" if self.target.is_none() => {
                self.highlighted = clamp_highlight(self.highlighted, -1, self.results.len());
                cx.notify();
            }
            _ => {}
        }
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.target.clone().filter(|_| !self.sending) else {
            return;
        };
        let comment = self.comment.read(cx).value().trim().to_owned();
        let state = self.app.read(cx);
        if state.mode.read_only {
            self.error = Some("Read-only mode: message not forwarded".to_owned());
            cx.notify();
            return;
        }
        let sent = ForwardDialogEvent::Sent {
            target: target.selection.clone(),
            title: target.title.clone(),
        };
        let Some(engine) = state.engine.clone() else {
            if state.mode.demo {
                let source = self.source.clone();
                self.app.update(cx, |state, cx| {
                    state.forward_locally(&source, &target.selection, &comment, cx)
                });
                cx.emit(sent);
            } else {
                self.error = Some("Not connected".to_owned());
                cx.notify();
            }
            return;
        };
        self.sending = true;
        self.error = None;
        let source = self.source.clone();
        let target_id = target.selection.conversation_id().to_owned();
        let receiver = runtime::spawn(async move {
            engine
                .forward_messages(
                    &source.conversation_id,
                    &target_id,
                    &[source.message_id],
                    &comment,
                )
                .await
        });
        cx.spawn(async move |this, cx| {
            let outcome = match receiver.await {
                Ok(Ok(_)) => Ok(()),
                Ok(Err(error)) => Err(short_error(&error)),
                Err(_) => Err("cancelled".to_owned()),
            };
            this.update(cx, |this, cx| {
                this.sending = false;
                match outcome {
                    Ok(()) => cx.emit(sent),
                    Err(reason) => this.error = Some(reason),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn header(&self, cx: &mut Context<Self>) -> Div {
        h_flex()
            .w_full()
            .items_center()
            .gap(px(SECTION_GAP))
            .child(
                div()
                    .flex_1()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Forward message"),
            )
            .child(
                div()
                    .id("forward-close")
                    .size(px(CLOSE_SIZE))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .hover(|button| button.bg(theme::row_hover()))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ForwardDialogEvent::Close)))
                    .child(symbol("close", 16., theme::text_muted())),
            )
    }

    fn target_field(&self, cx: &mut Context<Self>) -> AnyElement {
        let field = h_flex()
            .w_full()
            .h(px(FIELD_HEIGHT))
            .px(px(10.))
            .gap(px(8.))
            .items_center()
            .rounded(px(6.))
            .bg(theme::surface())
            .border_1()
            .border_color(theme::border());
        match &self.target {
            Some(target) => field
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(13.))
                        .child(format!("To: {}", target.title)),
                )
                .child(
                    div()
                        .id("forward-clear-target")
                        .size(px(20.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(4.))
                        .cursor_pointer()
                        .hover(|button| button.bg(theme::row_hover()))
                        .on_click(cx.listener(|this, _, window, cx| this.clear_target(window, cx)))
                        .child(icon(IconName::X, 14., theme::text_muted())),
                )
                .into_any_element(),
            None => field
                .child(icon(IconName::Search, 14., theme::text_muted()))
                .child(
                    div()
                        .flex_1()
                        .child(Input::new(&self.search).appearance(false).bordered(false)),
                )
                .into_any_element(),
        }
    }

    fn list(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let directory = &self.app.read(cx).directory;
        let selected_id = self
            .target
            .as_ref()
            .map(|target| target.selection.conversation_id().to_owned());
        let rows: Vec<AnyElement> = self
            .results
            .iter()
            .enumerate()
            .map(|(index, candidate)| {
                let chosen = selected_id.as_deref() == Some(candidate.selection.conversation_id());
                let highlighted = self.target.is_none() && index == self.highlighted;
                h_flex()
                    .id(ElementId::Name(format!("forward-target-{index}").into()))
                    .h(px(ROW_HEIGHT))
                    .flex_none()
                    .px(px(8.))
                    .gap(px(10.))
                    .items_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(highlighted || chosen, |row| row.bg(theme::surface_raised()))
                    .hover(|row| row.bg(theme::row_hover()))
                    .child(candidate_avatar(
                        directory,
                        &candidate.avatar,
                        AVATAR_SIZE,
                        theme::background(),
                    ))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(14.))
                                    .text_color(theme::text())
                                    .child(candidate.title.clone()),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.))
                                    .text_color(theme::text_muted())
                                    .child(candidate.subtitle.clone()),
                            ),
                    )
                    .when(chosen, |row| {
                        row.child(icon(IconName::Check, 16., theme::accent_text()))
                    })
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.choose(index, window, cx)),
                    )
                    .into_any_element()
            })
            .collect();
        v_flex()
            .id("forward-targets")
            .w_full()
            .max_h(px(ROW_HEIGHT * MAX_RESULTS as f32))
            .overflow_y_scroll()
            .children(rows)
    }

    fn send_button(&self, enabled: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let button = h_flex()
            .id("forward-send")
            .h(px(BUTTON_HEIGHT))
            .px(px(16.))
            .gap(px(6.))
            .items_center()
            .rounded(px(6.))
            .bg(theme::accent())
            .text_color(theme::on_accent())
            .text_size(px(13.))
            .font_weight(FontWeight::SEMIBOLD)
            .when(self.sending, |button| {
                button.child(icon(IconName::Loader, 14., theme::on_accent()))
            })
            .child("Forward");
        if enabled {
            button
                .cursor_pointer()
                .hover(|button| button.bg(theme::accent_text()))
                .on_click(cx.listener(|this, _, _, cx| this.send(cx)))
        } else {
            button.opacity(DISABLED_OPACITY)
        }
    }

    fn preview(&self) -> Div {
        let text = match self.source.text.trim() {
            "" => EMPTY_PREVIEW.to_owned(),
            text => text.to_owned(),
        };
        v_flex()
            .w_full()
            .px(px(10.))
            .py(px(6.))
            .gap(px(2.))
            .border_l(px(3.))
            .border_color(theme::accent())
            .rounded_r(px(4.))
            .bg(theme::quote_fill())
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_baseline()
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text_soft())
                            .child(self.source.author.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(theme::text_muted())
                            .child(self.source.time.clone()),
                    ),
            )
            .child(
                div()
                    .line_clamp(PREVIEW_LINES)
                    .text_size(px(12.5))
                    .text_color(theme::text_muted())
                    .child(text),
            )
    }
}

impl Render for ForwardDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let enabled = can_forward(self.target.as_ref(), self.sending);
        div()
            .id("forward-layer")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("forward-backdrop")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(black().opacity(BACKDROP_OPACITY))
                    .occlude()
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ForwardDialogEvent::Close))),
            )
            .child(
                v_flex()
                    .id("forward-card")
                    .w(px(CARD_WIDTH))
                    .max_w(relative(1.))
                    .max_h(relative(1.))
                    .m(px(CARD_MARGIN))
                    .p(px(CARD_PADDING))
                    .gap(px(SECTION_GAP))
                    .rounded(px(CARD_RADIUS))
                    .bg(theme::background())
                    .border_1()
                    .border_color(theme::border_strong())
                    .text_color(theme::text())
                    .shadow_lg()
                    .occlude()
                    .on_key_down(cx.listener(Self::on_key_down))
                    .child(self.header(cx))
                    .child(self.target_field(cx))
                    .child(self.list(cx))
                    .child(self.preview())
                    .child(Input::new(&self.comment))
                    .children(self.error.clone().map(|error| {
                        div()
                            .text_size(px(12.))
                            .text_color(theme::red_tint())
                            .child(error)
                    }))
                    .child(
                        h_flex()
                            .w_full()
                            .justify_end()
                            .gap(px(8.))
                            .child(
                                Button::new("forward-cancel")
                                    .ghost()
                                    .label("Cancel")
                                    .on_click(cx.listener(|_, _, _, cx| {
                                        cx.emit(ForwardDialogEvent::Close)
                                    })),
                            )
                            .child(self.send_button(enabled, cx)),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_RESULTS, can_forward, visible_candidates};
    use crate::app_state::Selection;
    use crate::views::switcher::{Candidate, CandidateAvatar};

    fn candidates(titles: &[&str]) -> Vec<(String, Candidate)> {
        titles
            .iter()
            .map(|title| {
                (
                    (*title).to_owned(),
                    Candidate {
                        title: (*title).to_owned(),
                        subtitle: "Chat".to_owned(),
                        avatar: CandidateAvatar::Team {
                            name: (*title).to_owned(),
                            key: (*title).to_owned(),
                        },
                        selection: Selection::Chat(format!("id-{title}")),
                    },
                )
            })
            .collect()
    }

    fn titles(found: &[Candidate]) -> Vec<&str> {
        found
            .iter()
            .map(|candidate| candidate.title.as_str())
            .collect()
    }

    #[test]
    fn empty_query_keeps_the_recent_order_and_caps_the_list() {
        let many: Vec<String> = (0..12).map(|number| format!("Chat {number:02}")).collect();
        let names: Vec<&str> = many.iter().map(String::as_str).collect();
        let found = visible_candidates(&candidates(&names), "");
        assert_eq!(found.len(), MAX_RESULTS);
        assert_eq!(titles(&found)[0], "Chat 00");
        assert_eq!(titles(&found)[7], "Chat 07");
    }

    #[test]
    fn a_query_keeps_only_matching_targets() {
        let found = visible_candidates(&candidates(&["Release", "Mara", "Atlas"]), "rel");
        assert_eq!(titles(&found), vec!["Release"]);
        assert!(visible_candidates(&candidates(&["Release"]), "zzz").is_empty());
    }

    #[test]
    fn forwarding_needs_a_target_and_no_send_in_flight() {
        let all = candidates(&["Release"]);
        let target = &all[0].1;
        assert!(!can_forward(None, false));
        assert!(can_forward(Some(target), false));
        assert!(!can_forward(Some(target), true));
    }
}
