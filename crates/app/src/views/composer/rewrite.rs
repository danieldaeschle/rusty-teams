use gpui_kit::component::fluent::{
    MENU_ITEM_GAP, MENU_ITEM_HEIGHT, MENU_ITEM_PADDING_X, MENU_LIST_PADDING,
};
use gpui_kit::component::input::{Input, InputContent, InputEvent, InputState};
use gpui_kit::component::tooltip::PopupTriggerTooltip as _;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::{ComposeAction, ComposeAdjustment, Draft, OBJECT_MARK, TABLE_MARK, html_to_spans};

use super::{COMPOSER_BUTTON_SIZE, COMPOSER_ICON_SIZE, Composer};
use crate::runtime;
use crate::theme::{self, ElevatedSurface as _};
use crate::views::widgets::symbol;
use app_ui_kit::spinner::spinner;

pub const REWRITE_BUTTON_TAG: &str = "composer/actions/copilot";
pub const REWRITE_PANEL_TAG: &str = "composer/copilot";
const BUTTON_LABEL: &str = "Write with Copilot";
const REWRITE_SYSTEM: &str = "You rewrite a Teams chat draft. Reply with only the new message text. Keep the meaning. Do not add a preamble. Write in the same language as the draft.";
const SWITCH_HINT: &str = "Choose another model under Settings > Agents, Write with.";
const CLOSE_LABEL: &str = "Close";
const ADJUST_LABEL: &str = "Adjust";
const CUSTOM_LABEL: &str = "Customised";
const CUSTOM_PLACEHOLDER: &str = "Describe how to adjust it, e.g. make it sound friendlier";
const REPLACE_LABEL: &str = "Replace";
const DISCLAIMER: &str = "AI-generated content may be incorrect";
const COPILOT_NAME: &str = "Copilot";
const BUTTON_ICON: &str = "auto_fix_high";
const LOCAL_BUTTON_ICON: &str = "smart_toy";
const PANEL_WIDTH: f32 = 600.;
const PANEL_PADDING: f32 = 16.;
const PANEL_RADIUS: f32 = 10.;
const CARD_RADIUS: f32 = 8.;
const CHIP_HEIGHT: f32 = 32.;
const CHIP_ICON: f32 = 16.;
const ADJUST_MENU_WIDTH: f32 = 200.;
const BUTTON_MIN_COMPOSER_WIDTH: f32 = 320.;
const DEMO_DELAY: std::time::Duration = std::time::Duration::from_millis(700);

#[derive(Default)]
pub(super) struct RewritePanel {
    open: bool,
    adjust_menu: bool,
    result: Option<RewriteResult>,
    generation: u64,
    custom_input: Option<Entity<InputState>>,
    custom_open: bool,
    _custom_events: Option<Subscription>,
}

impl RewritePanel {
    pub(super) fn input_focused(&self, window: &Window, cx: &App) -> bool {
        self.custom_input
            .as_ref()
            .is_some_and(|input| input.read(cx).focus_handle(cx).is_focused(window))
    }
}

struct RewriteResult {
    action: ComposeAction,
    answer: RewriteAnswer,
}

