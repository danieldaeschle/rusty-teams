use std::collections::HashMap;

use app_ui_kit::layout::{COMFY_RUNWAY_WIDTH, SIDEBAR_WIDTH};
use app_ui_kit::render::{Block, layout_blocks, render_blocks, strip_image_placeholders};
use chatsvc::{
    CalendarEvent, CalendarSource, DialIn, EventAttendee, Reply, ResponseType, join_info_text,
};
use chrono::{Datelike, Local, Offset, Weekday};
use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app_state::AppState;
use crate::call::week_grid::is_effective_all_day;
use crate::i18n::{tr, tr_args};
use crate::theme::{self, ElevatedSurface as _};

const CARD_WIDTH: f32 = 440.;
const CARD_RADIUS: f32 = 10.;
const CARD_PADDING: f32 = 16.;
const CARD_MARGIN: f32 = 32.;
const SECTION_GAP: f32 = 12.;
const ROW_GAP: f32 = 8.;
const ICON_SIZE: f32 = 16.;
const AVATAR_SIZE: f32 = 28.;
const BACKDROP_OPACITY: f32 = 0.55;
/// Shrink-only agenda cap (Forward-dialog preview pattern — no flex_1).
const AGENDA_MAX_HEIGHT: f32 = 280.;
const EXPANDED_WIDTH: f32 = 0.92;
const EXPANDED_HEIGHT: f32 = 0.88;
const TRACKING_GROUPS: [ResponseType; 4] = [
    ResponseType::Accepted,
    ResponseType::Tentative,
    ResponseType::Declined,
    ResponseType::NotResponded,
];
const COPY_JOIN_INFO: &str = "Copy join info";

pub enum EventPeekEvent {
    Close,
}

pub struct EventPeek {
    state: Entity<AppState>,
    event: CalendarEvent,
    expanded: bool,
    /// Parsed `body_html`, keyed by the HTML it came from.
    agenda: Option<(String, Vec<Block>)>,
    collapsed: Vec<ResponseType>,
    message: Entity<InputState>,
    _observation: Subscription,
}

impl EventEmitter<EventPeekEvent> for EventPeek {}

impl EventPeek {
    pub fn new(
        state: Entity<AppState>,
        event: CalendarEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let event_key = event.object_id.clone();
        let opening_preview = event
            .body_preview
            .as_ref()
            .map(|value| value.chars().count())
            .unwrap_or(0);
        app_base::diagnostics::record(format!(
            "calendar peek: open {event_key} preview_chars={opening_preview}"
        ));
        state.update(cx, |app, cx| app.enrich_calendar_event(&event_key, cx));
        let message = cx.new(|cx| {
            InputState::new(window, cx).placeholder(tr("calendar.peek.message_placeholder"))
        });
        let watched = state.clone();
        // observe (not subscribe): detail enrichment only notifies AppState, it does not emit.
        let observation = cx.observe(&watched, |this, _, cx| {
            this.pull_event(cx);
            cx.notify();
        });
        let mut peek = EventPeek {
            state,
            event,
            expanded: false,
            agenda: None,
            collapsed: Vec::new(),
            message,
            _observation: observation,
        };
        // The clicked calendar block can be a stale clone. If details are already
        // cached, enrich_calendar_event() emits no notify, so pull once now.
        peek.pull_event(cx);
        let pulled = peek
            .event
            .body_preview
            .as_ref()
            .map(|value| value.chars().count())
            .unwrap_or(0);
        if pulled != opening_preview {
            app_base::diagnostics::record(format!(
                "calendar peek: pulled preview_chars={pulled} for {event_key}"
            ));
        }
        peek
    }

    fn pull_event(&mut self, cx: &mut Context<Self>) {
        let key = self.event.object_id.clone();
        let app = self.state.read(cx);
        let best = app
            .calendar_week
            .events
            .iter()
            .chain(app.calendar_events.iter())
            .filter(|event| event.object_id == key)
            .cloned()
            .reduce(|best, candidate| {
                if event_detail_score(&candidate) > event_detail_score(&best) {
                    candidate
                } else {
                    best
                }
            });
        if let Some(event) = best {
            self.event = event;
        }
        self.refresh_agenda();
    }

