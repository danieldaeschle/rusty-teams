use gpui_kit::component::fluent::{
    MENU_ICON_LABEL_GAP, MENU_ICON_SLOT, MENU_ITEM_GAP, MENU_ITEM_HEIGHT, MENU_ITEM_PADDING_X,
    MENU_LIST_PADDING, OPTION_CHECK_SIZE,
};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;
use store::MessageImportance;

use super::{COMPOSER_BUTTON_INSET, COMPOSER_BUTTON_SIZE, COMPOSER_ICON_SIZE, Composer};
use crate::delivery_options::{
    DeliveryOption, URGENT_TOO_MANY_MEMBERS, UrgentAvailability, allowed_importance,
    delivery_options, urgent_banner,
};
use crate::importance::importance_label;
use crate::theme::{self, ElevatedSurface as _};
use crate::views::widgets::symbol;

pub const COMPOSER_IMPORTANCE_LABEL_TAG: &str = "composer/importance-label";
pub const COMPOSER_URGENT_BANNER_TAG: &str = "composer-urgent-banner";
pub const DELIVERY_MENU_TAG: &str = "delivery-menu";
const MENU_WIDTH: f32 = 320.;
const OPTION_ICON_SIZE: f32 = 20.;
const BANNER_ICON_SIZE: f32 = 16.;
const DISABLED_OPACITY: f32 = 0.4;
const LABEL_SIZE: f32 = 12.;
const LABEL_LINE_HEIGHT: f32 = 16.;
const BANNER_HEIGHT: f32 = 36.;
const BANNER_PADDING: f32 = 12.;
const BANNER_TINT: f32 = 0.14;
pub const LABEL_INSET_X: f32 =
    COMPOSER_BUTTON_INSET + (COMPOSER_BUTTON_SIZE - COMPOSER_ICON_SIZE) / 2.;

impl Composer {
    fn urgent_availability(&self, cx: &App) -> UrgentAvailability {
        let conversation_id = self.conversation_id.as_deref().unwrap_or_default();
        self.app
            .read(cx)
            .urgent_availability(conversation_id, self.editing.is_some())
    }

    /// Teams sends edits and scheduled messages as Standard, and drops Urgent where it is no longer offered.
    pub(super) fn sendable_importance(&self, cx: &App) -> MessageImportance {
        if self.editing.is_some() {
            return MessageImportance::Normal;
        }
        allowed_importance(self.importance, self.urgent_availability(cx))
    }

    pub(super) fn reset_importance(&mut self) {
        self.importance = MessageImportance::Normal;
        self.delivery_menu = false;
    }

    fn choose_importance(
        &mut self,
        importance: MessageImportance,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.importance = importance;
        self.delivery_menu = false;
        self.focus(window, cx);
        cx.notify();
    }

    pub(super) fn render_delivery_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.delivery_menu || self.editing.is_some() {
            return None;
        }
        let selected = self.sendable_importance(cx);
        let rows = delivery_options(self.urgent_availability(cx))
            .into_iter()
            .map(|(option, enabled)| {
                self.render_option(option, enabled, option.importance == selected, cx)
            })
            .collect::<Vec<_>>();
        Some(
            v_flex()
                .id("delivery-menu")
                .debug_selector(|| DELIVERY_MENU_TAG.into())
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
                    this.delivery_menu = false;
                    cx.notify();
                }))
                .children(rows)
                .into_any_element(),
        )
    }

    fn render_option(
        &self,
        option: DeliveryOption,
        enabled: bool,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let accent = if option.importance.is_marked() {
            theme::red()
        } else {
            theme::text_soft()
        };
        let row = h_flex()
            .id(option.tag)
            .debug_selector(|| option.tag.into())
            .w_full()
            .min_h(MENU_ITEM_HEIGHT)
            .px(MENU_ITEM_PADDING_X)
            .py(MENU_ITEM_PADDING_X)
            .gap(MENU_ICON_LABEL_GAP)
            .items_center()
            .rounded(px(6.))
            .child(
                div()
                    .size(MENU_ICON_SLOT)
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(symbol(option.symbol, OPTION_ICON_SIZE, accent)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text())
                            .child(option.header),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme::text_muted())
                            .child(option.content),
                    ),
            )
            .children(
                selected.then(|| symbol("done", f32::from(OPTION_CHECK_SIZE), theme::text())),
            );
        if !enabled {
            return row
                .opacity(DISABLED_OPACITY)
                .tooltip(|window, cx| Tooltip::new(URGENT_TOO_MANY_MEMBERS).build(window, cx));
        }
        let importance = option.importance;
        row.cursor_pointer()
            .hover(|row| row.bg(theme::popup_hover()))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.choose_importance(importance, window, cx)
            }))
    }

    pub(super) fn render_importance_label(&self, cx: &App) -> Option<Div> {
        let label = importance_label(self.sendable_importance(cx))?;
        Some(
            div()
                .pt(px(COMPOSER_BUTTON_INSET * 2.))
                .px(px(LABEL_INSET_X))
                .text_size(px(LABEL_SIZE))
                .line_height(px(LABEL_LINE_HEIGHT))
                .font_weight(FontWeight::BOLD)
                .text_color(theme::red())
                .child(
                    div()
                        .debug_selector(|| COMPOSER_IMPORTANCE_LABEL_TAG.into())
                        .child(label),
                ),
        )
    }

    pub(super) fn render_urgent_banner(&self, cx: &mut Context<Self>) -> Option<Div> {
        if self.sendable_importance(cx) != MessageImportance::Urgent {
            return None;
        }
        let member_count = self
            .conversation_id
            .as_deref()
            .and_then(|id| {
                self.app
                    .read(cx)
                    .sidebar
                    .chats
                    .iter()
                    .find(|chat| chat.id == id)
                    .map(|chat| chat.members.len())
            })
            .unwrap_or_default();
        Some(
            h_flex()
                .debug_selector(|| COMPOSER_URGENT_BANNER_TAG.into())
                .w_full()
                .h(px(BANNER_HEIGHT))
                .mb(px(8.))
                .px(px(BANNER_PADDING))
                .gap(px(8.))
                .items_center()
                .rounded(px(10.))
                .bg(theme::red().opacity(BANNER_TINT))
                .text_size(px(12.))
                .text_color(theme::red_tint())
                .child(symbol(
                    "notification_important",
                    BANNER_ICON_SIZE,
                    theme::red_tint(),
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(urgent_banner(member_count)),
                )
                .child(
                    div()
                        .id("composer-urgent-banner-close")
                        .size(px(24.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(6.))
                        .cursor_pointer()
                        .hover(|button| button.bg(theme::red().opacity(BANNER_TINT)))
                        .tooltip(|window, cx| Tooltip::new("Send as standard").build(window, cx))
                        .child(symbol("close", BANNER_ICON_SIZE, theme::red_tint()))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.choose_importance(MessageImportance::Normal, window, cx)
                        })),
                ),
        )
    }
}