enum RewriteAnswer {
    Waiting,
    Ready(Draft),
    Failed(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Choice {
    Rewrite,
    Proofread,
}

impl Composer {
    fn rewrite_available(&self, cx: &App) -> bool {
        let state = self.app.read(cx);
        self.editing.is_none()
            && (state.mode.demo || local_writer(cx).is_some() || state.engine.is_some())
            && self
                .conversation_id
                .as_deref()
                .is_some_and(|conversation_id| {
                    state
                        .sidebar
                        .chats
                        .iter()
                        .any(|chat| chat.id == conversation_id)
                })
    }

    pub(super) fn render_rewrite_button(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let fits = self
            .composer_bounds
            .get()
            .is_none_or(|bounds| f32::from(bounds.size.width) >= BUTTON_MIN_COMPOSER_WIDTH);
        if !fits || !self.rewrite_available(cx) {
            return None;
        }
        let open = self.rewrite.open;
        let writer = local_writer(cx);
        let local = writer.is_some();
        let tooltip = writer
            .as_ref()
            .map(|profile| format!("Rewrite with {}", profile.name))
            .unwrap_or_else(|| BUTTON_LABEL.to_owned());
        Some(
            div()
                .id("composer-copilot")
                .debug_selector(|| REWRITE_BUTTON_TAG.into())
                .size(px(COMPOSER_BUTTON_SIZE))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .cursor_pointer()
                .when(open, |button| button.bg(theme::popup_hover()))
                .hover(|button| button.bg(theme::popup_hover()))
                .popup_trigger_tooltip_text(open, tooltip)
                .child(symbol(
                    if local {
                        LOCAL_BUTTON_ICON
                    } else {
                        BUTTON_ICON
                    },
                    COMPOSER_ICON_SIZE,
                    if open {
                        theme::accent_text()
                    } else {
                        theme::text_muted()
                    },
                ))
                .on_mouse_down(MouseButton::Left, |_, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                })
                .on_click(cx.listener(|this, _, _, cx| {
                    this.rewrite.open = !this.rewrite.open;
                    this.rewrite.adjust_menu = false;
                    cx.notify();
                })),
        )
    }

    pub(super) fn close_rewrite(&mut self) -> bool {
        let was_open = self.rewrite.open;
        self.rewrite.open = false;
        self.rewrite.adjust_menu = false;
        self.rewrite.custom_open = false;
        self.rewrite.result = None;
        self.rewrite.generation += 1;
        was_open
    }

    fn request_rewrite(&mut self, action: ComposeAction, cx: &mut Context<Self>) {
        self.rewrite.adjust_menu = false;
        self.rewrite.generation += 1;
        let generation = self.rewrite.generation;
        let draft = self.current_draft(cx);
        let demo = self.app.read(cx).mode.demo;
        let engine = self.app.read(cx).engine.clone();
        let Some(chat_id) = self.conversation_id.clone() else {
            return;
        };
        let writer = writer_display_name(cx);
        let failed = failed_rewrite(&writer);
        let refusal = if draft.is_blank() {
            Some(empty_draft_message(&writer))
        } else if draft.text().contains([OBJECT_MARK, TABLE_MARK]) {
            Some(text_only_message(&writer))
        } else {
            None
        };
        if let Some(refusal) = refusal {
            self.rewrite.result = Some(RewriteResult {
                action,
                answer: RewriteAnswer::Failed(refusal),
            });
            cx.notify();
            return;
        }
        self.rewrite.result = Some(RewriteResult {
            action: action.clone(),
            answer: RewriteAnswer::Waiting,
        });
        cx.notify();
        let user_html = draft.to_html();
        let plain = draft.plain_text();
        if !demo && let Some(profile) = local_writer(cx) {
            let instruction = rewrite_instruction(&action, &plain);
            let receiver = runtime::spawn(async move {
                agents::complete_text(&profile, REWRITE_SYSTEM, &instruction).await
            });
            cx.spawn(async move |this, cx| {
                let answer = match receiver.await {
                    Ok(Ok(text)) => RewriteAnswer::Ready(Draft::from_markdown(&text)),
                    Ok(Err(error)) => RewriteAnswer::Failed(format!("{failed} {error}")),
                    Err(error) => RewriteAnswer::Failed(format!("{failed} {error}")),
                };
                this.update(cx, |this, cx| {
                    if this.rewrite.generation == generation
                        && let Some(result) = this.rewrite.result.as_mut()
                    {
                        result.answer = answer;
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
            return;
        }
        let receiver = (!demo).then_some(engine).flatten().map(|engine| {
            runtime::spawn(async move { engine.rewrite_draft(&chat_id, &user_html, action).await })
        });
        let demo_answer = demo_rewrite(&draft.plain_text());
        cx.spawn(async move |this, cx| {
            let answer = match receiver {
                None => {
                    cx.background_executor().timer(DEMO_DELAY).await;
                    RewriteAnswer::Ready(Draft::from_markdown(&demo_answer))
                }
                Some(receiver) => match receiver.await {
                    Ok(Ok(html)) => {
                        RewriteAnswer::Ready(Draft::from_spans(&html_to_spans(&html)).trimmed())
                    }
                    Ok(Err(error)) => {
                        RewriteAnswer::Failed(format!("{failed} {error} {SWITCH_HINT}"))
                    }
                    Err(error) => RewriteAnswer::Failed(format!("{failed} {error} {SWITCH_HINT}")),
                },
            };
            this.update(cx, |this, cx| {
                if this.rewrite.generation == generation
                    && let Some(result) = this.rewrite.result.as_mut()
                {
                    result.answer = answer;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn replace_with_rewrite(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(RewriteAnswer::Ready(draft)) =
            self.rewrite.result.as_ref().map(|result| &result.answer)
        else {
            return;
        };
        let draft = draft.clone();
        self.close_rewrite();
        self.load_draft(
            draft.clone(),
            InputContent::new(draft.text().to_owned()),
            window,
            cx,
        );
        self.mention_inputs.clear();
        self.schedule_draft_save(cx);
        self.focus(window, cx);
        cx.notify();
    }

    fn open_custom(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.rewrite.adjust_menu = false;
        self.rewrite.custom_open = true;
        if self.rewrite.custom_input.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(CUSTOM_PLACEHOLDER));
            let subscription = cx.subscribe_in(
                &input,
                window,
                |this, input, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        let prompt = input.read(cx).value().trim().to_owned();
                        if !prompt.is_empty() {
                            input.update(cx, |input, cx| input.set_value("", window, cx));
                            this.rewrite.custom_open = false;
                            this.request_rewrite(ComposeAction::Custom(prompt), cx);
                        }
                    }
                },
            );
            self.rewrite.custom_input = Some(input);
            self.rewrite._custom_events = Some(subscription);
        }
        if let Some(input) = &self.rewrite.custom_input {
            input.update(cx, |input, cx| input.focus(window, cx));
        }
        cx.notify();
    }

    pub(super) fn render_rewrite_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.rewrite.open || !self.rewrite_available(cx) {
            return None;
        }
        let busy = matches!(
            self.rewrite.result.as_ref().map(|result| &result.answer),
            Some(RewriteAnswer::Waiting)
        );
        let chips = h_flex()
            .w_full()
            .gap(px(8.))
            .items_center()
            .child(self.choice_chip(Choice::Rewrite, busy, cx))
            .child(self.choice_chip(Choice::Proofread, busy, cx))
            .child(
                div()
                    .relative()
                    .child(self.adjust_chip(busy, cx))
                    .children(self.render_adjust_menu(cx)),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("composer-copilot-close")
                    .size(px(CHIP_HEIGHT))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|button| button.bg(theme::popup_hover()))
                    .popup_trigger_tooltip_text(false, CLOSE_LABEL)
                    .child(symbol("close", CHIP_ICON, theme::text_soft()))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_rewrite();
                        cx.notify();
                    })),
            );
        let custom = self
            .rewrite
            .custom_open
            .then(|| self.rewrite.custom_input.clone())
            .flatten()
            .map(|input| {
                div()
                    .w_full()
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(CARD_RADIUS))
                    .bg(theme::surface_raised())
                    .child(Input::new(&input).appearance(false).bordered(false))
            });
        Some(
            v_flex()
                .id(REWRITE_PANEL_TAG)
                .debug_selector(|| REWRITE_PANEL_TAG.into())
                .absolute()
                .bottom(relative(1.))
                .right_0()
                .mb(px(6.))
                .w(px(PANEL_WIDTH))
                .max_w(relative(1.))
                .p(px(PANEL_PADDING))
                .gap(px(12.))
                .rounded(px(PANEL_RADIUS))
                .flyout_surface()
                .occlude()
                .child(chips)
                .children(custom)
                .children(self.render_rewrite_result(cx))
                .into_any_element(),
        )
    }

    fn chip(
        id: &'static str,
        icon: &'static str,
        label: &'static str,
        busy: bool,
    ) -> Stateful<Div> {
        h_flex()
            .id(id)
            .h(px(CHIP_HEIGHT))
            .px(px(12.))
            .gap(px(6.))
            .items_center()
            .rounded(px(6.))
            .bg(theme::surface_raised())
            .text_size(px(13.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme::text())
            .child(symbol(icon, CHIP_ICON, theme::text_soft()))
            .child(label)
            .when(busy, |chip| chip.opacity(0.5))
            .when(!busy, |chip| {
                chip.cursor_pointer()
                    .hover(|chip| chip.bg(theme::popup_hover()))
            })
    }

    fn choice_chip(&self, choice: Choice, busy: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let (id, icon, action) = match choice {
            Choice::Rewrite => (
                "composer-copilot-rewrite",
                BUTTON_ICON,
                ComposeAction::Rewrite,
            ),
            Choice::Proofread => (
                "composer-copilot-proofread",
                "spellcheck",
                ComposeAction::Proofread,
            ),
        };
        let label = match choice {
            Choice::Rewrite => "Rewrite",
            Choice::Proofread => "Proofread",
        };
        Self::chip(id, icon, label, busy).when(!busy, |chip| {
            chip.on_click(
                cx.listener(move |this, _, _, cx| this.request_rewrite(action.clone(), cx)),
            )
        })
    }

    fn adjust_chip(&self, busy: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        Self::chip("composer-copilot-adjust", "more_horiz", ADJUST_LABEL, busy)
            .child(symbol("keyboard_arrow_down", CHIP_ICON, theme::text_soft()))
            .when(!busy, |chip| {
                chip.on_click(cx.listener(|this, _, _, cx| {
                    this.rewrite.adjust_menu = !this.rewrite.adjust_menu;
                    cx.notify();
                }))
            })
    }

    fn render_adjust_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.rewrite.adjust_menu {
            return None;
        }
        let item = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .w_full()
                .h(MENU_ITEM_HEIGHT)
                .px(MENU_ITEM_PADDING_X)
                .flex()
                .items_center()
                .rounded(px(6.))
                .cursor_pointer()
                .hover(|row| row.bg(theme::popup_hover()))
                .text_size(px(13.))
                .text_color(theme::text())
                .child(label)
        };
        let custom = item("composer-copilot-custom", CUSTOM_LABEL)
            .on_click(cx.listener(|this, _, window, cx| this.open_custom(window, cx)));
        let adjustments = ComposeAdjustment::ALL.into_iter().map(|adjustment| {
            item(adjustment.label(), adjustment.label()).on_click(cx.listener(
                move |this, _, _, cx| this.request_rewrite(ComposeAction::Adjust(adjustment), cx),
            ))
        });
        Some(
            v_flex()
                .id("composer-copilot-adjust-menu")
                .absolute()
                .bottom(relative(1.))
                .left_0()
                .mb(px(4.))
                .w(px(ADJUST_MENU_WIDTH))
                .p(MENU_LIST_PADDING)
                .gap(MENU_ITEM_GAP)
                .rounded(px(10.))
                .flyout_surface()
                .occlude()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.rewrite.adjust_menu = false;
                    cx.notify();
                }))
                .child(custom)
                .children(adjustments)
                .into_any_element(),
        )
    }

    fn render_rewrite_result(&self, cx: &mut Context<Self>) -> Option<Div> {
        let result = self.rewrite.result.as_ref()?;
        let card = v_flex()
            .w_full()
            .p(px(PANEL_PADDING))
            .gap(px(12.))
            .rounded(px(CARD_RADIUS))
            .bg(theme::surface_raised())
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme::text_soft())
                    .child(result.action.label().to_owned()),
            );
        Some(match &result.answer {
            RewriteAnswer::Waiting => card.child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(spinner(16.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .text_color(theme::text_soft())
                            .child(working_message(&writer_display_name(cx))),
                    ),
            ),
            RewriteAnswer::Failed(reason) => card.child(
                div()
                    .text_size(px(14.))
                    .text_color(theme::text_soft())
                    .child(reason.clone()),
            ),
            RewriteAnswer::Ready(draft) => card
                .child(
                    div()
                        .text_size(px(14.))
                        .text_color(theme::text())
                        .child(draft.plain_text()),
                )
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .id("composer-copilot-replace")
                                .h(px(CHIP_HEIGHT))
                                .px(px(24.))
                                .flex()
                                .items_center()
                                .rounded(px(6.))
                                .bg(theme::accent())
                                .text_color(theme::on_accent())
                                .text_size(px(14.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .cursor_pointer()
                                .hover(|button| button.opacity(0.85))
                                .child(REPLACE_LABEL)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.replace_with_rewrite(window, cx)
                                })),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(theme::text_muted())
                                .child(DISCLAIMER),
                        ),
                ),
        })
    }
}