    fn refresh_agenda(&mut self) {
        let Some(html) = self.event.body_html.as_ref() else {
            self.agenda = None;
            return;
        };
        if self.agenda.as_ref().is_some_and(|(source, _)| source == html) {
            return;
        }
        let blocks = strip_image_placeholders(layout_blocks(&teams_core::html_to_spans(html)));
        self.agenda = Some((html.clone(), blocks));
    }

    fn agenda_element(&self, cx: &App) -> Option<AnyElement> {
        if let Some((_, blocks)) = self.agenda.as_ref().filter(|(_, blocks)| !blocks.is_empty()) {
            return Some(
                div()
                    .w_full()
                    .min_w(px(0.))
                    .text_size(px(14.))
                    .text_color(theme::text())
                    .child(render_blocks(blocks, "calendar-peek-agenda", false, cx))
                    .into_any_element(),
            );
        }
        let preview = self.event.body_preview.clone().filter(|value| !value.is_empty())?;
        Some(
            div()
                .w_full()
                .min_w(px(0.))
                .whitespace_normal()
                .text_size(px(13.))
                .text_color(theme::text_soft())
                .child(preview)
                .into_any_element(),
        )
    }

    fn toggle_expand(&mut self, cx: &mut Context<Self>) {
        self.expanded = !self.expanded;
        cx.notify();
    }

    fn toggle_group(&mut self, group: ResponseType, cx: &mut Context<Self>) {
        if let Some(index) = self.collapsed.iter().position(|kind| *kind == group) {
            self.collapsed.remove(index);
        } else {
            self.collapsed.push(group);
        }
        cx.notify();
    }

    fn toggle_email_organizer(&mut self, cx: &mut Context<Self>) {
        self.state
            .update(cx, |state, cx| state.toggle_rsvp_send_response(cx));
        cx.notify();
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        cx.emit(EventPeekEvent::Close);
    }

    fn respond(&mut self, reply: Reply, cx: &mut Context<Self>) {
        let key = self.event.object_id.clone();
        let send = self.state.read(cx).rsvp_send_response;
        let comment = if send && self.event.source == CalendarSource::Outlook {
            self.message.read(cx).value().to_string()
        } else {
            String::new()
        };
        self.state
            .update(cx, |state, cx| state.respond_to_event(&key, reply, &comment, cx));
        self.close(cx);
    }

    fn respond_series_master(&mut self, reply: Reply, cx: &mut Context<Self>) {
        let Some(master) = self.event.series_master_id.clone() else {
            self.respond(reply, cx);
            return;
        };
        let send = self.state.read(cx).rsvp_send_response;
        let comment = if send && self.event.source == CalendarSource::Outlook {
            self.message.read(cx).value().to_string()
        } else {
            String::new()
        };
        self.state
            .update(cx, |state, cx| state.respond_to_event(&master, reply, &comment, cx));
        self.close(cx);
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key.as_str() == "escape" {
            self.close(cx);
            cx.stop_propagation();
        }
    }

    fn has_scroll_content(&self) -> bool {
        self.agenda
            .as_ref()
            .is_some_and(|(_, blocks)| !blocks.is_empty())
            || self
                .event
                .body_preview
                .as_ref()
                .is_some_and(|value| !value.is_empty())
    }

    fn subject_label(&self) -> String {
        let event = &self.event;
        if event.cancelled {
            format!("{}{}", tr("calendar.peek.canceled_prefix"), event.subject)
        } else if event.subject.is_empty() {
            tr("calendar.peek.no_title")
        } else {
            event.subject.clone()
        }
    }

    fn organizer_label(&self) -> String {
        self.event
            .organizer_name
            .clone()
            .or_else(|| self.event.organizer_address.clone())
            .unwrap_or_default()
    }

    fn expand_button(&self, cx: &mut Context<Self>) -> Button {
        Button::new("calendar-peek-expand")
            .ghost()
            .small()
            .icon(if self.expanded {
                IconName::Minimize2
            } else {
                IconName::Expand
            })
            .on_click(cx.listener(|this, _, _, cx| this.toggle_expand(cx)))
    }

