use std::collections::HashSet;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::{h_flex, tooltip::Tooltip, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{NEW_CHAT_BUTTON_SIZE, SidebarEvent, SidebarTab, SidebarView};
use crate::app_state::Selection;
use crate::chat_filter::{
    CLEAR_FILTER_TEXT, CLEAR_FILTERS, CLOSE_FILTER_TEXT, ChatFilterPill, FEWER_FILTERS,
    FILTER_PLACEHOLDER, MORE_FILTERS, NO_MATCHES, SHOW_FILTER_TEXT, mentioned_conversations,
};
use crate::theme;
use crate::views::widgets::icon;

pub const FILTER_TAG: &str = "shell/islands/sidebar/chat-filter";
pub const FILTER_INPUT_TAG: &str = "shell/islands/sidebar/chat-filter/input";
pub const FILTER_PILLS_TAG: &str = "shell/islands/sidebar/chat-filter/pills";
pub const FILTER_EMPTY_TAG: &str = "shell/islands/sidebar/chat-filter-empty";
const INPUT_HEIGHT: f32 = 32.;
const PILL_HEIGHT: f32 = 24.;
const SIDE_INSET: f32 = 16.;

impl SidebarView {
    pub fn toggle_filter_box(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab != SidebarTab::Chats {
            self.open_tab(SidebarTab::Chats, cx);
        } else if self.filter_box_open {
            self.close_filter_box(window, cx);
            return;
        }
        self.filter_box_open = true;
        let input = self.filter_input(window, cx);
        input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    pub fn toggle_pill(&mut self, pill: ChatFilterPill, cx: &mut Context<Self>) {
        if self.tab != SidebarTab::Chats {
            self.open_tab(SidebarTab::Chats, cx);
        }
        if self.chat_filter.toggle(pill) {
            cx.notify();
        }
    }

    pub fn clear_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.chat_filter.clear();
        if let Some(input) = &self.filter_input {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        cx.notify();
    }

    pub(super) fn mentioned_chats(&self, cx: &App) -> HashSet<String> {
        if !self.chat_filter.has(ChatFilterPill::Mentions) {
            return HashSet::new();
        }
        let activity = self.activity_panel.read(cx).activity();
        mentioned_conversations(activity.read(cx).feed().unread_entries())
    }

    pub(super) fn selected_chat_id(&self, cx: &App) -> Option<String> {
        match &self.state.read(cx).selection {
            Some(Selection::Chat(chat_id)) => Some(chat_id.clone()),
            _ => None,
        }
    }

    fn close_filter_box(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter_box_open = false;
        cx.emit(SidebarEvent::RestoreFocus);
        self.chat_filter.text.clear();
        if let Some(input) = &self.filter_input {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        cx.notify();
    }

    fn filter_input(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some(input) = &self.filter_input {
            return input.clone();
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(FILTER_PLACEHOLDER));
        let subscription = cx.subscribe_in(&input, window, |this, input, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.chat_filter.text = input.read(cx).value().to_string();
                cx.notify();
            }
        });
        self.filter_subscription = Some(subscription);
        self.filter_input = Some(input.clone());
        input
    }

    pub(super) fn filter_toggle_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.filter_box_open;
        let active = open || self.chat_filter.is_active();
        let label = if open {
            CLOSE_FILTER_TEXT
        } else {
            SHOW_FILTER_TEXT
        };
        div()
            .id("chat-filter-toggle")
            .size(px(NEW_CHAT_BUTTON_SIZE))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(8.))
            .cursor_pointer()
            .when(active, |button| button.bg(theme::popup_hover()))
            .hover(|button| button.bg(theme::popup_hover()))
            .tooltip(move |window, cx| {
                Tooltip::new(format!("{label} (Ctrl+Shift+F)")).build(window, cx)
            })
            .child(icon(
                IconName::Funnel,
                16.,
                if active {
                    theme::accent_text()
                } else {
                    theme::text_soft()
                },
            ))
            .on_click(cx.listener(|this, _, window, cx| this.toggle_filter_box(window, cx)))
    }

    pub(super) fn filter_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let input_row = self
            .filter_input
            .clone()
            .filter(|_| self.filter_box_open)
            .map(|input| self.filter_input_row(&input, cx));
        v_flex()
            .debug_selector(|| FILTER_TAG.into())
            .flex_none()
            .w_full()
            .pb(px(8.))
            .gap(px(8.))
            .children(input_row)
            .child(self.filter_pills(cx))
    }

    fn filter_input_row(&self, input: &Entity<InputState>, cx: &mut Context<Self>) -> Div {
        let has_text = !self.chat_filter.text.is_empty();
        h_flex()
            .debug_selector(|| FILTER_INPUT_TAG.into())
            .mx(px(SIDE_INSET))
            .h(px(INPUT_HEIGHT))
            .flex_none()
            .pl(px(10.))
            .pr(px(4.))
            .gap(px(8.))
            .items_center()
            .rounded(px(6.))
            .bg(theme::surface_raised())
            .capture_action(cx.listener(|this, _: &Escape, window, cx| {
                this.close_filter_box(window, cx);
                cx.stop_propagation();
            }))
            .child(icon(IconName::Funnel, 14., theme::text_muted()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(input).appearance(false).bordered(false)),
            )
            .when(has_text, |row| {
                row.child(
                    div()
                        .id("chat-filter-clear-text")
                        .size(px(PILL_HEIGHT))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(6.))
                        .cursor_pointer()
                        .hover(|button| button.bg(theme::popup_hover()))
                        .tooltip(|window, cx| Tooltip::new(CLEAR_FILTER_TEXT).build(window, cx))
                        .child(icon(IconName::X, 14., theme::text_muted()))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.chat_filter.text.clear();
                            if let Some(input) = &this.filter_input {
                                input.update(cx, |input, cx| {
                                    input.set_value("", window, cx);
                                    input.focus(window, cx);
                                });
                            }
                            cx.notify();
                        })),
                )
            })
    }

    fn filter_pills(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let overflow_active = ChatFilterPill::OVERFLOW
            .iter()
            .any(|pill| self.chat_filter.has(*pill));
        let show_overflow = self.more_filters_open || overflow_active;
        let mut pills: Vec<AnyElement> = ChatFilterPill::PRIMARY
            .into_iter()
            .map(|pill| self.filter_pill(pill, cx).into_any_element())
            .collect();
        if show_overflow {
            pills.extend(
                ChatFilterPill::OVERFLOW
                    .into_iter()
                    .map(|pill| self.filter_pill(pill, cx).into_any_element()),
            );
        }
        if !overflow_active {
            pills.push(self.more_filters_button(cx).into_any_element());
        }
        h_flex()
            .debug_selector(|| FILTER_PILLS_TAG.into())
            .px(px(SIDE_INSET))
            .flex_wrap()
            .gap(px(6.))
            .children(pills)
    }

    fn filter_pill(&self, pill: ChatFilterPill, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.chat_filter.has(pill);
        let disabled = self.chat_filter.is_disabled(pill);
        div()
            .id(ElementId::Name(
                format!("chat-filter-{}", pill.label()).into(),
            ))
            .h(px(PILL_HEIGHT))
            .px(px(12.))
            .flex()
            .flex_none()
            .items_center()
            .rounded_full()
            .text_size(px(12.))
            .when(selected, |chip| {
                chip.bg(theme::accent()).text_color(theme::on_accent())
            })
            .when(!selected && disabled, |chip| {
                chip.bg(theme::surface_raised())
                    .text_color(theme::text_faint())
            })
            .when(!selected && !disabled, |chip| {
                chip.bg(theme::surface_raised())
                    .text_color(theme::text_soft())
                    .cursor_pointer()
                    .hover(|chip| chip.bg(theme::border_strong()))
            })
            .child(pill.label())
            .when(!disabled, |chip| {
                chip.on_click(cx.listener(move |this, _, _, cx| this.toggle_pill(pill, cx)))
            })
    }

    fn more_filters_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.more_filters_open;
        let label = if open { FEWER_FILTERS } else { MORE_FILTERS };
        div()
            .id("chat-filter-more")
            .size(px(PILL_HEIGHT))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .bg(theme::surface_raised())
            .hover(|chip| chip.bg(theme::border_strong()))
            .tooltip(move |window, cx| Tooltip::new(label).build(window, cx))
            .child(icon(
                if open {
                    IconName::ChevronUp
                } else {
                    IconName::ChevronDown
                },
                14.,
                theme::text_soft(),
            ))
            .on_click(cx.listener(|this, _, _, cx| {
                this.more_filters_open = !this.more_filters_open;
                cx.notify();
            }))
    }

    pub(super) fn filter_empty_state(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .debug_selector(|| FILTER_EMPTY_TAG.into())
            .w_full()
            .px(px(SIDE_INSET))
            .py(px(SIDE_INSET))
            .gap(px(8.))
            .items_center()
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(theme::text_muted())
                    .child(NO_MATCHES),
            )
            .child(
                div()
                    .id("chat-filter-clear-all")
                    .px(px(8.))
                    .h(px(PILL_HEIGHT))
                    .flex()
                    .items_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .text_size(px(12.))
                    .text_color(theme::accent_text())
                    .hover(|button| button.bg(theme::surface_raised()))
                    .child(CLEAR_FILTERS)
                    .on_click(cx.listener(|this, _, window, cx| this.clear_filters(window, cx))),
            )
    }
}
