use std::time::Instant;

use gpui_kit::component::{
    h_flex,
    input::{InputEvent, Textarea, TextareaState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::center::NotificationCenter;
use super::layout::ACTION_ROW;
use super::platform;
use super::rules::ChatKind;
use super::stack::{ReplyState, ToastModel};
use super::text::{ToastText, describe, mention_ranges};
use crate::sidebar_model::{AvatarSpec, Face};
use crate::views::avatar::{spec_avatar, with_presence};
use crate::views::composer::submitted_text;
use crate::views::widgets::symbol;
use crate::theme;

const AVATAR_SIZE: f32 = 40.;
const CLOSE_SIZE: f32 = 24.;
const PROGRESS_HEIGHT: f32 = 3.;
const TOAST_BORDER_MENTION: f32 = 2.;

pub struct ToastView {
    center: Entity<NotificationCenter>,
    id: u64,
    input: Entity<TextareaState>,
    focus_handle: FocusHandle,
    last_title: String,
    _subscriptions: Vec<Subscription>,
}

impl ToastView {
    pub fn new(
        center: Entity<NotificationCenter>,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 4)
                .submit_on_enter(true)
                .placeholder("Antworten ...")
        });
        let subscriptions = vec![
            cx.subscribe_in(&input, window, |this, input, event: &InputEvent, window, cx| {
                let value = input.read(cx).value();
                if submitted_text(event, &value).is_some() {
                    this.submit(window, cx);
                }
            }),
            cx.observe(&center, |_, _, cx| cx.notify()),
        ];
        ToastView {
            center,
            id,
            input,
            focus_handle: cx.focus_handle(),
            last_title: String::new(),
            _subscriptions: subscriptions,
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().trim().to_owned();
        if text.is_empty() {
            return;
        }
        self.input
            .update(cx, |state, cx| state.set_value("", window, cx));
        let id = self.id;
        self.center
            .update(cx, |center, cx| center.submit_reply(id, text, cx));
    }

    fn open_reply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.id;
        let draft = self
            .center
            .update(cx, |center, cx| center.open_reply(id, cx));
        if let Some(native) = platform::native_handle(window) {
            platform::set_activatable(native, true);
        }
        let input = self.input.clone();
        cx.defer_in(window, move |_, window, cx| {
            input.update(cx, |state, cx| {
                if let Some(draft) = draft.filter(|draft| !draft.is_empty()) {
                    state.set_value(&draft, window, cx);
                }
                state.focus(window, cx);
            });
        });
    }

    fn close_reply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input
            .update(cx, |state, cx| state.set_value("", window, cx));
        let id = self.id;
        self.center
            .update(cx, |center, cx| center.close_reply(id, cx));
        window.focus(&self.focus_handle, cx);
    }

    fn retry(&mut self, cx: &mut Context<Self>) {
        let id = self.id;
        self.center.update(cx, |center, cx| {
            if let Some(text) = center.reply_text(id) {
                center.submit_reply(id, text, cx);
            }
        });
    }
}

fn pill_button(label: &'static str, symbol_name: &'static str, accent: bool) -> Stateful<Div> {
    let color = if accent {
        theme::accent_text()
    } else {
        theme::text_soft()
    };
    h_flex()
        .id(label)
        .h(px(ACTION_ROW))
        .px(px(14.))
        .gap(px(6.))
        .items_center()
        .justify_center()
        .rounded(px(8.))
        .border_1()
        .border_color(theme::border_strong())
        .cursor_pointer()
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(color)
        .hover(|button| button.bg(theme::row_hover()))
        .child(symbol(symbol_name, 18., color))
        .child(label)
}

fn highlighted_preview(text: &str, mention: bool) -> AnyElement {
    let mut styled = StyledText::new(text.to_owned());
    if mention {
        let style = HighlightStyle {
            color: Some(theme::accent_soft()),
            font_weight: Some(FontWeight::SEMIBOLD),
            ..Default::default()
        };
        let highlights: Vec<_> = mention_ranges(text)
            .into_iter()
            .map(|range| (range, style))
            .collect();
        styled = styled.with_highlights(highlights);
    }
    div()
        .text_size(px(13.))
        .line_height(px(18.))
        .text_color(theme::text())
        .line_clamp(2)
        .child(styled)
        .into_any_element()
}