    fn when_row(&self) -> Div {
        h_flex()
            .gap(px(ROW_GAP))
            .items_start()
            .child(icon(IconName::Clock, theme::text_muted()))
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(8.))
                    .items_center()
                    .child(
                        div()
                            .text_size(px(14.))
                            .text_color(theme::text())
                            .child(peek_when_label(&self.event)),
                    )
                    .when(self.event.series, |row| {
                        row.child(
                            h_flex()
                                .gap(px(4.))
                                .items_center()
                                .child(icon(IconName::Repeat, theme::text_muted()))
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .text_color(theme::text_soft())
                                        .child(tr("calendar.peek.series")),
                                ),
                        )
                    }),
            )
    }

    fn location_row(&self) -> Option<Div> {
        let location = location_label(&self.event);
        (!location.is_empty()).then(|| {
            h_flex()
                .gap(px(ROW_GAP))
                .items_start()
                .child(icon(IconName::MapPin, theme::text_muted()))
                .child(
                    div()
                        .text_size(px(14.))
                        .text_color(theme::text())
                        .child(location),
                )
        })
    }

    /// Google-only: which calendar owns the event.
    fn calendar_row(&self) -> Option<Div> {
        if self.event.source != CalendarSource::Google {
            return None;
        }
        let name = self.event.calendar_name.clone().filter(|value| !value.is_empty())?;
        Some(
            h_flex()
                .gap(px(ROW_GAP))
                .items_center()
                .child(icon(IconName::Calendar, theme::text_muted()))
                .child(
                    div()
                        .text_size(px(14.))
                        .text_color(theme::text())
                        .child(name),
                ),
        )
    }

    fn reminder_row(&self) -> Option<Div> {
        if self.event.reminders.is_empty() {
            return None;
        }
        Some(
            h_flex()
                .gap(px(ROW_GAP))
                .items_start()
                .child(icon(IconName::Bell, theme::text_muted()))
                .child(
                    v_flex()
                        .gap(px(2.))
                        .children(self.event.reminders.iter().map(|minutes| {
                            div()
                                .text_size(px(14.))
                                .text_color(theme::text())
                                .child(reminder_label(*minutes))
                        })),
                ),
        )
    }

    fn source_row(&self, _cx: &mut Context<Self>) -> Option<AnyElement> {
        let (title, url) = self.event.source_link.clone()?;
        let open = url.clone();
        Some(
            h_flex()
                .gap(px(ROW_GAP))
                .items_start()
                .child(icon(IconName::Link, theme::text_muted()))
                .child(
                    v_flex()
                        .gap(px(2.))
                        .min_w(px(0.))
                        .child(
                            div()
                                .text_size(px(14.))
                                .text_color(theme::text())
                                .whitespace_normal()
                                .child(title),
                        )
                        .child(
                            div()
                                .id("calendar-peek-view-source")
                                .text_size(px(12.))
                                .text_color(theme::accent_text())
                                .cursor_pointer()
                                .child(tr("calendar.peek.view_source"))
                                .on_click(move |_, _, cx| cx.open_url(&open)),
                        ),
                )
                .into_any_element(),
        )
    }

    fn google_meta_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut rows = Vec::new();
        if let Some(row) = self.calendar_row() {
            rows.push(row.into_any_element());
        }
        if let Some(row) = self.reminder_row() {
            rows.push(row.into_any_element());
        }
        if let Some(row) = self.source_row(cx) {
            rows.push(row);
        }
        rows
    }

    fn render_header(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let event = &self.event;
        let subject = self.subject_label();
        let can_rsvp = event.response != ResponseType::Organizer && event.organizer_name.is_some();
        let organizer = self.organizer_label();
        let initials = app_base::format::initials(&organizer);
        let summary = response_summary(event);

        let header_actions = h_flex()
            .gap(px(8.))
            .children(self.google_action_buttons())
            .children(self.join_chat_buttons(cx));

        let title_row = h_flex()
            .w_full()
            .items_start()
            .justify_between()
            .gap(px(8.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(18.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text())
                    .child(subject),
            )
            .child(self.expand_button(cx));

        let mut header = v_flex()
            .id("calendar-peek-header")
            .w_full()
            .flex_none()
            .gap(px(SECTION_GAP))
            .child(title_row)
            .child(header_actions)
            .child(self.when_row())
            .children(self.location_row())
            .children(self.google_meta_rows(cx));

        if !organizer.is_empty() {
            let invite_line = if can_rsvp {
                let mut args = HashMap::new();
                args.insert("name".to_owned(), organizer.clone());
                tr_args("calendar.peek.invited_you", &args)
            } else {
                organizer.clone()
            };
            header = header.child(
                h_flex()
                    .gap(px(ROW_GAP))
                    .items_start()
                    .child(avatar(initials))
                    .child(
                        v_flex()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .text_color(theme::text())
                                    .child(invite_line),
                            )
                            .children(summary.map(|text| {
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme::text_muted())
                                    .child(text)
                            })),
                    ),
            );
        }

        header
    }

    /// Google-only: Open in Google Calendar + Email guests.
    fn google_action_buttons(&self) -> Vec<AnyElement> {
        if self.event.source != CalendarSource::Google {
            return Vec::new();
        }
        let mut buttons = Vec::new();
        if let Some(url) = self.event.web_link.clone() {
            buttons.push(
                Button::new("calendar-peek-open-google")
                    .outline()
                    .icon(IconName::ExternalLink)
                    .label(tr("calendar.peek.open_in_google"))
                    .on_click(move |_, _, cx| cx.open_url(&url))
                    .into_any_element(),
            );
        }
        let addresses: Vec<String> = self
            .event
            .attendees
            .iter()
            .filter_map(|attendee| attendee.address.clone())
            .filter(|address| !address.is_empty())
            .collect();
        if !addresses.is_empty() {
            let mut mailto = format!("mailto:{}", addresses.join(","));
            if !self.event.subject.is_empty() {
                mailto = format!(
                    "{mailto}?subject={}",
                    percent_encode_query(&self.event.subject)
                );
            }
            buttons.push(
                Button::new("calendar-peek-email-guests")
                    .outline()
                    .icon(IconName::Mail)
                    .label(tr("calendar.peek.email_guests"))
                    .on_click(move |_, _, cx| cx.open_url(&mailto))
                    .into_any_element(),
            );
        }
        buttons
    }

    fn join_chat_buttons(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let event = &self.event;
        let can_join = event.online && !event.cancelled && event.join_url.is_some();
        let join_url = event.join_url.clone();
        let dial_in = event.dial_in.clone();
        let thread_id = event.thread_id.clone();
        let meeting_title = event.subject.clone();
        let mut buttons = Vec::new();
        if can_join {
            let join_state = self.state.clone();
            let url = join_url.clone();
            let title = meeting_title.clone();
            let clipboard = join_url
                .as_deref()
                .map(|url| copy_join_clipboard(url, dial_in.as_ref()));
            buttons.push(
                Button::new("calendar-peek-join")
                    .outline()
                    .label(tr("common.join"))
                    .icon(IconName::Video)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        join_state.update(cx, |state, cx| {
                            state.join_calendar_meeting(url.as_deref(), &title, cx)
                        });
                        this.close(cx);
                    }))
                    .into_any_element(),
            );
            if let Some(info) = clipboard {
                buttons.push(
                    Button::new("calendar-peek-join-menu")
                        .outline()
                        .icon(IconName::ChevronDown)
                        .dropdown_menu(move |menu, _, _| {
                            let info = info.clone();
                            menu.item(
                                PopupMenuItem::new(COPY_JOIN_INFO)
                                    .icon(IconName::Copy)
                                    .on_click(move |_, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            info.clone(),
                                        ))
                                    }),
                            )
                        })
                        .into_any_element(),
                );
            }
        }
        if thread_id.is_some() {
            let chat_state = self.state.clone();
            let chat_thread = thread_id.clone();
            buttons.push(
                Button::new("calendar-peek-chat")
                    .outline()
                    .label(tr("common.chat"))
                    .icon(IconName::MessageSquare)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        chat_state.update(cx, |state, cx| {
                            state.open_meeting_chat(chat_thread.as_deref(), cx)
                        });
                        this.close(cx);
                    }))
                    .into_any_element(),
            );
        }
        buttons
    }

    /// Compact agenda: max_h + scrollbar on the content root (Forward-preview pattern).
    /// A nested scroll wrapper with min_h(0) collapses to 0 height in GPUI.
    fn render_scroll_body(agenda: AnyElement) -> impl IntoElement {
        h_flex()
            .id("calendar-peek-scroll-content")
            .w_full()
            .gap(px(ROW_GAP))
            .items_start()
            .child(icon(IconName::List, theme::text_muted()))
            .child(div().flex_1().min_w(px(0.)).child(agenda))
            .flex_shrink_0()
            .max_h(px(AGENDA_MAX_HEIGHT))
            .overflow_y_scrollbar()
    }

    fn render_expanded(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let toolbar = h_flex()
            .w_full()
            .flex_none()
            .gap(px(8.))
            .items_center()
            .children(self.action_buttons(cx))
            .child(div().flex_1())
            .children(self.google_action_buttons())
            .children(self.join_chat_buttons(cx))
            .child(self.expand_button(cx))
            .child(
                Button::new("calendar-peek-close")
                    .ghost()
                    .small()
                    .icon(IconName::X)
                    .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
            );

        let names = self.attendee_names_line();
        let mut details = v_flex()
            .id("calendar-peek-expanded-details")
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .gap(px(SECTION_GAP))
            .pr(px(CARD_PADDING))
            .child(
                div()
                    .text_size(px(20.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text())
                    .child(self.subject_label()),
            )
            .children(names.map(|names| {
                h_flex()
                    .gap(px(ROW_GAP))
                    .items_start()
                    .child(icon(IconName::Users, theme::text_muted()))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .whitespace_normal()
                            .text_size(px(13.))
                            .text_color(theme::text_soft())
                            .child(names),
                    )
            }))
            .child(self.when_row())
            .children(self.location_row())
            .children(self.google_meta_rows(cx))
            .children(self.rsvp_message_controls(cx));
        if let Some(agenda) = self.agenda_element(cx) {
            details = details.child(
                div()
                    .w_full()
                    .pt(px(SECTION_GAP))
                    .border_t_1()
                    .border_color(theme::border())
                    .child(agenda),
            );
        }

        let body = h_flex()
            .w_full()
            .flex_1()
            .min_h(px(0.))
            .items_start()
            .child(details.overflow_y_scrollbar())
            .child(self.render_tracking(cx));

        v_flex()
            .size_full()
            .gap(px(SECTION_GAP))
            .child(toolbar)
            .child(body)
    }

    fn attendee_names_line(&self) -> Option<String> {
        let organizer = self.organizer_label();
        let mut names: Vec<String> = Vec::new();
        if !organizer.is_empty() {
            names.push(organizer);
        }
        for attendee in &self.event.attendees {
            if !names.contains(&attendee.name) {
                names.push(attendee.name.clone());
            }
        }
        (names.len() > 1).then(|| names.join(", "))
    }

    fn render_tracking(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let organizer = self.organizer_label();
        let mut panel = v_flex()
            .id("calendar-peek-tracking")
            .w(px(SIDEBAR_WIDTH))
            .flex_none()
            .h_full()
            .pl(px(CARD_PADDING))
            .gap(px(SECTION_GAP))
            .border_l_1()
            .border_color(theme::border())
            .child(
                div()
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text())
                    .child(tr("calendar.peek.tracking")),
            );
        if !organizer.is_empty() {
            panel = panel.child(person_row(&organizer, tr("calendar.peek.organizer")));
        }

        let roster: Vec<&EventAttendee> = self
            .event
            .attendees
            .iter()
            .filter(|attendee| attendee.response != ResponseType::Organizer)
            .collect();
        if roster.is_empty() {
            let note = response_summary(&self.event)
                .unwrap_or_else(|| tr("calendar.peek.attendees_unknown"));
            return panel
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(theme::text_muted())
                        .child(note),
                )
                .overflow_y_scrollbar();
        }

        for (index, group) in TRACKING_GROUPS.into_iter().enumerate() {
            let members: Vec<&&EventAttendee> = roster
                .iter()
                .filter(|attendee| attendee.response == group)
                .collect();
            if members.is_empty() {
                continue;
            }
            let collapsed = self.collapsed.contains(&group);
            let mut section = v_flex().gap(px(ROW_GAP)).child(
                h_flex()
                    .id(SharedString::from(format!("calendar-peek-group-{index}")))
                    .gap(px(4.))
                    .items_center()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_group(group, cx)))
                    .child(icon(
                        if collapsed {
                            IconName::ChevronRight
                        } else {
                            IconName::ChevronDown
                        },
                        theme::text_muted(),
                    ))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text())
                            .child(format!("{} {}", group_label(group), members.len())),
                    ),
            );
            if !collapsed {
                section = section.children(members.into_iter().map(|attendee| {
                    let role = if attendee.required {
                        tr("calendar.peek.required")
                    } else {
                        tr("calendar.peek.optional")
                    };
                    person_row(&attendee.name, role)
                }));
            }
            panel = panel.child(section);
        }
        panel.overflow_y_scrollbar()
    }

    fn can_rsvp(&self) -> bool {
        self.event.response != ResponseType::Organizer && self.event.organizer_name.is_some()
    }

    /// Outlook-only: Graph RSVP supports sendResponse + comment. Google does not.
    fn rsvp_message_controls(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if !self.can_rsvp() || self.event.source != CalendarSource::Outlook {
            return Vec::new();
        }
        let send_response = self.state.read(cx).rsvp_send_response;
        let mut controls = vec![
            Checkbox::new("calendar-peek-email-organizer")
                .label(tr("calendar.peek.email_organizer"))
                .checked(send_response)
                .on_click(cx.listener(|this, _, _, cx| this.toggle_email_organizer(cx)))
                .into_any_element(),
        ];
        if send_response {
            controls.push(Input::new(&self.message).into_any_element());
        }
        controls
    }

    /// Accept / Decline / Tentative (when invited) plus the overflow menu with Delete.
    fn action_buttons(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let response = self.event.response;
        let series = self.event.series;
        let has_master = self.event.series_master_id.is_some();
        let peek = cx.entity();
        let mut buttons = Vec::new();
        if self.can_rsvp() {
            buttons.push(rsvp_button(
                "calendar-peek-accept",
                tr("calendar.peek.accept"),
                Reply::Accept,
                response == ResponseType::Accepted,
                series,
                has_master,
                peek.clone(),
            ));
            buttons.push(rsvp_button(
                "calendar-peek-decline",
                tr("calendar.peek.decline"),
                Reply::Decline,
                response == ResponseType::Declined,
                series,
                has_master,
                peek.clone(),
            ));
            buttons.push(rsvp_button(
                "calendar-peek-tentative",
                tr("calendar.peek.tentative_action"),
                Reply::Tentative,
                response == ResponseType::Tentative,
                false,
                false,
                peek.clone(),
            ));
        }
        let delete_state = self.state.clone();
        let delete_key = self.event.object_id.clone();
        buttons.push(
            Button::new("calendar-peek-more")
                .ghost()
                .icon(IconName::Ellipsis)
                .dropdown_menu(move |menu, _, _| {
                    let delete_state = delete_state.clone();
                    let delete_key = delete_key.clone();
                    let delete_peek = peek.clone();
                    menu.item(
                        PopupMenuItem::new(tr("common.delete"))
                            .icon(IconName::Trash)
                            .on_click(move |_, _, cx| {
                                delete_state.update(cx, |state, cx| {
                                    state.delete_calendar_event(&delete_key, cx)
                                });
                                delete_peek.update(cx, |peek, cx| peek.close(cx));
                            }),
                    )
                })
                .into_any_element(),
        );
        buttons
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let can_rsvp = self.can_rsvp();
        v_flex()
            .id("calendar-peek-footer")
            .w_full()
            .flex_none()
            .gap(px(8.))
            .children(self.rsvp_message_controls(cx))
            .child(
                h_flex()
                    .w_full()
                    .gap(px(8.))
                    .items_center()
                    .when(!can_rsvp, |row| row.justify_end())
                    .children(self.action_buttons(cx)),
            )
    }
}

