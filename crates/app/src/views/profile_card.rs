use std::sync::Arc;

use chrono::Utc;
use gpui_kit::assets::IconName;
use gpui_kit::base::GlobalState;
use gpui_kit::component::{h_flex, tooltip::Tooltip, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::{OrgPerson, PROFILE_LINK_PREFIX, PersonProfile, WorkLocationKind};

use super::avatar::{person_avatar, with_presence};
use super::widgets::icon;
use crate::app_state::{AppHandle, AppState};
use crate::data::{Presence, PresenceKind, presence_kind_of};
use crate::people::resolve_names;
use crate::profile_model::{
    ContactKind, ContactRow, StatusBox, contact_rows, has_organization, local_time_label,
    mailto_url, status_box, status_line, work_location_label,
};
use crate::theme;

const CARD_WIDTH: f32 = 340.;
const CARD_RADIUS: f32 = 10.;
const CARD_PADDING: f32 = 16.;
const CARD_GAP: f32 = 12.;
const CARD_MARGIN: f32 = 8.;
const CLICK_OFFSET: f32 = 4.;
const PHOTO_SIZE: f32 = 56.;
const PERSON_ROW_AVATAR: f32 = 22.;
const STATUS_LINES: usize = 3;
const REPORTS_MAX_HEIGHT: f32 = 168.;
const ACTION_HEIGHT: f32 = 30.;
const SKELETON_HEIGHT: f32 = 10.;

pub enum ProfileCardEvent {
    Close,
}

pub struct ProfileCard {
    app: Entity<AppState>,
    user_id: String,
    history: Vec<String>,
    anchor: Point<Pixels>,
    contact_open: bool,
    organization_open: bool,
    reports_open: bool,
    focus_handle: FocusHandle,
    _observation: Subscription,
}

impl EventEmitter<ProfileCardEvent> for ProfileCard {}

pub fn open_profile(user_id: &str, position: Point<Pixels>, cx: &mut App) {
    let Some(handle) = cx.try_global::<AppHandle>() else {
        return;
    };
    let (app, user_id) = (handle.0.clone(), user_id.to_owned());
    app.update(cx, |state, cx| {
        state.open_profile_card(&user_id, position, cx)
    });
}

/// `true` when the link was a profile link and has been handled.
pub fn open_profile_link(url: &str, position: Point<Pixels>, cx: &mut App) -> bool {
    match url.strip_prefix(PROFILE_LINK_PREFIX) {
        Some(user_id) => {
            open_profile(user_id, position, cx);
            true
        }
        None => false,
    }
}

pub fn opens_profile(element: Stateful<Div>, user_id: Option<&str>) -> Stateful<Div> {
    match user_id {
        Some(user_id) => {
            let user_id = user_id.to_owned();
            element.cursor_pointer().on_click(move |event, _, cx| {
                cx.stop_propagation();
                open_profile(&user_id, event.position(), cx)
            })
        }
        None => element,
    }
}

impl ProfileCard {
    pub fn new(
        app: Entity<AppState>,
        user_id: String,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        let observation = cx.observe(&app, |_, _, cx| cx.notify());
        ProfileCard {
            app,
            user_id,
            history: Vec::new(),
            anchor,
            contact_open: true,
            organization_open: true,
            reports_open: false,
            focus_handle,
            _observation: observation,
        }
    }

    pub fn move_to(&mut self, user_id: String, anchor: Point<Pixels>, cx: &mut Context<Self>) {
        self.history.clear();
        self.anchor = anchor;
        self.show(user_id, false, cx);
    }

    fn show(&mut self, user_id: String, remember: bool, cx: &mut Context<Self>) {
        if remember {
            self.history
                .push(std::mem::replace(&mut self.user_id, user_id));
        } else {
            self.user_id = user_id;
        }
        self.reports_open = false;
        let id = self.user_id.clone();
        self.app.update(cx, |state, cx| {
            state.request_profile(&id, false, cx);
            state.request_avatars(vec![id.clone()], cx);
        });
        cx.notify();
    }

    fn go_back(&mut self, cx: &mut Context<Self>) {
        if let Some(previous) = self.history.pop() {
            self.show(previous, false, cx);
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            cx.emit(ProfileCardEvent::Close);
        }
    }

    fn display_name(&self, profile: Option<&PersonProfile>, cx: &App) -> String {
        profile
            .and_then(|profile| profile.display_name.clone())
            .or_else(|| {
                resolve_names(self.app.read(cx), std::slice::from_ref(&self.user_id))
                    .remove(&self.user_id)
            })
            .unwrap_or_else(|| "Unknown person".to_owned())
    }

    fn presence(&self, profile: Option<&PersonProfile>, cx: &App) -> Presence {
        let directory = self.app.read(cx).directory.presence_of(&self.user_id);
        match (directory, profile.and_then(|profile| profile.availability)) {
            (Presence::Live(kind), _) if kind != PresenceKind::Unknown => directory,
            (_, Some(availability)) => Presence::Live(presence_kind_of(availability)),
            _ => directory,
        }
    }

    fn header(&self, profile: Option<&PersonProfile>, failed: bool, cx: &App) -> Div {
        let name = self.display_name(profile, cx);
        let presence = self.presence(profile, cx);
        let directory = &self.app.read(cx).directory;
        let photo = with_presence(
            person_avatar(directory, Some(&self.user_id), &name, PHOTO_SIZE),
            presence,
            PHOTO_SIZE,
            theme::surface_raised(),
        );
        let availability = presence.kind().label();
        let local_time = profile.and_then(|profile| local_time_label(profile, Utc::now()));
        let subline = status_line(availability, local_time.as_deref());
        let details = v_flex()
            .gap(px(1.))
            .children(profile.into_iter().flat_map(|profile| {
                [&profile.job_title, &profile.department]
                    .into_iter()
                    .flatten()
                    .filter(|text| !text.trim().is_empty())
                    .map(|text| muted_line(text.clone()))
                    .collect::<Vec<_>>()
            }));
        let work_location = profile
            .and_then(|profile| profile.work_location)
            .map(|location| {
                let glyph = match location.kind {
                    WorkLocationKind::Office => IconName::Building2,
                    WorkLocationKind::Remote => IconName::House,
                };
                h_flex()
                    .gap(px(5.))
                    .items_center()
                    .child(icon(glyph, 13., theme::text_muted()))
                    .child(muted_line(work_location_label(location).to_owned()))
            });
        let loading = profile.is_none() && !failed;
        h_flex().gap(px(12.)).items_center().child(photo).child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(2.))
                .child(
                    div()
                        .truncate()
                        .text_size(px(15.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::text())
                        .child(name),
                )
                .when(!subline.is_empty(), |column| {
                    column.child(muted_line(subline))
                })
                .children(work_location)
                .child(details)
                .when(loading, |column| column.child(skeleton(0.6))),
        )
    }

    fn status_box(&self, status: &StatusBox) -> AnyElement {
        let (background, foreground) = if status.out_of_office {
            (theme::amber().opacity(0.12), theme::amber())
        } else {
            (theme::surface(), theme::text_soft())
        };
        let full_text = status.text.clone();
        div()
            .id("profile-status")
            .w_full()
            .px(px(10.))
            .py(px(8.))
            .rounded(px(6.))
            .bg(background)
            .text_color(foreground)
            .text_size(px(12.))
            .line_height(relative(1.4))
            .child(
                div()
                    .line_clamp(STATUS_LINES)
                    .text_ellipsis()
                    .child(status.text.clone()),
            )
            .tooltip(move |window, cx| Tooltip::new(full_text.clone()).build(window, cx))
            .into_any_element()
    }

    fn actions(&self, email: Option<String>, cx: &mut Context<Self>) -> Div {
        let is_me = self
            .app
            .read(cx)
            .directory
            .me
            .as_ref()
            .is_some_and(|me| me.user_id == self.user_id);
        let user_id = self.user_id.clone();
        let (mail_email, copy_email) = (email.clone(), email.clone());
        h_flex()
            .gap(px(8.))
            .when(!is_me, |row| {
                row.child(
                    action_button("profile-chat", "Chat", IconName::MessageSquare, true).on_click(
                        cx.listener(move |this, _, _, cx| {
                            let user_id = user_id.clone();
                            this.app
                                .update(cx, |state, cx| state.open_chat_with(&user_id, cx));
                            cx.emit(ProfileCardEvent::Close);
                        }),
                    ),
                )
            })
            .when_some(mail_email, |row, address| {
                row.child(
                    action_button("profile-email", "Email", IconName::Mail, false).on_click(
                        cx.listener(move |_, _, _, cx| {
                            cx.open_url(&mailto_url(&address));
                            cx.emit(ProfileCardEvent::Close);
                        }),
                    ),
                )
            })
            .when_some(copy_email, |row, address| {
                row.child(
                    action_button("profile-copy-email", "Copy email", IconName::Copy, false)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.app
                                .update(cx, |state, cx| state.copy_profile_email(&address, cx));
                            cx.emit(ProfileCardEvent::Close);
                        })),
                )
            })
    }

    fn section_header(
        &self,
        id: &'static str,
        label: &'static str,
        open: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        h_flex()
            .id(id)
            .gap(px(4.))
            .items_center()
            .cursor_pointer()
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme::text_muted())
            .child(icon(
                if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                },
                14.,
                theme::text_muted(),
            ))
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| {
                match id {
                    "profile-contact-header" => this.contact_open = !this.contact_open,
                    _ => this.organization_open = !this.organization_open,
                }
                cx.notify();
            }))
    }

    fn contact_section(&self, rows: Vec<ContactRow>, cx: &mut Context<Self>) -> Div {
        let open = self.contact_open;
        v_flex()
            .gap(px(6.))
            .child(self.section_header("profile-contact-header", "Contact", open, cx))
            .when(open, |section| {
                section.children(rows.into_iter().map(contact_row))
            })
    }

    fn person_row(
        &self,
        id: String,
        person: &OrgPerson,
        caption: Option<&'static str>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let name = person.display_name.clone().unwrap_or_default();
        let subtitle = caption
            .map(str::to_owned)
            .or_else(|| person.job_title.clone())
            .filter(|text| !text.is_empty());
        let target = person.id.clone();
        let directory = &self.app.read(cx).directory;
        h_flex()
            .id(ElementId::Name(id.into()))
            .gap(px(8.))
            .px(px(4.))
            .py(px(3.))
            .items_center()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(|row| row.bg(theme::row_hover()))
            .child(person_avatar(
                directory,
                Some(&person.id),
                &name,
                PERSON_ROW_AVATAR,
            ))
            .child(
                div()
                    .truncate()
                    .text_size(px(12.5))
                    .text_color(theme::text())
                    .child(name),
            )
            .children(
                subtitle.map(|text| div().flex_1().min_w_0().truncate().child(muted_text(text))),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.show(target.clone(), true, cx)))
    }

    fn organization_section(&self, profile: &PersonProfile, cx: &mut Context<Self>) -> Div {
        let open = self.organization_open;
        let mut section = v_flex().gap(px(4.)).child(self.section_header(
            "profile-organization-header",
            "Organization",
            open,
            cx,
        ));
        if !open {
            return section;
        }
        if let Some(manager) = &profile.manager {
            section = section.child(self.person_row(
                "profile-manager".to_owned(),
                manager,
                Some("Manager"),
                cx,
            ));
        }
        let count = profile.direct_reports.len();
        if count > 0 {
            let label = match count {
                1 => "1 direct report".to_owned(),
                count => format!("{count} direct reports"),
            };
            let reports_open = self.reports_open;
            section = section.child(
                h_flex()
                    .id("profile-reports-toggle")
                    .gap(px(4.))
                    .px(px(4.))
                    .py(px(3.))
                    .items_center()
                    .cursor_pointer()
                    .text_size(px(12.5))
                    .text_color(theme::text_soft())
                    .child(label)
                    .child(icon(
                        if reports_open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        },
                        14.,
                        theme::text_muted(),
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.reports_open = !this.reports_open;
                        cx.notify();
                    })),
            );
            if reports_open {
                let rows: Vec<Stateful<Div>> = profile
                    .direct_reports
                    .iter()
                    .enumerate()
                    .map(|(index, person)| {
                        self.person_row(format!("profile-report-{index}"), person, None, cx)
                    })
                    .collect();
                section = section.child(
                    v_flex()
                        .id("profile-reports")
                        .max_h(px(REPORTS_MAX_HEIGHT))
                        .overflow_y_scroll()
                        .children(rows),
                );
            }
        }
        section
    }

    fn loading_sections(&self) -> Div {
        v_flex()
            .gap(px(8.))
            .child(skeleton(1.))
            .child(skeleton(0.8))
            .child(skeleton(0.5))
    }

    fn failure(&self, cx: &mut Context<Self>) -> Div {
        h_flex()
            .gap(px(6.))
            .text_size(px(12.))
            .text_color(theme::red_soft())
            .child("Profile details could not be loaded.")
            .child(
                div()
                    .id("profile-retry")
                    .cursor_pointer()
                    .underline()
                    .text_color(theme::red_tint())
                    .child("Retry")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let id = this.user_id.clone();
                        this.app
                            .update(cx, |state, cx| state.request_profile(&id, true, cx));
                    })),
            )
    }

    fn body(&self, entry_state: EntryState, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let EntryState { profile, failed } = entry_state;
        let mut parts: Vec<AnyElement> = Vec::new();
        let Some(profile) = profile else {
            parts.push(self.actions(None, cx).into_any_element());
            parts.push(if failed {
                self.failure(cx).into_any_element()
            } else {
                self.loading_sections().into_any_element()
            });
            return parts;
        };
        if let Some(status) = status_box(&profile) {
            parts.push(self.status_box(&status));
        }
        parts.push(self.actions(profile.email.clone(), cx).into_any_element());
        let rows = contact_rows(&profile);
        if !rows.is_empty() {
            parts.push(self.contact_section(rows, cx).into_any_element());
        }
        if has_organization(&profile) {
            parts.push(self.organization_section(&profile, cx).into_any_element());
        }
        if failed {
            parts.push(self.failure(cx).into_any_element());
        }
        parts
    }
}