fn side_chip(label: String, mention: bool) -> Div {
    div()
        .h(px(20.))
        .px(px(8.))
        .flex()
        .items_center()
        .rounded(px(10.))
        .bg(if mention {
            theme::bubble_own()
        } else {
            theme::border_strong()
        })
        .text_color(if mention {
            theme::accent_soft()
        } else {
            theme::text()
        })
        .text_size(px(11.))
        .line_height(px(20.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(label)
}

impl ToastView {
    fn avatar(&self, model: &ToastModel, fill: Hsla, cx: &App) -> AnyElement {
        let center = self.center.read(cx);
        let directory = center.directory(cx);
        let sender = Face {
            user_id: model.sender_id.clone(),
            name: model.sender_name.clone(),
        };
        match &model.kind {
            ChatKind::Group { .. } => {
                let chat = Face {
                    user_id: None,
                    name: model.chat_title.clone(),
                };
                spec_avatar(directory, &AvatarSpec::Pair(sender, chat), AVATAR_SIZE, fill)
            }
            _ => {
                let presence = model
                    .sender_id
                    .as_deref()
                    .map(|user_id| directory.presence_of(user_id))
                    .unwrap_or(crate::data::PresenceKind::Unknown);
                let face = spec_avatar(directory, &AvatarSpec::Single(sender), AVATAR_SIZE, fill);
                with_presence(face, presence, AVATAR_SIZE, fill).into_any_element()
            }
        }
    }

    fn side_slot(&self, model: &ToastModel, hovered: bool) -> AnyElement {
        let id = self.id;
        if hovered {
            let center = self.center.clone();
            return div()
                .id("toast-close")
                .size(px(CLOSE_SIZE))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(theme::border_strong())
                .cursor_pointer()
                .hover(|button| button.bg(theme::text_faint()))
                .child(symbol("close", 16., theme::text()))
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    center.update(cx, |center, cx| center.dismiss(id, cx));
                })
                .into_any_element();
        }
        if model.mentions_me {
            return side_chip("Erwähnung".to_owned(), true).into_any_element();
        }
        if model.count > 1 {
            return side_chip(format!("{} neue", model.count), false).into_any_element();
        }
        div()
            .text_size(px(12.))
            .text_color(theme::text_muted())
            .child(model.time.clone())
            .into_any_element()
    }

    fn body(&self, model: &ToastModel, text: &ToastText, hovered: bool) -> Div {
        let subtitle_icon = match &model.kind {
            ChatKind::Group { .. } => Some("group"),
            ChatKind::Channel { .. } => Some("tag"),
            ChatKind::Direct => None,
        };
        let preview = if text.image_only {
            h_flex()
                .gap(px(6.))
                .items_center()
                .child(symbol("image", 16., theme::text_muted()))
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(theme::text())
                        .child(text.preview.clone()),
                )
                .into_any_element()
        } else {
            highlighted_preview(&text.preview, model.mentions_me)
        };
        v_flex()
            .flex_1()
            .min_w_0()
            .gap(px(2.))
            .child(
                h_flex()
                    .w_full()
                    .gap(px(8.))
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text())
                            .child(text.title.clone()),
                    )
                    .child(div().flex_none().child(self.side_slot(model, hovered))),
            )
            .child(
                h_flex()
                    .gap(px(4.))
                    .items_center()
                    .h(px(16.))
                    .text_size(px(12.))
                    .text_color(theme::text_muted())
                    .children(subtitle_icon.map(|name| symbol(name, 14., theme::text_muted())))
                    .child(div().truncate().child(text.subtitle.clone())),
            )
            .child(preview)
    }

    fn action_row(&self, cx: &mut Context<Self>) -> Div {
        let id = self.id;
        let center = self.center.clone();
        h_flex()
            .w_full()
            .mt(px(4.))
            .justify_between()
            .child(pill_button("Gelesen", "done", false).on_click(move |_, _, cx| {
                cx.stop_propagation();
                center.update(cx, |center, cx| center.mark_read(id, cx));
            }))
            .child(
                pill_button("Antworten", "reply", true).on_click(cx.listener(
                    |this, _, window, cx| {
                        cx.stop_propagation();
                        this.open_reply(window, cx);
                    },
                )),
            )
    }

    fn reply_field(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let focused = self.input.focus_handle(cx).contains_focused(window, cx);
        let empty = self.input.read(cx).value().trim().is_empty();
        let send = div()
            .id("toast-send")
            .size(px(ACTION_ROW))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(8.))
            .bg(theme::accent())
            .child(symbol("arrow_upward", 20., theme::on_accent()))
            .when(empty, |button| button.opacity(0.4))
            .when(!empty, |button| {
                button
                    .cursor_pointer()
                    .hover(|button| button.bg(theme::accent_text()))
                    .on_click(cx.listener(|this, _, window, cx| {
                        cx.stop_propagation();
                        this.submit(window, cx);
                    }))
            });
        let hint = |keys: &'static str, action: &'static str| {
            h_flex()
                .gap(px(4.))
                .child(div().text_color(theme::text_soft()).child(keys))
                .child(action)
        };
        v_flex()
            .w_full()
            .mt(px(8.))
            .gap(px(4.))
            .child(
                h_flex()
                    .w_full()
                    .h(px(ACTION_ROW))
                    .gap(px(6.))
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h(px(ACTION_ROW))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .rounded(px(8.))
                            .bg(theme::surface())
                            .border_1()
                            .border_color(if focused {
                                theme::accent()
                            } else {
                                theme::border_strong()
                            })
                            .child(
                                Textarea::new(&self.input)
                                    .appearance(false)
                                    .bordered(false),
                            ),
                    )
                    .child(send),
            )
            .child(
                h_flex()
                    .h(px(16.))
                    .gap(px(12.))
                    .text_size(px(11.))
                    .text_color(theme::text_muted())
                    .child(hint("Enter", "senden"))
                    .child(hint("Umschalt+Enter", "Zeilenumbruch"))
                    .child(hint("Esc", "schließen")),
            )
    }

    fn confirmation(&self, model: &ToastModel, cx: &mut Context<Self>) -> Div {
        let sent = model.reply == ReplyState::Sent;
        let label = if sent {
            format!("Gesendet an {}", target_name(model))
        } else {
            "Senden fehlgeschlagen".to_owned()
        };
        let (icon_name, tone) = if sent {
            ("check_circle", theme::green())
        } else {
            ("error", theme::red_soft())
        };
        h_flex()
            .size_full()
            .gap(px(10.))
            .items_center()
            .child(symbol(icon_name, 22., tone))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text())
                    .child(label),
            )
            .when(sent, |row| {
                row.child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::text_muted())
                        .child(model.time.clone()),
                )
            })
            .when(!sent, |row| {
                row.child(pill_button("Erneut", "reply", true).on_click(cx.listener(
                    |this, _, _, cx| {
                        cx.stop_propagation();
                        this.retry(cx);
                    },
                )))
            })
    }
}