fn avatar(initials: String) -> Div {
    div()
        .size(px(AVATAR_SIZE))
        .flex_none()
        .rounded_full()
        .bg(theme::accent_soft())
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::accent_text())
        .child(initials)
}

fn person_row(name: &str, role: String) -> Div {
    h_flex()
        .gap(px(ROW_GAP))
        .items_center()
        .child(avatar(app_base::format::initials(name)))
        .child(
            v_flex()
                .flex_1()
                .min_w(px(0.))
                .child(
                    div()
                        .text_size(px(14.))
                        .text_color(theme::text())
                        .truncate()
                        .child(name.to_owned()),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::text_muted())
                        .child(role),
                ),
        )
}

fn group_label(group: ResponseType) -> String {
    match group {
        ResponseType::Accepted | ResponseType::Organizer => tr("calendar.peek.accepted"),
        ResponseType::Tentative => tr("calendar.peek.tentative"),
        ResponseType::Declined => tr("calendar.peek.declined"),
        ResponseType::NotResponded => tr("calendar.peek.no_response"),
    }
}

/// Percent-encode a mailto `subject` query value (UTF-8, RFC 3986).
fn percent_encode_query(value: &str) -> String {
    let mut out = String::with_capacity(value.len() * 3);
    for byte in value.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn is_teams_url(url: &str) -> bool {
    url.contains("teams.microsoft.com")
}

fn is_google_meet_url(url: &str) -> bool {
    url.contains("meet.google.com")
}

fn copy_join_clipboard(url: &str, dial_in: Option<&DialIn>) -> String {
    if is_teams_url(url) {
        join_info_text(url, dial_in)
    } else {
        url.to_owned()
    }
}

fn event_detail_score(event: &CalendarEvent) -> u8 {
    u8::from(event.body_html.is_some()) * 32
        + u8::from(!event.attendees.is_empty()) * 16
        + u8::from(
        event
            .body_preview
            .as_ref()
            .is_some_and(|value| !value.is_empty()),
    ) * 8
        + u8::from(
            event
                .location
                .as_ref()
                .is_some_and(|value| !value.is_empty()),
        ) * 4
        + u8::from(event.response_counts.is_some()) * 2
        + u8::from(event.series_master_id.is_some())
}

fn peek_when_label(event: &CalendarEvent) -> String {
    let offset = Local::now().offset().fix();
    if is_effective_all_day(event, offset) {
        let start = event.start.with_timezone(&offset).date_naive();
        let end = event.end.with_timezone(&offset).date_naive();
        if end > start + chrono::Duration::days(1) {
            return format!(
                "{} · {} – {}",
                tr("calendar.peek.all_day"),
                start.format("%a, %b %-d"),
                (end - chrono::Duration::days(1)).format("%a, %b %-d, %Y")
            );
        }
        return format!(
            "{} · {}",
            tr("calendar.peek.all_day"),
            start.format("%a, %b %-d, %Y")
        );
    }
    let start = event.start.with_timezone(&offset);
    let end = event.end.with_timezone(&offset);
    let weekday = localized_weekday(start.weekday());
    let date = format_peek_date(start.date_naive());
    let clock = app_base::format::clock_format();
    if start.date_naive() == end.date_naive() {
        format!(
            "{weekday} {date} {} - {}",
            start.format(clock),
            end.format(clock)
        )
    } else {
        // Multi-day timed (Google-style): date + time on both sides.
        format!(
            "{weekday} {date}, {} – {}, {}",
            start.format(clock),
            format_peek_date(end.date_naive()),
            end.format(clock)
        )
    }
}

fn reminder_label(minutes: u32) -> String {
    if minutes == 0 {
        return tr("calendar.peek.when_starts");
    }
    let mut args = HashMap::new();
    if minutes % 1440 == 0 {
        let days = minutes / 1440;
        args.insert("count".to_owned(), days.to_string());
        return tr_args(
            if days == 1 {
                "calendar.peek.day_before"
            } else {
                "calendar.peek.days_before"
            },
            &args,
        );
    }
    if minutes % 60 == 0 {
        let hours = minutes / 60;
        args.insert("count".to_owned(), hours.to_string());
        return tr_args(
            if hours == 1 {
                "calendar.peek.hour_before"
            } else {
                "calendar.peek.hours_before"
            },
            &args,
        );
    }
    args.insert("count".to_owned(), minutes.to_string());
    tr_args(
        if minutes == 1 {
            "calendar.peek.minute_before"
        } else {
            "calendar.peek.minutes_before"
        },
        &args,
    )
}

fn ui_is_german() -> bool {
    gpui_kit::component::locale().starts_with("de")
}

fn format_peek_date(date: chrono::NaiveDate) -> String {
    if ui_is_german() {
        date.format("%d.%m.%Y").to_string()
    } else {
        date.format("%-m/%-d/%Y").to_string()
    }
}

fn localized_weekday(weekday: Weekday) -> &'static str {
    let de = ui_is_german();
    match (de, weekday) {
        (true, Weekday::Mon) => "Mo",
        (true, Weekday::Tue) => "Di",
        (true, Weekday::Wed) => "Mi",
        (true, Weekday::Thu) => "Do",
        (true, Weekday::Fri) => "Fr",
        (true, Weekday::Sat) => "Sa",
        (true, Weekday::Sun) => "So",
        (false, Weekday::Mon) => "Mon",
        (false, Weekday::Tue) => "Tue",
        (false, Weekday::Wed) => "Wed",
        (false, Weekday::Thu) => "Thu",
        (false, Weekday::Fri) => "Fri",
        (false, Weekday::Sat) => "Sat",
        (false, Weekday::Sun) => "Sun",
    }
}