struct EntryState {
    profile: Option<Arc<PersonProfile>>,
    failed: bool,
}

fn muted_text(text: String) -> Div {
    div()
        .text_size(px(12.))
        .text_color(theme::text_muted())
        .child(text)
}

fn muted_line(text: String) -> Div {
    muted_text(text).truncate()
}

fn skeleton(width_ratio: f32) -> Div {
    div()
        .h(px(SKELETON_HEIGHT))
        .w(relative(width_ratio))
        .rounded(px(4.))
        .bg(theme::border_strong())
}

fn contact_row(row: ContactRow) -> Div {
    let glyph = match row.kind {
        ContactKind::Email => IconName::Mail,
        ContactKind::WorkPhone => IconName::Phone,
        ContactKind::MobilePhone => IconName::Smartphone,
        ContactKind::Office => IconName::MapPin,
    };
    h_flex()
        .gap(px(8.))
        .px(px(4.))
        .items_center()
        .child(icon(glyph, 14., theme::text_muted()))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(12.5))
                .text_color(theme::text())
                .child(row.value),
        )
}

fn action_button(
    id: &'static str,
    label: &'static str,
    glyph: IconName,
    filled: bool,
) -> Stateful<Div> {
    let (foreground, background) = if filled {
        (theme::on_accent(), theme::accent())
    } else {
        (theme::text_soft(), theme::surface())
    };
    h_flex()
        .id(id)
        .h(px(ACTION_HEIGHT))
        .px(px(10.))
        .gap(px(6.))
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .cursor_pointer()
        .bg(background)
        .text_color(foreground)
        .text_size(px(12.5))
        .font_weight(FontWeight::SEMIBOLD)
        .when(!filled, |button| {
            button
                .border_1()
                .border_color(theme::border_strong())
                .hover(|button| button.bg(theme::row_hover()))
        })
        .when(filled, |button| button.hover(|button| button.opacity(0.9)))
        .child(icon(glyph, 14., foreground))
        .child(label)
}

