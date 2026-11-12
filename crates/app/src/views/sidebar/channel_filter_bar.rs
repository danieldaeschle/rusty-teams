use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{h_flex, tooltip::Tooltip, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{NEW_CHAT_BUTTON_SIZE, SidebarEvent, SidebarView};
use crate::app_state::Selection;
use crate::channel_filter::{CHANNEL_FILTER_PLACEHOLDER, UNREAD_PILL};
use crate::chat_filter::{CLOSE_FILTER_TEXT, SHOW_FILTER_TEXT};
use crate::theme;
use crate::views::create_channel_dialog::open_create_channel_dialog;
use crate::views::widgets::{icon, menu_icon};

pub const CHANNEL_FILTER_TAG: &str = "shell/islands/sidebar/channel-filter";
const NEW_CHANNEL_LABEL: &str = "New channel";
const NEW_MESSAGE_LABEL: &str = "New message";
const INPUT_HEIGHT: f32 = 32.;
const PILL_HEIGHT: f32 = 24.;
const SIDE_INSET: f32 = 16.;

impl SidebarView {
    fn toggle_channel_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.channel_filter_open {
            self.close_channel_filter(window, cx);
            return;
        }
        self.channel_filter_open = true;
        let input = self.channel_filter_input(window, cx);
        input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    fn close_channel_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.channel_filter_open = false;
        self.channel_filter.text.clear();
        if let Some(input) = &self.channel_filter_input {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        cx.emit(SidebarEvent::RestoreFocus);
        cx.notify();
    }

    fn channel_filter_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(input) = &self.channel_filter_input {
            return input.clone();
        }
        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder(CHANNEL_FILTER_PLACEHOLDER));
        let subscription = cx.subscribe_in(&input, window, |this, input, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.channel_filter.text = input.read(cx).value().to_string();
                cx.notify();
            }
        });
        self.channel_filter_subscription = Some(subscription);
        self.channel_filter_input = Some(input.clone());
        input
    }

    pub(super) fn channel_filter_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.channel_filter_open;
        let label = if open {
            CLOSE_FILTER_TEXT
        } else {
            SHOW_FILTER_TEXT
        };
        div()
            .id("channel-filter-toggle")
            .size(px(NEW_CHAT_BUTTON_SIZE))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(8.))
            .cursor_pointer()
            .when(open, |button| button.bg(theme::popup_hover()))
            .hover(|button| button.bg(theme::popup_hover()))
            .tooltip(move |window, cx| Tooltip::new(label).build(window, cx))
            .child(icon(
                IconName::Search,
                16.,
                if open {
                    theme::accent_text()
                } else {
                    theme::text_soft()
                },
            ))
            .on_click(cx.listener(|this, _, window, cx| this.toggle_channel_filter(window, cx)))
    }

    pub(super) fn new_channel_items_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.clone();
        let sidebar = cx.entity().downgrade();
        let app = self.state.read(cx);
        let channel_selected = matches!(app.selection, Some(Selection::Channel(_)));
        let team_id = match &app.selection {
            Some(Selection::Channel(channel_id)) => app
                .sidebar
                .teams
                .iter()
                .find(|entry| {
                    entry
                        .channels
                        .iter()
                        .any(|channel| &channel.id == channel_id)
                })
                .filter(|entry| entry.rights.add_channel && !app.mode.read_only)
                .map(|entry| entry.team.id.clone()),
            _ => None,
        };
        Button::new("channel-list-new")
            .icon(IconName::Pencil)
            .ghost()
            .size(px(NEW_CHAT_BUTTON_SIZE))
            .rounded(px(8.))
            .tooltip("New items")
            .dropdown_menu(move |menu, _, _| {
                let (state, team_id, sidebar) = (state.clone(), team_id.clone(), sidebar.clone());
                menu.item(
                    PopupMenuItem::new(NEW_MESSAGE_LABEL)
                        .icon(menu_icon(IconName::MessageSquare))
                        .disabled(!channel_selected)
                        .on_click(move |_, _, cx| {
                            sidebar
                                .update(cx, |_, cx| cx.emit(SidebarEvent::NewChannelPost))
                                .ok();
                        }),
                )
                .item(
                    PopupMenuItem::new(NEW_CHANNEL_LABEL)
                        .icon(menu_icon(IconName::Hash))
                        .disabled(team_id.is_none())
                        .on_click(move |_, window, cx| {
                            if let Some(team_id) = &team_id {
                                open_create_channel_dialog(state.clone(), team_id, window, cx);
                            }
                        }),
                )
            })
    }

    pub(super) fn channel_filter_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let input_row = self
            .channel_filter_input
            .clone()
            .filter(|_| self.channel_filter_open)
            .map(|input| self.channel_filter_input_row(&input, cx));
        let selected = self.channel_filter.unread_only;
        v_flex()
            .debug_selector(|| CHANNEL_FILTER_TAG.into())
            .flex_none()
            .w_full()
            .pb(px(8.))
            .gap(px(8.))
            .children(input_row)
            .child(
                h_flex().px(px(SIDE_INSET)).child(
                    div()
                        .id("channel-filter-unread")
                        .h(px(PILL_HEIGHT))
                        .px(px(12.))
                        .flex()
                        .flex_none()
                        .items_center()
                        .rounded_full()
                        .text_size(px(12.))
                        .cursor_pointer()
                        .when(selected, |chip| {
                            chip.bg(theme::accent()).text_color(theme::on_accent())
                        })
                        .when(!selected, |chip| {
                            chip.bg(theme::surface_raised())
                                .text_color(theme::text_soft())
                                .hover(|chip| chip.bg(theme::border_strong()))
                        })
                        .child(UNREAD_PILL)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.channel_filter.unread_only = !this.channel_filter.unread_only;
                            cx.notify();
                        })),
                ),
            )
    }

    fn channel_filter_input_row(&self, input: &Entity<InputState>, cx: &mut Context<Self>) -> Div {
        h_flex()
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
                this.close_channel_filter(window, cx);
                cx.stop_propagation();
            }))
            .child(icon(IconName::Search, 14., theme::text_muted()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(input).appearance(false).bordered(false)),
            )
    }
}
