use chrono::{Local, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Side,
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::*;
use teams_core::{ForcedAvailability, ForcedKind, PresenceStatus, WorkLocationKind};

use super::avatar::{person_avatar, status_glyph, with_presence, with_presence_badge};
use super::widgets::{menu_icon, symbol};
use crate::app_state::AppState;
use crate::i18n::tr;
use crate::data::{Person, PresenceKind};
use crate::notify::pause;
use crate::own_status::{
    STATUS_CHOICES, STATUS_DURATIONS, StateSummary, StatusDuration, choice_label, duration_target,
    presence_kind_for, state_summary, work_location_label,
};
use crate::settings_section::SettingsSection;
use crate::theme;

pub const MENU_WIDTH: f32 = 250.;
pub(crate) const TITLE_AVATAR_SIZE: f32 = 28.;
pub(crate) const OWN_AVATAR_DISC: &str = "shell/title-bar/own-status/avatar/disc";
pub(crate) const SETTINGS_MORE_TAG: &str = "shell/title-bar/settings-more";
const VIEW_ACCOUNT_URL: &str = "https://myaccount.microsoft.com";
const TITLE_PRESENCE_SIZE: f32 = 14.;
const HEADER_AVATAR_SIZE: f32 = 40.;
const PRESENCE_OVERHANG: f32 = 2.;
const TITLE_BUTTON_INSET: f32 = 2.;
const OWN_STATUS_BUTTON_SIZE: f32 =
    TITLE_AVATAR_SIZE + 2. * (PRESENCE_OVERHANG + TITLE_BUTTON_INSET);
const PRESENCE_ICON_SIZE: f32 = 14.;
const ICON_COLUMN: f32 = 16.;
const HEADER_INSET: f32 = 24.;

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
    let summary = state_summary(&state.own_status, state.own_presence_kind(), &Local::now());
    let context = MenuContext {
        app: app.clone(),
        email: state.own_email.clone(),
        note: state.own_status.note.as_ref().map(|note| note.text.clone()),
        status: state.own_status.clone(),
        summary,
        person: person.clone(),
    };
    let avatar = with_presence_badge(
        person_avatar(
            &state.directory,
            Some(&person.user_id),
            &person.display_name,
            TITLE_AVATAR_SIZE,
        ),
        presence,
        TITLE_AVATAR_SIZE,
        TITLE_PRESENCE_SIZE,
        theme::background(),
    )
    .debug_selector(|| OWN_AVATAR_DISC.into());
    let toggle = app.clone();
    Some(
        div()
            .debug_selector(|| "shell/title-bar/own-status".into())
            .flex_none()
            .child(
                Button::new("title-own-status")
                    .tooltip(version_tooltip())
                    .ghost()
                    .p_0()
                    .size(px(OWN_STATUS_BUTTON_SIZE))
                    .rounded_full()
                    .child(
                        div()
                            .debug_selector(|| "shell/title-bar/own-status/avatar".into())
                            .p(px(PRESENCE_OVERHANG))
                            .child(avatar),
                    )
                    .dropdown_menu_with_anchor(Anchor::TopRight, move |popup, window, cx| {
                        status_menu(popup, window, cx, &context)
                    })
                    .on_open_change(move |open, _, cx| {
                        let open = *open;
                        toggle.update(cx, |state, cx| state.set_status_menu_open(open, cx));
                    }),
            )
            .into_any_element(),
    )
}

fn version_tooltip() -> String {
    let build_id = crate::updater::running_version();
    format!(
        "{} {}\nBuild {build_id}",
        app_base::APP_NAME,
        crate::format::short_version(build_id, env!("CARGO_PKG_VERSION")),
    )
}

fn status_icon(kind: PresenceKind) -> gpui_kit::component::Icon {
    let (name, color) = status_glyph(kind).unwrap_or(("status_ring", theme::text_faint()));
    symbol(name, PRESENCE_ICON_SIZE, color)
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
                .child(
                    h_flex()
                        .id("status-menu-view-account")
                        .gap(px(4.))
                        .items_center()
                        .cursor_pointer()
                        .text_size(px(12.))
                        .text_color(theme::text_soft())
                        .hover(|link| link.text_color(theme::accent_text()))
                        .child(tr("status_menu.view_account"))
                        .child(menu_icon(IconName::ExternalLink).size(px(12.)))
                        .on_click(|_, _, cx| cx.open_url(VIEW_ACCOUNT_URL)),
                )
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
        .item(pause_item(&context.app, window, cx))
        .item(work_location_item(context, window, cx))
        .item(
            PopupMenuItem::new(tr("status_menu.set_status_message"))
                .icon(menu_icon(IconName::Pencil))
                .on_click(move |_, _, cx| {
                    message_app.update(cx, |state, cx| state.request_status_message(cx));
                }),
        )
}