impl Render for ProfileCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entry_state = self
            .app
            .read(cx)
            .profiles
            .get(&self.user_id)
            .map(|entry| EntryState {
                profile: entry.profile.clone(),
                failed: entry.failed,
            })
            .unwrap_or(EntryState {
                profile: None,
                failed: false,
            });
        let profile = entry_state.profile.clone();
        let header = self.header(profile.as_deref(), entry_state.failed, cx);
        let body = self.body(entry_state, cx);
        let back = (!self.history.is_empty()).then(|| {
            h_flex()
                .id("profile-back")
                .gap(px(4.))
                .items_center()
                .cursor_pointer()
                .text_size(px(12.))
                .text_color(theme::text_muted())
                .hover(|button| button.text_color(theme::text()))
                .child(icon(IconName::ArrowLeft, 14., theme::text_muted()))
                .child("Back")
                .on_click(cx.listener(|this, _, _, cx| this.go_back(cx)))
        });
        let card = v_flex()
            .id("profile-card")
            .track_focus(&self.focus_handle)
            .w(px(CARD_WIDTH))
            .p(px(CARD_PADDING))
            .gap(px(CARD_GAP))
            .rounded(px(CARD_RADIUS))
            .border_1()
            .border_color(theme::border_strong())
            .bg(theme::surface_raised())
            .text_color(theme::text())
            .shadow_lg()
            .occlude()
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                GlobalState::suppress_text_selection(cx);
                cx.stop_propagation();
            })
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .children(back)
            .child(header)
            .children(body);
        let offset = point(px(CLICK_OFFSET), px(CLICK_OFFSET));
        div()
            .id("profile-layer")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(ProfileCardEvent::Close)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|_, _, _, cx| cx.emit(ProfileCardEvent::Close)),
            )
            .child(
                deferred(
                    anchored()
                        .position(self.anchor + offset)
                        .anchor(Anchor::TopLeft)
                        .snap_to_window_with_margin(px(CARD_MARGIN))
                        .child(card),
                )
                .with_priority(2),
            )
    }
}
