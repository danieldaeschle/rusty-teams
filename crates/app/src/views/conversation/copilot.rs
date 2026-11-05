use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Disableable as _, Selectable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::ConversationView;
use crate::app_state::Selection;
use crate::render::{layout_card_blocks, render_blocks};
use crate::runtime;
use crate::theme;
use crate::views::scroll_edge::with_top_edge;

pub const COPILOT_PANE_TAG: &str = "shell/islands/main/copilot";
const COPILOT_SCROLL_DIVIDER: &str = "shell/islands/main/copilot/divider-scroll";
const TITLE: &str = "Copilot";
const OPEN_LABEL: &str = "Open Copilot";
const CLOSE_LABEL: &str = "Close Copilot";
const PLACEHOLDER: &str = "Message Copilot";
const WORKING: &str = "Copilot is reading the chat ...";
const UNAVAILABLE: &str = "Copilot needs a signed-in account.";
const INTRO: &str = "Ask Copilot about this chat.";
const SUGGESTIONS: [&str; 3] = [
    "Summarize this chat",
    "What are open items?",
    "What decisions were made?",
];
const PANE_WIDTH: f32 = 360.;
const PANE_PADDING: f32 = 16.;
const HEADER_HEIGHT: f32 = 48.;
const BUTTON_SIZE: f32 = 32.;
const BUBBLE_RADIUS: f32 = 8.;

#[derive(Default)]
pub(super) struct CopilotPane {
    open: bool,
    turns: Vec<CopilotTurn>,
    input: Option<Entity<InputState>>,
    scroll: ScrollHandle,
    generation: u64,
    _input_events: Option<Subscription>,
}

struct CopilotTurn {
    prompt: String,
    answer: CopilotAnswer,
}

enum CopilotAnswer {
    Waiting,
    Ready(String),
    Failed(String),
}

impl ConversationView {
    fn copilot_chat_id(&self) -> Option<String> {
        match &self.current.as_ref()?.selection {
            Selection::Chat(chat_id) => Some(chat_id.clone()),
            Selection::Channel(_) => None,
        }
    }

    pub(super) fn copilot_button(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.copilot_chat_id()?;
        let open = self.copilot.open;
        Some(
            Button::new("copilot-toggle")
                .icon(IconName::Sparkles)
                .ghost()
                .size(px(BUTTON_SIZE))
                .rounded(px(8.))
                .when(open, |button| button.selected(true))
                .tooltip(if open { CLOSE_LABEL } else { OPEN_LABEL })
                .on_click(cx.listener(|this, _, window, cx| this.toggle_copilot(window, cx)))
                .into_any_element(),
        )
    }

