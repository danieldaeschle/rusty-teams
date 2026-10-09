use chrono::Local;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Side,
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::{ChatSection, ForcedAvailability, ForcedKind, PresenceStatus, WorkLocationKind};

use super::avatar::{person_avatar, with_presence};
use super::widgets::{icon, symbol};
use crate::app_state::AppState;
use crate::data::{Person, PresenceKind};
use crate::own_status::{
    STATUS_CHOICES, STATUS_DURATIONS, StateSummary, StatusDuration, choice_label, duration_target,
    presence_kind_for, state_summary, work_location_label,
};
use crate::theme;

pub const MENU_WIDTH: f32 = 250.;
const TITLE_AVATAR_SIZE: f32 = 26.;
const HEADER_AVATAR_SIZE: f32 = 40.;
const RING_WIDTH: f32 = 2.;
const MENU_ICON_SIZE: f32 = 14.;
const ICON_COLUMN: f32 = 16.;
const HEADER_INSET: f32 = 24.;
const SWITCH_WIDTH: f32 = 28.;
const SWITCH_HEIGHT: f32 = 16.;
const SWITCH_THUMB: f32 = 12.;
const SWITCH_PADDING: f32 = 2.;
const TOGGLE_LABEL_WIDTH: f32 = 270.;

#[derive(Clone)]
struct MenuContext {
    app: Entity<AppState>,
    person: Person,
    email: String,
    note: Option<String>,
    status: PresenceStatus,
    summary: StateSummary,
}

pub fn own_status_button(app: &Entity<AppState>, state: &AppState) -> Option<AnyElement> {
    let person = state.directory.me.clone()?;
    let presence = state.directory.presence_of(&person.user_id);
    let open = state.status_menu_open;
    let summary = state_summary(&state.own_status, state.own_presence_kind(), &Local::now());
    let context = MenuContext {
        app: app.clone(),
        email: state.own_email.clone(),
        note: state.own_status.note.as_ref().map(|note| note.text.clone()),
        status: state.own_status.clone(),
        summary,
        person: person.clone(),
    };
    let avatar = with_presence(
        person_avatar(
            &state.directory,
            Some(&person.user_id),
            &person.display_name,
            TITLE_AVATAR_SIZE,
        ),
        presence,
        TITLE_AVATAR_SIZE,
        theme::background(),
    );
    let ringed = div()
        .p(px(1.))
        .rounded_full()
        .border(px(RING_WIDTH))
        .border_color(if open {
            theme::accent()
        } else {
            transparent_black()
        })
        .child(avatar);
    let toggle = app.clone();
    Some(
        Button::new("title-own-status")
            .ghost()
            .p_0()
            .rounded_full()
            .child(ringed)
            .dropdown_menu_with_anchor(Anchor::TopRight, move |popup, window, cx| {
                status_menu(popup, window, cx, &context)
            })
            .on_open_change(move |open, _, cx| {
                let open = *open;
                toggle.update(cx, |state, cx| state.set_status_menu_open(open, cx));
            })
            .into_any_element(),
    )
}

fn status_icon(kind: PresenceKind) -> gpui_kit::component::Icon {
    let (name, color) = match kind {
        PresenceKind::Available => ("status_dot", theme::green()),
        PresenceKind::Busy => ("status_dot", theme::red()),
        PresenceKind::DoNotDisturb => ("status_dnd", theme::red()),
        PresenceKind::Away => ("status_away", theme::amber()),
        PresenceKind::Offline => ("status_ring", theme::text_muted()),
        PresenceKind::Unknown => ("status_ring", theme::text_faint()),
    };
    symbol(name, MENU_ICON_SIZE, color)
}

fn menu_icon(name: IconName) -> gpui_kit::component::Icon {
    icon(name, MENU_ICON_SIZE, theme::text_muted())
}

