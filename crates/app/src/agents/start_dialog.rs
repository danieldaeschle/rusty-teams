use agents::AgentAction;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, h_flex,
    input::{Textarea, TextareaState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{mcp_bridge, message_context, start_run, start_run_when_mcp_ready};
use crate::app_state::AppState;
use crate::theme;
use crate::views::widgets::{icon, symbol};

const CARD_WIDTH: f32 = 520.;
const CARD_RADIUS: f32 = 10.;
const CARD_PADDING: f32 = 16.;
const CARD_MARGIN: f32 = 32.;
const SECTION_GAP: f32 = 12.;
const BACKDROP_OPACITY: f32 = 0.55;
const CLOSE_SIZE: f32 = 28.;
const BUTTON_HEIGHT: f32 = 32.;
const QUOTE_LINES: usize = 3;
const FIELD_MIN_ROWS: usize = 3;
const FIELD_MAX_ROWS: usize = 6;
const PROMPT_SIZE: f32 = 12.;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartRequest {
    pub action_id: String,
    pub conversation_id: String,
    pub message_id: String,
}

pub enum StartDialogEvent {
    Close,
}

pub struct StartDialog {
    app: Entity<AppState>,
    request: StartRequest,
    action: AgentAction,
    subtitle: String,
    sender: String,
    quote: String,
    needs_allow: bool,
    input: Entity<TextareaState>,
    show_prompt: bool,
}

impl EventEmitter<StartDialogEvent> for StartDialog {}

pub fn needs_dialog(action: &AgentAction, chat_allowed: bool) -> bool {
    action.ask_extra_instructions || !chat_allowed
}

impl StartDialog {
    pub fn new(
        app: Entity<AppState>,
        request: StartRequest,
        action: AgentAction,
        agent_name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let context =
            message_context::build(app.read(cx), &request.conversation_id, &request.message_id);
        let (sender, quote) = context
            .map(|context| (context.sender, context.message))
            .unwrap_or_default();
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(FIELD_MIN_ROWS, FIELD_MAX_ROWS)
                .placeholder("e.g. only the backend, open a PR when done")
        });
        input.update(cx, |input, cx| input.focus(window, cx));
        StartDialog {
            app,
            subtitle: format!("{agent_name} - {}", action.working_dir),
            needs_allow: !mcp_bridge::chat_allowed(&request.conversation_id, cx),
            request,
            action,
            sender,
            quote,
            input,
            show_prompt: false,
        }
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        let extra = self.input.read(cx).value().trim().to_owned();
        if self.needs_allow {
            mcp_bridge::allow_chat(&self.request.conversation_id, cx);
        }
        if self.needs_allow && mcp_bridge::endpoint(cx).is_none() {
            start_run_when_mcp_ready(
                self.request.action_id.clone(),
                self.request.conversation_id.clone(),
                self.request.message_id.clone(),
                extra,
                cx,
            );
        } else {
            start_run(
                &self.request.action_id,
                &self.request.conversation_id,
                &self.request.message_id,
                &extra,
                cx,
            );
        }
        cx.emit(StartDialogEvent::Close);
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => cx.emit(StartDialogEvent::Close),
            "enter" if event.keystroke.modifiers.control => self.start(cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    fn header(&self, cx: &mut Context<Self>) -> Div {
        h_flex()
            .w_full()
            .items_start()
            .gap(px(SECTION_GAP))
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::BOLD)
                            .child(self.action.label.clone()),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(theme::text_muted())
                            .child(self.subtitle.clone()),
                    ),
            )
            .child(
                div()
                    .id("agent-dialog-close")
                    .size(px(CLOSE_SIZE))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .hover(|button| button.bg(theme::popup_hover()))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(StartDialogEvent::Close)))
                    .child(symbol("close", 16., theme::text_muted())),
            )
    }

    fn quote(&self) -> impl IntoElement {
        let text = format!("{} - {}", self.sender, self.quote);
        let sender_style = HighlightStyle {
            color: Some(theme::text()),
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        };
        let sender_range = 0..self.sender.len();
        div()
            .w_full()
            .px(px(12.))
            .py(px(8.))
            .rounded(px(8.))
            .bg(theme::reply_surface())
            .text_size(px(12.))
            .line_height(relative(1.4))
            .text_color(theme::text_soft())
            .line_clamp(QUOTE_LINES)
            .child(StyledText::new(text).with_highlights([(sender_range, sender_style)]))
    }

    fn field(&self, window: &Window, cx: &App) -> Div {
        let focused = self.input.focus_handle(cx).contains_focused(window, cx);
        v_flex()
            .w_full()
            .gap(px(6.))
            .child(
                div()
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text_faint())
                    .child("Extra instructions (optional)"),
            )
            .child(
                div()
                    .w_full()
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(8.))
                    .bg(theme::surface_raised())
                    .border_1()
                    .border_color(if focused {
                        theme::accent()
                    } else {
                        theme::surface_raised()
                    })
                    .child(Textarea::new(&self.input).appearance(false).bordered(false)),
            )
    }

    fn prompt_toggle(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        h_flex()
            .id("agent-dialog-prompt-toggle")
            .gap(px(4.))
            .items_center()
            .cursor_pointer()
            .text_size(px(12.))
            .text_color(theme::text_muted())
            .hover(|toggle| toggle.text_color(theme::text_soft()))
            .child(icon(
                if self.show_prompt {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                },
                12.,
                theme::text_muted(),
            ))
            .child("Show prompt")
            .on_click(cx.listener(|this, _, _, cx| {
                this.show_prompt = !this.show_prompt;
                cx.notify();
            }))
    }

    fn prompt_box(&self, cx: &mut Context<Self>) -> Div {
        let extra = self.input.read(cx).value().trim().to_owned();
        let prompt = message_context::preview_prompt(
            self.app.read(cx),
            &self.action,
            &self.request.conversation_id,
            &self.request.message_id,
            &extra,
        )
        .unwrap_or_default();
        div()
            .w_full()
            .max_h(px(160.))
            .px(px(12.))
            .py(px(8.))
            .rounded(px(8.))
            .bg(theme::reply_surface())
            .overflow_hidden()
            .font_family(cx.theme().mono_font_family.clone())
            .text_size(px(PROMPT_SIZE))
            .line_height(relative(1.5))
            .text_color(theme::text_muted())
            .child(prompt)
    }

    fn buttons(&self, cx: &mut Context<Self>) -> Div {
        let start_label = if self.needs_allow {
            "Allow and start"
        } else {
            "Start"
        };
        h_flex()
            .w_full()
            .items_center()
            .gap(px(8.))
            .child(self.prompt_toggle(cx))
            .child(div().flex_1())
            .child(
                h_flex()
                    .id("agent-dialog-cancel")
                    .h(px(BUTTON_HEIGHT))
                    .px(px(12.))
                    .items_center()
                    .rounded(px(8.))
                    .cursor_pointer()
                    .bg(theme::surface_raised())
                    .hover(|button| button.bg(theme::popup_hover()))
                    .text_size(px(13.))
                    .child("Cancel")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(StartDialogEvent::Close))),
            )
            .child(
                h_flex()
                    .id("agent-dialog-start")
                    .h(px(BUTTON_HEIGHT))
                    .px(px(12.))
                    .items_center()
                    .rounded(px(8.))
                    .cursor_pointer()
                    .bg(theme::accent())
                    .hover(|button| button.bg(theme::accent_text()))
                    .text_color(theme::on_accent())
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(start_label)
                    .on_click(cx.listener(|this, _, _, cx| this.start(cx))),
            )
    }
}