    fn toggle_copilot(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.copilot.open = !self.copilot.open;
        if self.copilot.open && self.copilot.input.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(PLACEHOLDER));
            let subscription = cx.subscribe_in(&input, window, |this, input, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    let prompt = input.read(cx).value().trim().to_owned();
                    if !prompt.is_empty() {
                        input.update(cx, |input, cx| input.set_value("", window, cx));
                        this.ask_copilot(prompt, cx);
                    }
                }
            });
            self.copilot.input = Some(input);
            self.copilot._input_events = Some(subscription);
        }
        if self.copilot.open {
            self.channel_details_open = false;
            if let Some(input) = &self.copilot.input {
                input.update(cx, |input, cx| input.focus(window, cx));
            }
        }
        cx.notify();
    }

    pub(super) fn reset_copilot(&mut self) {
        let input = self.copilot.input.take();
        let events = self.copilot._input_events.take();
        self.copilot = CopilotPane {
            input,
            generation: self.copilot.generation + 1,
            _input_events: events,
            ..CopilotPane::default()
        };
    }

    fn ask_copilot(&mut self, prompt: String, cx: &mut Context<Self>) {
        let Some(chat_id) = self.copilot_chat_id() else {
            return;
        };
        let index = self.copilot.turns.len();
        let generation = self.copilot.generation;
        self.copilot.turns.push(CopilotTurn {
            prompt: prompt.clone(),
            answer: CopilotAnswer::Waiting,
        });
        self.copilot.scroll.scroll_to_bottom();
        cx.notify();
        let Some(engine) = self.app.read(cx).engine.clone() else {
            self.settle_copilot(generation, index, Err(UNAVAILABLE.to_owned()), cx);
            return;
        };
        let receiver = runtime::spawn(async move { engine.ask_copilot(&chat_id, &prompt).await });
        cx.spawn(async move |this, cx| {
            let answer = match receiver.await {
                Ok(Ok(text)) => Ok(text),
                Ok(Err(error)) => Err(error.to_string()),
                Err(error) => Err(error.to_string()),
            };
            this.update(cx, |this, cx| this.settle_copilot(generation, index, answer, cx))
                .ok();
        })
        .detach();
    }

    fn settle_copilot(
        &mut self,
        generation: u64,
        index: usize,
        answer: Result<String, String>,
        cx: &mut Context<Self>,
    ) {
        if generation != self.copilot.generation {
            return;
        }
        let Some(turn) = self.copilot.turns.get_mut(index) else {
            return;
        };
        turn.answer = match answer {
            Ok(text) => CopilotAnswer::Ready(text),
            Err(error) => CopilotAnswer::Failed(error),
        };
        self.copilot.scroll.scroll_to_bottom();
        cx.notify();
    }

    pub(super) fn render_copilot(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.copilot.open {
            return None;
        }
        self.copilot_chat_id()?;
        let mut body = v_flex()
            .w_full()
            .px(px(PANE_PADDING))
            .pb(px(PANE_PADDING))
            .gap(px(12.));
        if self.copilot.turns.is_empty() {
            body = body.child(
                div()
                    .text_size(px(13.))
                    .text_color(theme::text_muted())
                    .child(INTRO),
            );
        }
        for (index, turn) in self.copilot.turns.iter().enumerate() {
            body = body.child(prompt_bubble(&turn.prompt)).child(answer_view(index, &turn.answer, cx));
        }
        let busy = self
            .copilot
            .turns
            .last()
            .is_some_and(|turn| matches!(turn.answer, CopilotAnswer::Waiting));
        let suggestions = h_flex().flex_wrap().gap(px(6.)).children(
            SUGGESTIONS.iter().enumerate().map(|(index, prompt)| {
                Button::new(("copilot-suggestion", index))
                    .label(*prompt)
                    .outline()
                    .rounded(px(BUBBLE_RADIUS))
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.ask_copilot((*prompt).to_owned(), cx)
                    }))
            }),
        );
        let footer = v_flex()
            .flex_none()
            .px(px(PANE_PADDING))
            .py(px(12.))
            .gap(px(8.))
            .child(suggestions)
            .children(self.copilot.input.as_ref().map(|input| Input::new(input)));
        Some(
            h_flex()
                .h_full()
                .flex_none()
                .child(div().w(px(1.)).h_full().flex_none().bg(theme::border()))
                .child(
                    v_flex()
                        .debug_selector(|| COPILOT_PANE_TAG.into())
                        .w(px(PANE_WIDTH))
                        .h_full()
                        .bg(theme::surface())
                        .child(
                            h_flex()
                                .h(px(HEADER_HEIGHT))
                                .flex_none()
                                .px(px(PANE_PADDING))
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_size(px(15.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme::text())
                                        .child(TITLE),
                                )
                                .child(
                                    Button::new("copilot-close")
                                        .icon(IconName::X)
                                        .ghost()
                                        .size(px(BUTTON_SIZE))
                                        .rounded(px(8.))
                                        .tooltip(CLOSE_LABEL)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.copilot.open = false;
                                            cx.notify();
                                        })),
                                ),
                        )
                        .child(with_top_edge(
                            div()
                                .id("copilot-body")
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scrollbar()
                                .track_scroll(&self.copilot.scroll)
                                .child(body),
                            &self.copilot.scroll,
                            COPILOT_SCROLL_DIVIDER,
                        ))
                        .child(footer),
                )
                .into_any_element(),
        )
    }
}

fn prompt_bubble(prompt: &str) -> Div {
    h_flex().w_full().justify_end().child(
        div()
            .max_w(px(PANE_WIDTH * 0.8))
            .px(px(10.))
            .py(px(6.))
            .rounded(px(BUBBLE_RADIUS))
            .bg(theme::row_hover())
            .text_size(px(13.))
            .text_color(theme::text())
            .child(prompt.to_owned()),
    )
}

fn answer_view(index: usize, answer: &CopilotAnswer, cx: &App) -> AnyElement {
    match answer {
        CopilotAnswer::Waiting => div()
            .text_size(px(13.))
            .text_color(theme::text_muted())
            .child(WORKING)
            .into_any_element(),
        CopilotAnswer::Failed(error) => div()
            .text_size(px(13.))
            .text_color(theme::red())
            .child(error.clone())
            .into_any_element(),
        CopilotAnswer::Ready(text) => {
            let spans = teams_core::html_to_spans(&teams_core::markdown_to_html(text));
            div()
                .w_full()
                .text_size(px(13.))
                .child(render_blocks(
                    &layout_card_blocks(&spans),
                    &format!("copilot-answer-{index}"),
                    false,
                    cx,
                ))
                .into_any_element()
        }
    }
}