fn local_writer(cx: &App) -> Option<agents::AgentProfile> {
    crate::agents::local_writer(cx)
}

fn writer_display_name(cx: &App) -> String {
    local_writer(cx)
        .map(|profile| profile.name)
        .unwrap_or_else(|| COPILOT_NAME.to_owned())
}

fn working_message(name: &str) -> String {
    format!("{name} is writing ...")
}

fn failed_rewrite(name: &str) -> String {
    format!("{name} could not rewrite this:")
}

fn empty_draft_message(name: &str) -> String {
    format!("Type a message first, then let {name} rewrite it.")
}

fn text_only_message(name: &str) -> String {
    format!("{name} rewrites text only. Move images and tables out of the message first.")
}

fn rewrite_instruction(action: &ComposeAction, draft: &str) -> String {
    let ask = match action {
        ComposeAction::Rewrite => "Rewrite it so it is clearer.",
        ComposeAction::Proofread => "Proofread it. Fix spelling and grammar.",
        ComposeAction::Adjust(adjustment) => match adjustment {
            ComposeAdjustment::Concise => "Make it shorter.",
            ComposeAdjustment::Longer => "Make it a bit longer.",
            ComposeAdjustment::Casual => "Make it more casual.",
            ComposeAdjustment::Professional => "Make it more professional.",
            ComposeAdjustment::Confident => "Make it more confident.",
            ComposeAdjustment::Enthusiastic => "Make it more enthusiastic.",
        },
        ComposeAction::Custom(prompt) => prompt.as_str(),
    };
    format!("{ask}\n\n{draft}")
}

fn demo_rewrite(text: &str) -> String {
    let trimmed = text.trim().trim_end_matches(['.', '!', '?']);
    let mut characters = trimmed.chars();
    let capitalized: String = characters
        .next()
        .map(|first| first.to_uppercase().chain(characters).collect())
        .unwrap_or_default();
    format!("{capitalized}.")
}

#[cfg(test)]
mod tests {
    use super::demo_rewrite;

    #[test]
    fn the_demo_answer_tidies_the_draft() {
        assert_eq!(demo_rewrite(" hallo zusammen!! "), "Hallo zusammen.");
    }
}