impl Render for StartDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("agent-dialog-layer")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("agent-dialog-backdrop")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(black().opacity(BACKDROP_OPACITY))
                    .occlude()
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(StartDialogEvent::Close))),
            )
            .child(
                v_flex()
                    .id("agent-dialog-card")
                    .w(px(CARD_WIDTH))
                    .max_w(relative(1.))
                    .max_h(relative(1.))
                    .m(px(CARD_MARGIN))
                    .p(px(CARD_PADDING))
                    .gap(px(SECTION_GAP))
                    .rounded(px(CARD_RADIUS))
                    .bg(theme::popup())
                    .text_color(theme::text())
                    .occlude()
                    .capture_key_down(cx.listener(Self::on_key_down))
                    .child(self.header(cx))
                    .child(self.quote())
                    .when(self.needs_allow, |card| {
                        card.child(
                            div()
                                .text_size(px(12.))
                                .text_color(theme::text_soft())
                                .child("Agents can't see this chat yet."),
                        )
                    })
                    .child(self.field(window, cx))
                    .when(self.show_prompt, |card| card.child(self.prompt_box(cx)))
                    .child(self.buttons(cx)),
            )
    }
}

#[cfg(test)]
mod tests {
    use agents::AgentAction;

    use super::needs_dialog;

    #[test]
    fn the_dialog_shows_for_extra_instructions_or_a_chat_agents_cannot_see() {
        let quiet = AgentAction::answer_this("claude");
        let asking = AgentAction::build_this("claude");
        assert!(!needs_dialog(&quiet, true));
        assert!(needs_dialog(&quiet, false));
        assert!(needs_dialog(&asking, true));
    }
}