fn location_label(event: &CalendarEvent) -> String {
    if let Some(location) = event
        .location
        .clone()
        .filter(|value| !value.is_empty())
    {
        return location;
    }
    if !event.online {
        return String::new();
    }
    match event.join_url.as_deref() {
        Some(url) if is_google_meet_url(url) => tr("calendar.peek.google_meet"),
        Some(url) if is_teams_url(url) => tr("calendar.peek.teams_meeting"),
        _ if event.source == CalendarSource::Google => tr("calendar.peek.google_meet"),
        _ => tr("calendar.peek.teams_meeting"),
    }
}

fn response_summary(event: &CalendarEvent) -> Option<String> {
    let counts = event.response_counts?;
    if counts.is_empty() {
        return None;
    }
    let mut parts = vec![
        format!("{} {}", tr("calendar.peek.accepted"), counts.accepted),
        format!("{} {}", tr("calendar.peek.declined"), counts.declined),
    ];
    if counts.tentative > 0 {
        parts.insert(
            1,
            format!("{} {}", tr("calendar.peek.tentative"), counts.tentative),
        );
    }
    parts.push(format!(
        "{} {}",
        tr("calendar.peek.no_response"),
        counts.not_responded
    ));
    Some(parts.join(", "))
}

fn icon(name: IconName, color: Hsla) -> Icon {
    Icon::new(name).size(px(ICON_SIZE)).text_color(color)
}