fn target_name(model: &ToastModel) -> String {
    match &model.kind {
        ChatKind::Group { .. } => model.chat_title.clone(),
        _ => model.sender_name.clone(),
    }
}

impl Render for ToastView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let id = self.id;
        let (model, preview_on) = {
            let center = self.center.read(cx);
            (center.model(id).cloned(), center.settings().preview)
        };
        let Some(model) = model else {
            return div()
                .size_full()
                .bg(theme::surface_raised())
                .into_any_element();
        };
        let text = describe(&model, preview_on);
        let narration = text.narration();
        if narration != self.last_title {
            window.set_window_title(&narration);
            self.last_title = narration;
        }
        let mention = model.mentions_me;
        let fill = if mention {
            theme::toast_mention_fill()
        } else {
            theme::surface_raised()
        };
        let border = if mention {
            theme::accent()
        } else {
            theme::border_strong()
        };
        let border_width = if mention { TOAST_BORDER_MENTION } else { 1. };
        let fraction = model.timer.fraction_left(Instant::now());
        let bar_color = if model.timer.is_paused() {
            theme::text_faint()
        } else {
            theme::accent()
        };
        let hovered = model.hovered;
        let content = match model.reply {
            ReplyState::Sent | ReplyState::Failed => self.confirmation(&model, cx),
            ReplyState::Open => v_flex()
                .w_full()
                .child(
                    h_flex()
                        .w_full()
                        .gap(px(12.))
                        .items_start()
                        .child(self.avatar(&model, fill, cx))
                        .child(self.body(&model, &text, true)),
                )
                .child(self.reply_field(window, cx)),
            ReplyState::Closed => v_flex()
                .w_full()
                .child(
                    h_flex()
                        .w_full()
                        .gap(px(12.))
                        .items_start()
                        .child(self.avatar(&model, fill, cx))
                        .child(self.body(&model, &text, hovered)),
                )
                .when(hovered, |column| column.child(self.action_row(cx))),
        };
        let center = self.center.clone();
        let hover_center = self.center.clone();
        div()
            .id(("toast", id))
            .track_focus(&self.focus_handle)
            .relative()
            .size_full()
            .bg(fill)
            .border(px(border_width))
            .border_color(border)
            .overflow_hidden()
            .cursor_pointer()
            .on_hover(move |is_hovered, _, cx| {
                hover_center.update(cx, |center, cx| center.set_hover(id, *is_hovered, cx));
            })
            .on_click(move |_, _, cx| {
                center.update(cx, |center, cx| center.activate(id, cx));
            })
            .capture_action(cx.listener(|this, _: &gpui_kit::component::input::Escape, window, cx| {
                this.close_reply(window, cx);
                cx.stop_propagation();
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    let id = this.id;
                    this.center.update(cx, |center, cx| center.dismiss(id, cx));
                }
            }))
            .child(div().size_full().p(px(12.)).child(content))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .bottom_0()
                    .h(px(PROGRESS_HEIGHT))
                    .w(relative(fraction))
                    .bg(bar_color),
            )
            .into_any_element()
    }
}

pub struct PillView {
    center: Entity<NotificationCenter>,
    _subscription: Subscription,
}

impl PillView {
    pub fn new(center: Entity<NotificationCenter>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&center, |_, _, cx| cx.notify());
        PillView {
            center,
            _subscription: subscription,
        }
    }
}

impl Render for PillView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let hidden = self.center.read(cx).hidden_count();
        let center = self.center.clone();
        div()
            .id("toast-pill")
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme::surface_raised())
            .border_1()
            .border_color(theme::border_strong())
            .cursor_pointer()
            .text_size(px(13.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme::text())
            .child(format!("+{hidden} weitere"))
            .on_click(move |_, _, cx| {
                center.update(cx, |center, cx| center.open_chat_list(cx));
            })
    }
}
