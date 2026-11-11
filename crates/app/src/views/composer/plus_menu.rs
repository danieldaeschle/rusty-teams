use gpui_kit::component::fluent::{
    MENU_ICON_LABEL_GAP, MENU_ICON_SLOT, MENU_ITEM_GAP, MENU_ITEM_HEIGHT, MENU_ITEM_PADDING_X,
    MENU_LIST_PADDING,
};
use gpui_kit::component::tooltip::PopupTriggerTooltip as _;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{COMPOSER_BUTTON_SIZE, COMPOSER_ICON_SIZE, Composer};
use crate::app_state::AppState;
use crate::delivery_options::DELIVERY_BUTTON_TITLE;
use crate::theme::{self, ElevatedSurface as _};
use crate::views::new_chat::Pick;
use crate::views::new_meeting_dialog::open_new_meeting_with;
use crate::views::widgets::symbol;

pub const COMPOSER_PLUS_TAG: &str = "composer/actions/plus";
pub const PLUS_MENU_TAG: &str = "composer-plus-menu";
const PLUS_TITLE: &str = "Actions and apps";
const MENU_WIDTH: f32 = 260.;
const ITEM_ICON_SIZE: f32 = 20.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlusAction {
    ScheduleMessage,
    DeliveryOptions,
    ScheduleMeeting,
}

impl PlusAction {
    pub const ALL: [PlusAction; 3] = [
        PlusAction::ScheduleMessage,
        PlusAction::DeliveryOptions,
        PlusAction::ScheduleMeeting,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PlusAction::ScheduleMessage => "Schedule message",
            PlusAction::DeliveryOptions => DELIVERY_BUTTON_TITLE,
            PlusAction::ScheduleMeeting => "Schedule meeting",
        }
    }

    pub fn tag(self) -> &'static str {
        match self {
            PlusAction::ScheduleMessage => "composer-plus/schedule-message",
            PlusAction::DeliveryOptions => "composer-plus/delivery-options",
            PlusAction::ScheduleMeeting => "composer-plus/schedule-meeting",
        }
    }

    fn symbol(self) -> &'static str {
        match self {
            PlusAction::ScheduleMessage => "schedule_send",
            PlusAction::DeliveryOptions => "priority_high",
            PlusAction::ScheduleMeeting => "calendar_add_on",
        }
    }
}

impl Composer {
    pub(super) fn render_plus_button(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        if self.editing.is_some() {
            return None;
        }
        let open = self.plus_menu;
        let marked = self.importance.is_marked();
        Some(
            div()
                .id("composer-plus")
                .debug_selector(|| COMPOSER_PLUS_TAG.into())
                .size(px(COMPOSER_BUTTON_SIZE))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .cursor_pointer()
                .when(open, |button| button.bg(theme::popup_hover()))
                .hover(|button| button.bg(theme::popup_hover()))
                .popup_trigger_tooltip_text(open, PLUS_TITLE)
                .child(symbol(
                    "add",
                    COMPOSER_ICON_SIZE,
                    if marked {
                        theme::red()
                    } else {
                        theme::text_muted()
                    },
                ))
                .on_mouse_down(MouseButton::Left, |_, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                })
                .on_click(cx.listener(|this, _, _, cx| {
                    this.plus_menu = !this.plus_menu;
                    this.delivery_menu = false;
                    cx.notify();
                })),
        )
    }

    pub(super) fn render_plus_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.plus_menu || self.editing.is_some() {
            return None;
        }
        let rows = PlusAction::ALL
            .into_iter()
            .map(|action| self.render_plus_item(action, cx))
            .collect::<Vec<_>>();
        Some(
            v_flex()
                .id(PLUS_MENU_TAG)
                .debug_selector(|| PLUS_MENU_TAG.into())
                .absolute()
                .bottom(relative(1.))
                .right_0()
                .mb(px(6.))
                .w(px(MENU_WIDTH))
                .max_w(relative(1.))
                .p(MENU_LIST_PADDING)
                .gap(MENU_ITEM_GAP)
                .rounded(px(10.))
                .flyout_surface()
                .occlude()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.plus_menu = false;
                    cx.notify();
                }))
                .children(rows)
                .into_any_element(),
        )
    }

    fn render_plus_item(&self, action: PlusAction, cx: &mut Context<Self>) -> Stateful<Div> {
        h_flex()
            .id(action.label())
            .debug_selector(move || action.tag().into())
            .w_full()
            .h(MENU_ITEM_HEIGHT)
            .px(MENU_ITEM_PADDING_X)
            .gap(MENU_ICON_LABEL_GAP)
            .items_center()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(|row| row.bg(theme::popup_hover()))
            .child(
                div()
                    .size(MENU_ICON_SLOT)
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(symbol(action.symbol(), ITEM_ICON_SIZE, theme::text_soft())),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(theme::text())
                    .child(action.label()),
            )
            .on_click(
                cx.listener(move |this, _, window, cx| this.run_plus_action(action, window, cx)),
            )
    }

    fn run_plus_action(&mut self, action: PlusAction, window: &mut Window, cx: &mut Context<Self>) {
        self.plus_menu = false;
        match action {
            PlusAction::ScheduleMessage => self.open_schedule_picker(cx),
            PlusAction::DeliveryOptions => self.delivery_menu = true,
            PlusAction::ScheduleMeeting => {
                let invitees = self
                    .conversation_id
                    .as_deref()
                    .map(|chat_id| chat_invitees(self.app.read(cx), chat_id))
                    .unwrap_or_default();
                open_new_meeting_with(self.app.clone(), invitees, window, cx);
            }
        }
        cx.notify();
    }
}

pub(crate) fn chat_invitees(state: &AppState, chat_id: &str) -> Vec<Pick> {
    let me = state.directory.me.as_ref().map(|me| me.user_id.as_str());
    let Some(chat) = state.sidebar.chats.iter().find(|chat| chat.id == chat_id) else {
        return Vec::new();
    };
    chat.members
        .iter()
        .filter(|member| !member.is_bot())
        .filter_map(|member| {
            let user_id = member.user_id.clone()?;
            (Some(user_id.as_str()) != me).then(|| Pick {
                user_id,
                name: member.display_name.clone(),
                mail: None,
            })
        })
        .collect()
}