fn rsvp_button(
    id: &'static str,
    label: String,
    reply: Reply,
    chosen: bool,
    series: bool,
    has_master: bool,
    peek: Entity<EventPeek>,
) -> AnyElement {
    let button = Button::new(id).label(label).when(chosen, |button| button.primary());
    let button = match reply {
        Reply::Accept => button.icon(IconName::Check),
        Reply::Decline => button.icon(IconName::X),
        Reply::Tentative => button.icon(IconName::CalendarClock),
    };
    if series {
        let occurrence = peek.clone();
        let master = peek;
        button
            .dropdown_menu(move |menu, _, _| {
                let occurrence = occurrence.clone();
                let master = master.clone();
                let mut menu = menu.item(
                    PopupMenuItem::new(tr("calendar.peek.this_occurrence")).on_click(
                        move |_, _, cx| {
                            occurrence.update(cx, |peek, cx| peek.respond(reply, cx));
                        },
                    ),
                );
                if has_master {
                    menu = menu.item(
                        PopupMenuItem::new(tr("calendar.peek.entire_series")).on_click(
                            move |_, _, cx| {
                                master.update(cx, |peek, cx| peek.respond_series_master(reply, cx));
                            },
                        ),
                    );
                }
                menu
            })
            .into_any_element()
    } else {
        button
            .on_click(move |_, _, cx| {
                peek.update(cx, |peek, cx| peek.respond(reply, cx));
            })
            .into_any_element()
    }
}