fn header(context: &MenuContext, cx: &App) -> Div {
    let state = context.app.read(cx);
    let presence = state.directory.presence_of(&context.person.user_id);
    let avatar = with_presence(
        person_avatar(
            &state.directory,
            Some(&context.person.user_id),
            &context.person.display_name,
            HEADER_AVATAR_SIZE,
        ),
        presence,
        HEADER_AVATAR_SIZE,
        theme::surface(),
    );
    let line = |text: String, color: Hsla| {
        div()
            .w_full()
            .truncate()
            .text_size(px(12.))
            .text_color(color)
            .child(text)
    };
    h_flex()
        .w(px(MENU_WIDTH - HEADER_INSET))
        .ml(px(-(ICON_COLUMN + 4.)))
        .py(px(6.))
        .gap(px(10.))
        .items_center()
        .child(avatar)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .w_full()
                        .truncate()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::text())
                        .child(context.person.display_name.clone()),
                )
                .child(line(context.email.clone(), theme::text_muted()))
                .children(
                    context
                        .note
                        .clone()
                        .map(|note| line(note, theme::text_soft())),
                ),
        )
}

fn status_menu(
    popup: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
    context: &MenuContext,
) -> PopupMenu {
    let header_context = context.clone();
    let message_app = context.app.clone();
    let settings_app = context.app.clone();
    let row_label = match &context.summary.until {
        Some(until) => format!("{} {until}", context.summary.label),
        None => context.summary.label.to_owned(),
    };
    let submenu_context = context.clone();
    popup
        .min_w(px(MENU_WIDTH))
        .item(PopupMenuItem::element(move |_, cx| {
            header(&header_context, cx)
        }))
        .separator()
        .submenu_with_icon(
            Some(status_icon(context.summary.presence)),
            row_label,
            window,
            cx,
            move |submenu, window, cx| state_menu(submenu, window, cx, &submenu_context),
        )
        .item(work_location_item(context, window, cx))
        .item(
            PopupMenuItem::new("Set status message")
                .icon(menu_icon(IconName::Pencil))
                .on_click(move |_, _, cx| {
                    message_app.update(cx, |state, cx| state.request_status_message(cx));
                }),
        )
        .separator()
        .item(chat_list_item(context, window, cx))
        .item(
            PopupMenuItem::new("Notification settings")
                .icon(menu_icon(IconName::Settings))
                .on_click(move |_, _, cx| {
                    settings_app.update(cx, |state, cx| state.open_notification_settings(cx));
                }),
        )
}