fn pause_item(
    app: &Entity<AppState>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenuItem {
    let paused_until = app.read(cx).notifications_paused_until;
    let paused = pause::is_paused(paused_until, Utc::now());
    let label = pause::menu_label(paused_until, Local::now());
    let app = app.clone();
    let submenu = PopupMenu::build(window, cx, move |menu, _, _| {
        let resume_app = app.clone();
        let menu = STATUS_DURATIONS.iter().fold(menu, |menu, duration| {
            let app = app.clone();
            let duration = *duration;
            menu.item(
                PopupMenuItem::new(duration.label()).on_click(move |_, _, cx| {
                    let until = duration.expires_at(Local::now());
                    app.update(cx, |state, cx| state.pause_notifications(until, cx));
                }),
            )
        });
        if !paused {
            return menu;
        }
        menu.separator().item(
            PopupMenuItem::new(tr("status_menu.resume_notifications"))
                .icon(menu_icon(IconName::BellRing))
                .on_click(move |_, _, cx| {
                    resume_app.update(cx, |state, cx| state.resume_notifications(cx));
                }),
        )
    });
    PopupMenuItem::submenu(label, submenu).icon(menu_icon(if paused {
        IconName::BellOff
    } else {
        IconName::Bell
    }))
}

pub fn settings_more_button(app: &Entity<AppState>) -> AnyElement {
    let app = app.clone();
    div()
        .debug_selector(|| SETTINGS_MORE_TAG.into())
        .flex_none()
        .child(
            Button::new("title-settings-more")
                .ghost()
                .icon(IconName::Ellipsis)
                .tooltip(tr("shell.settings_and_more"))
                .dropdown_menu_with_anchor(Anchor::TopRight, move |popup, window, cx| {
                    settings_more_menu(popup, window, cx, &app)
                }),
        )
        .into_any_element()
}

fn settings_more_menu(
    popup: PopupMenu,
    _window: &mut Window,
    cx: &mut Context<PopupMenu>,
    app: &Entity<AppState>,
) -> PopupMenu {
    let settings_app = app.clone();
    let chat_list_app = app.clone();
    let translation_app = app.clone();
    let notification_app = app.clone();
    let call_app = app.clone();
    let zoom_app = app.clone();
    let ring_app = app.clone();
    let demo = app.read(cx).mode.demo;
    let popup = popup
        .min_w(px(MENU_WIDTH))
        .item(
            PopupMenuItem::new(tr("common.settings"))
                .icon(menu_icon(IconName::Settings))
                .on_click(move |_, _, cx| {
                    settings_app.update(cx, |state, cx| {
                        state.open_settings(SettingsSection::General, cx)
                    });
                }),
        )
        .separator()
        .item(
            PopupMenuItem::new(tr("status_menu.chat_list"))
                .icon(menu_icon(IconName::LayoutList))
                .on_click(move |_, _, cx| {
                    chat_list_app.update(cx, |state, cx| {
                        state.open_settings(SettingsSection::Chats, cx)
                    });
                }),
        )
        .item(
            PopupMenuItem::new(tr("status_menu.translation"))
                .icon(menu_icon(IconName::Languages))
                .on_click(move |_, _, cx| {
                    translation_app.update(cx, |state, cx| {
                        state.open_settings(SettingsSection::General, cx)
                    });
                }),
        )
        .item(
            PopupMenuItem::new(tr("status_menu.notification_settings"))
                .icon(menu_icon(IconName::Bell))
                .on_click(move |_, _, cx| {
                    notification_app.update(cx, |state, cx| {
                        state.open_settings(SettingsSection::Notifications, cx)
                    });
                }),
        )
        .item(
            PopupMenuItem::new(tr("status_menu.test_call"))
                .icon(menu_icon(IconName::Phone))
                .on_click(move |_, _, cx| {
                    call_app.update(cx, |state, cx| state.start_test_call(cx));
                }),
        )
        .item(PopupMenuItem::element(move |_, cx| {
            super::window_zoom::menu_row(&zoom_app, cx)
        }));
    if !demo {
        return popup;
    }
    popup.item(
        PopupMenuItem::new(tr("status_menu.simulate_incoming"))
            .icon(menu_icon(IconName::Phone))
            .on_click(move |_, _, cx| {
                ring_app.update(cx, |state, cx| state.demo_incoming_ring(cx));
            }),
    )
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
        let pick = |label: String, kind: WorkLocationKind| {
            let app = app.clone();
            PopupMenuItem::new(label)
                .icon(work_location_icon(Some(kind)))
                .on_click(move |_, _, cx| {
                    app.update(cx, |state, cx| state.set_own_work_location(Some(kind), cx));
                })
        };
        let clear_app = app.clone();
        let menu = menu
            .item(PopupMenuItem::label(tr("status_menu.work_location_today")))
            .item(pick(tr("status_menu.office"), WorkLocationKind::Office))
            .item(pick(tr("status_menu.remote"), WorkLocationKind::Remote));
        if current.is_none() {
            return menu;
        }
        menu.separator().item(
            PopupMenuItem::new(tr("status_menu.clear_work_location"))
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
            PopupMenuItem::submenu(tr("status_menu.duration"), durations)
                .icon(menu_icon(IconName::Clock))
                .disabled(target.is_none()),
        )
        .item(
            PopupMenuItem::new(tr("status_menu.reset_status"))
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