impl Render for EventPeek {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let card = if self.expanded {
            v_flex()
                .id("calendar-peek-card")
                .w(relative(EXPANDED_WIDTH))
                .max_w(px(COMFY_RUNWAY_WIDTH))
                .h(relative(EXPANDED_HEIGHT))
                .child(self.render_expanded(cx))
        } else {
            let header = self.render_header(cx);
            let body = self
                .has_scroll_content()
                .then(|| self.agenda_element(cx))
                .flatten()
                .map(Self::render_scroll_body);
            let footer = self.render_footer(cx);
            // Shrink-only agenda (Forward preview pattern): max_h + scrollbar, no flex_1.
            v_flex()
                .id("calendar-peek-card")
                .w(px(CARD_WIDTH))
                .max_w(relative(1.))
                .max_h(relative(1.))
                .child(header)
                .children(body)
                .child(footer)
        };

        div()
            .id("calendar-peek-layer")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("calendar-peek-backdrop")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(black().opacity(BACKDROP_OPACITY))
                    .occlude()
                    .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
            )
            .child(
                card.m(px(CARD_MARGIN))
                    .p(px(CARD_PADDING))
                    .gap(px(SECTION_GAP))
                    .rounded(px(CARD_RADIUS))
                    .dialog_surface()
                    .text_color(theme::text())
                    .occlude()
                    .overflow_hidden()
                    .capture_key_down(cx.listener(Self::on_key_down)),
            )
    }
}

pub fn open_calendar_event(
    state: Entity<AppState>,
    event: CalendarEvent,
    _: &mut Window,
    cx: &mut App,
) {
    state.update(cx, |state, cx| state.request_calendar_event_peek(event, cx));
}