fn chat_list_item(
    context: &MenuContext,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenuItem {
    let app = context.app.clone();
    let submenu = PopupMenu::build(window, cx, move |menu, _, cx| {
        let settings = app.read(cx).directory.section_settings;
        menu.item(section_toggle(
            &app,
            ChatSection::Muted,
            "Show muted chats in own section",
            settings.enabled(ChatSection::Muted),
        ))
        .item(section_toggle(
            &app,
            ChatSection::Meeting,
            "Show meeting chats in own section",
            settings.enabled(ChatSection::Meeting),
        ))
    });
    PopupMenuItem::submenu("Chat list", submenu).icon(menu_icon(IconName::LayoutList))
}

fn section_toggle(
    app: &Entity<AppState>,
    section: ChatSection,
    label: &'static str,
    enabled: bool,
) -> PopupMenuItem {
    let app = app.clone();
    PopupMenuItem::element(move |_, _| {
        h_flex()
            .w(px(TOGGLE_LABEL_WIDTH))
            .gap(px(12.))
            .items_center()
            .justify_between()
            .child(div().flex_1().min_w_0().truncate().child(label))
            .child(switch(enabled))
    })
    .on_click(move |_, _, cx| {
        app.update(cx, |state, cx| {
            state.set_chat_section(section, !enabled, cx)
        });
    })
}

fn switch(enabled: bool) -> Div {
    let thumb = div()
        .size(px(SWITCH_THUMB))
        .rounded_full()
        .bg(theme::text_strong());
    h_flex()
        .w(px(SWITCH_WIDTH))
        .h(px(SWITCH_HEIGHT))
        .flex_none()
        .p(px(SWITCH_PADDING))
        .items_center()
        .rounded_full()
        .bg(if enabled {
            theme::accent()
        } else {
            theme::badge_muted()
        })
        .when(enabled, |track| track.justify_end())
        .child(thumb)
}

fn work_location_icon(kind: Option<WorkLocationKind>) -> gpui_kit::component::Icon {
    menu_icon(match kind {
        Some(WorkLocationKind::Office) => IconName::Building2,
        Some(WorkLocationKind::Remote) => IconName::House,
        None => IconName::MapPin,
    })
}

fn work_location_item(
    context: &MenuContext,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenuItem {
    let current = context.status.work_location;
    let app = context.app.clone();
    let submenu = PopupMenu::build(window, cx, move |menu, _, _| {
        let pick = |label: &'static str, kind: WorkLocationKind| {
            let app = app.clone();
            PopupMenuItem::new(label)
                .icon(work_location_icon(Some(kind)))
                .on_click(move |_, _, cx| {
                    app.update(cx, |state, cx| state.set_own_work_location(Some(kind), cx));
                })
        };
        let clear_app = app.clone();
        let menu = menu
            .item(PopupMenuItem::label("Set work location for today"))
            .item(pick("Office", WorkLocationKind::Office))
            .item(pick("Remote", WorkLocationKind::Remote));
        if current.is_none() {
            return menu;
        }
        menu.separator().item(
            PopupMenuItem::new("Clear work location")
                .icon(menu_icon(IconName::X))
                .on_click(move |_, _, cx| {
                    clear_app.update(cx, |state, cx| state.set_own_work_location(None, cx));
                }),
        )
    });
    PopupMenuItem::submenu(work_location_label(current), submenu)
        .icon(work_location_icon(current.map(|location| location.kind)))
}

fn state_menu(
    popup: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
    context: &MenuContext,
) -> PopupMenu {
    let current = context.status.forced.map(|forced| forced.kind);
    let mut popup = popup.check_side(Side::Right);
    for kind in STATUS_CHOICES {
        let app = context.app.clone();
        popup = popup.item(
            PopupMenuItem::new(choice_label(kind))
                .icon(status_icon(presence_kind_for(kind)))
                .checked(current == Some(kind))
                .on_click(move |_, _, cx| {
                    let forced = ForcedAvailability {
                        kind,
                        expires_at: None,
                    };
                    app.update(cx, |state, cx| state.set_own_availability(Some(forced), cx));
                }),
        );
    }
    let target = duration_target(&context.status);
    let app = context.app.clone();
    let durations = PopupMenu::build(window, cx, move |menu, _, _| {
        STATUS_DURATIONS.iter().fold(menu, |menu, duration| {
            menu.item(duration_item(&app, target, *duration))
        })
    });
    let reset_app = context.app.clone();
    popup
        .separator()
        .item(
            PopupMenuItem::submenu("Duration", durations)
                .icon(menu_icon(IconName::Clock))
                .disabled(target.is_none()),
        )
        .item(
            PopupMenuItem::new("Reset status")
                .icon(menu_icon(IconName::RotateCcw))
                .on_click(move |_, _, cx| {
                    reset_app.update(cx, |state, cx| state.set_own_availability(None, cx));
                }),
        )
}

fn duration_item(
    app: &Entity<AppState>,
    target: Option<ForcedKind>,
    duration: StatusDuration,
) -> PopupMenuItem {
    let app = app.clone();
    PopupMenuItem::new(duration.label())
        .disabled(target.is_none())
        .on_click(move |_, _, cx| {
            let Some(kind) = target else {
                return;
            };
            let forced = ForcedAvailability {
                kind,
                expires_at: Some(duration.expires_at(Local::now())),
            };
            app.update(cx, |state, cx| state.set_own_availability(Some(forced), cx));
        })
}
