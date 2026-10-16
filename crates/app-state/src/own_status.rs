use std::sync::Arc;

use chrono::{DateTime, Datelike, Duration, NaiveTime, TimeZone, Utc};
use gpui_kit::*;
use teams_core::{
    ForcedAvailability, ForcedKind, PresenceStatus, StatusNote, WorkLocation, WorkLocationKind,
    WorkLocationSource,
};

use crate::app_state::{AppEvent, AppState};
use crate::backend::Engine;
use crate::data::PresenceKind;

pub const NOTE_LIMIT: usize = 280;
const WORK_LOCATION_FAILED: &str = "Work location couldn't be set. Try again.";
pub const STATUS_CHOICES: [ForcedKind; 6] = [
    ForcedKind::Available,
    ForcedKind::Busy,
    ForcedKind::DoNotDisturb,
    ForcedKind::BeRightBack,
    ForcedKind::Away,
    ForcedKind::Offline,
];
pub const STATUS_DURATIONS: [StatusDuration; 5] = [
    StatusDuration::HalfHour,
    StatusDuration::OneHour,
    StatusDuration::TwoHours,
    StatusDuration::Today,
    StatusDuration::ThisWeek,
];
pub const NOTE_DURATIONS: [NoteDuration; 5] = [
    NoteDuration::Never,
    NoteDuration::OneHour,
    NoteDuration::FourHours,
    NoteDuration::Today,
    NoteDuration::ThisWeek,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusDuration {
    HalfHour,
    OneHour,
    TwoHours,
    Today,
    ThisWeek,
}

impl StatusDuration {
    pub fn label(self) -> &'static str {
        match self {
            StatusDuration::HalfHour => "30 minutes",
            StatusDuration::OneHour => "1 hour",
            StatusDuration::TwoHours => "2 hours",
            StatusDuration::Today => "Today",
            StatusDuration::ThisWeek => "This week",
        }
    }

    pub fn expires_at<Zone: TimeZone>(self, now: DateTime<Zone>) -> DateTime<Utc> {
        match self {
            StatusDuration::HalfHour => after(&now, Duration::minutes(30)),
            StatusDuration::OneHour => after(&now, Duration::hours(1)),
            StatusDuration::TwoHours => after(&now, Duration::hours(2)),
            StatusDuration::Today => end_of_day(&now),
            StatusDuration::ThisWeek => end_of_week(&now),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteDuration {
    Never,
    OneHour,
    FourHours,
    Today,
    ThisWeek,
}

impl NoteDuration {
    pub const DEFAULT: NoteDuration = NoteDuration::Today;

    pub fn label(self) -> &'static str {
        match self {
            NoteDuration::Never => "Never",
            NoteDuration::OneHour => "1 hour",
            NoteDuration::FourHours => "4 hours",
            NoteDuration::Today => "Today",
            NoteDuration::ThisWeek => "This week",
        }
    }

    pub fn expires_at<Zone: TimeZone>(self, now: DateTime<Zone>) -> Option<DateTime<Utc>> {
        match self {
            NoteDuration::Never => None,
            NoteDuration::OneHour => Some(after(&now, Duration::hours(1))),
            NoteDuration::FourHours => Some(after(&now, Duration::hours(4))),
            NoteDuration::Today => Some(end_of_day(&now)),
            NoteDuration::ThisWeek => Some(end_of_week(&now)),
        }
    }
}

fn after<Zone: TimeZone>(now: &DateTime<Zone>, span: Duration) -> DateTime<Utc> {
    (now.clone() + span).with_timezone(&Utc)
}

fn end_of_local_date<Zone: TimeZone>(now: &DateTime<Zone>, days_ahead: i64) -> DateTime<Utc> {
    let last_moment = NaiveTime::from_hms_milli_opt(23, 59, 59, 999).unwrap_or(NaiveTime::MIN);
    let date = now.date_naive() + Duration::days(days_ahead);
    now.timezone()
        .from_local_datetime(&date.and_time(last_moment))
        .latest()
        .map_or_else(
            || after(now, Duration::days(days_ahead + 1)),
            |moment| moment.with_timezone(&Utc),
        )
}

pub fn end_of_day<Zone: TimeZone>(now: &DateTime<Zone>) -> DateTime<Utc> {
    end_of_local_date(now, 0)
}

pub fn end_of_week<Zone: TimeZone>(now: &DateTime<Zone>) -> DateTime<Utc> {
    let days_since_sunday = i64::from(now.weekday().num_days_from_sunday());
    end_of_local_date(now, 6 - days_since_sunday)
}

pub fn choice_label(kind: ForcedKind) -> &'static str {
    match kind {
        ForcedKind::Away => "Appear away",
        ForcedKind::Offline => "Appear offline",
        other => state_label(other),
    }
}

pub fn state_label(kind: ForcedKind) -> &'static str {
    match kind {
        ForcedKind::Available => "Available",
        ForcedKind::Busy => "Busy",
        ForcedKind::DoNotDisturb => "Do not disturb",
        ForcedKind::BeRightBack => "Be right back",
        ForcedKind::Away => "Away",
        ForcedKind::Offline => "Offline",
    }
}

pub fn presence_kind_for(kind: ForcedKind) -> PresenceKind {
    match kind {
        ForcedKind::Available => PresenceKind::Available,
        ForcedKind::Busy => PresenceKind::Busy,
        ForcedKind::DoNotDisturb => PresenceKind::DoNotDisturb,
        ForcedKind::BeRightBack | ForcedKind::Away => PresenceKind::Away,
        ForcedKind::Offline => PresenceKind::Offline,
    }
}

pub fn until_label<Zone: TimeZone>(expires_at: DateTime<Utc>, now: &DateTime<Zone>) -> String
where
    Zone::Offset: std::fmt::Display,
{
    let local = expires_at.with_timezone(&now.timezone());
    if local.date_naive() == now.date_naive() {
        format!("until {}", local.format("%H:%M"))
    } else {
        format!("until {}", local.format("%a %H:%M"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateSummary {
    pub label: &'static str,
    pub presence: PresenceKind,
    pub until: Option<String>,
}

pub fn state_summary<Zone: TimeZone>(
    status: &PresenceStatus,
    live: PresenceKind,
    now: &DateTime<Zone>,
) -> StateSummary
where
    Zone::Offset: std::fmt::Display,
{
    match status.forced {
        Some(forced) => StateSummary {
            label: state_label(forced.kind),
            presence: presence_kind_for(forced.kind),
            until: forced
                .expires_at
                .map(|expires_at| until_label(expires_at, now)),
        },
        None => StateSummary {
            label: match live.label() {
                "" => "Set status",
                label => label,
            },
            presence: live,
            until: None,
        },
    }
}

pub fn duration_target(status: &PresenceStatus) -> Option<ForcedKind> {
    status
        .forced
        .map(|forced| forced.kind)
        .filter(|kind| *kind != ForcedKind::Available)
}

pub fn with_forced(status: &PresenceStatus, forced: Option<ForcedAvailability>) -> PresenceStatus {
    let availability = match forced {
        Some(forced) => forced.kind.code().to_owned(),
        None => status.availability.clone(),
    };
    PresenceStatus {
        availability,
        forced,
        ..status.clone()
    }
}

pub fn with_note(status: &PresenceStatus, note: Option<StatusNote>) -> PresenceStatus {
    PresenceStatus {
        note,
        ..status.clone()
    }
}

pub fn work_location_label(location: Option<WorkLocation>) -> &'static str {
    match location {
        None => "Set work location",
        Some(WorkLocation {
            kind: WorkLocationKind::Office,
            source: WorkLocationSource::Set,
        }) => "In the office",
        Some(WorkLocation {
            kind: WorkLocationKind::Office,
            source: WorkLocationSource::Scheduled,
        }) => "Planned in the office",
        Some(WorkLocation {
            kind: WorkLocationKind::Office,
            source: WorkLocationSource::Verified,
        }) => "Checked in to the office",
        Some(WorkLocation {
            kind: WorkLocationKind::Remote,
            source: WorkLocationSource::Scheduled,
        }) => "Planned to work remotely",
        Some(WorkLocation {
            kind: WorkLocationKind::Remote,
            ..
        }) => "Working remotely",
    }
}

pub fn with_work_location(
    status: &PresenceStatus,
    work_location: Option<WorkLocation>,
) -> PresenceStatus {
    PresenceStatus {
        work_location,
        ..status.clone()
    }
}

pub fn clamp_note(text: &str) -> String {
    text.chars().take(NOTE_LIMIT).collect()
}

pub fn demo_status() -> PresenceStatus {
    PresenceStatus {
        availability: "Available".to_owned(),
        ..PresenceStatus::default()
    }
}

impl AppState {
    pub fn own_presence_kind(&self) -> PresenceKind {
        match &self.directory.me {
            Some(me) => self.directory.presence_of(&me.user_id).kind(),
            None => PresenceKind::Unknown,
        }
    }

    pub fn set_status_menu_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.status_menu_open != open {
            self.status_menu_open = open;
            cx.notify();
        }
    }

    pub fn request_status_message(&mut self, cx: &mut Context<Self>) {
        self.status_message_request = true;
        cx.emit(AppEvent::StatusMessage);
        cx.notify();
    }

    pub fn take_status_message_request(&mut self) -> bool {
        std::mem::take(&mut self.status_message_request)
    }

    pub fn open_notification_settings(&mut self, cx: &mut Context<Self>) {
        cx.emit(AppEvent::NotificationSettings);
    }

    pub fn refresh_own_status(&mut self, cx: &mut Context<Self>) {
        let Some(engine) = self.engine.clone().filter(|_| !self.mode.demo) else {
            return;
        };
        drop(crate::runtime::spawn(async move {
            engine.refresh_own_status().await
        }));
        cx.notify();
    }

    pub fn apply_own_status(&mut self, status: PresenceStatus, cx: &mut Context<Self>) {
        self.own_status = status;
        cx.emit(AppEvent::Directory);
        cx.notify();
    }

    pub fn set_own_availability(
        &mut self,
        forced: Option<ForcedAvailability>,
        cx: &mut Context<Self>,
    ) {
        let Some(engine_or_demo) = self.own_status_engine(cx) else {
            return;
        };
        let previous_status = self.own_status.clone();
        let previous_presence = self.own_presence_kind();
        self.own_status = with_forced(&previous_status, forced);
        if let Some(forced) = forced {
            self.show_own_presence(presence_kind_for(forced.kind), cx);
        }
        let Some(engine) = engine_or_demo else {
            return;
        };
        self.run_chat_action(
            "Status",
            async move { engine.set_own_availability(forced).await },
            move |state, cx| {
                state.own_status = previous_status;
                state.show_own_presence(previous_presence, cx);
            },
            |_, _| {},
            cx,
        );
    }

    pub fn set_own_status_note(&mut self, note: Option<StatusNote>, cx: &mut Context<Self>) {
        let Some(engine_or_demo) = self.own_status_engine(cx) else {
            return;
        };
        let previous_status = self.own_status.clone();
        self.own_status = with_note(&previous_status, note.clone());
        cx.emit(AppEvent::Directory);
        cx.notify();
        let Some(engine) = engine_or_demo else {
            return;
        };
        self.run_chat_action(
            "Status message",
            async move { engine.set_own_status_note(note).await },
            move |state, cx| {
                state.own_status = previous_status;
                cx.emit(AppEvent::Directory);
                cx.notify();
            },
            |_, _| {},
            cx,
        );
    }

    pub fn set_own_work_location(
        &mut self,
        kind: Option<WorkLocationKind>,
        cx: &mut Context<Self>,
    ) {
        let Some(engine_or_demo) = self.own_status_engine(cx) else {
            return;
        };
        let previous_status = self.own_status.clone();
        let optimistic = kind.map(|kind| WorkLocation {
            kind,
            source: WorkLocationSource::Set,
        });
        self.own_status = with_work_location(&previous_status, optimistic);
        cx.emit(AppEvent::Directory);
        cx.notify();
        let Some(engine) = engine_or_demo else {
            return;
        };
        let location = kind.map(|kind| (kind, end_of_day(&chrono::Local::now())));
        let receiver =
            crate::runtime::spawn(async move { engine.set_own_work_location(location).await });
        cx.spawn(async move |this, cx| {
            let succeeded = matches!(receiver.await, Ok(Ok(())));
            this.update(cx, |state, cx| {
                if succeeded {
                    return;
                }
                state.own_status = previous_status;
                state.raise_notice(WORK_LOCATION_FAILED.to_owned(), None, cx);
            })
            .ok();
        })
        .detach();
    }

    fn show_own_presence(&mut self, kind: PresenceKind, cx: &mut Context<Self>) {
        if let Some(me) = self
            .directory
            .me
            .clone()
            .filter(|_| kind != PresenceKind::Unknown)
        {
            self.directory.set_presence(&me.user_id, kind);
        }
        cx.emit(AppEvent::Directory);
        cx.notify();
    }

    fn own_status_engine(&mut self, cx: &mut Context<Self>) -> Option<Option<Arc<Engine>>> {
        if self.mode.read_only {
            self.raise_notice("Read-only mode: status not changed".to_owned(), None, cx);
            return None;
        }
        match self.engine.clone() {
            Some(engine) => Some(Some(engine)),
            None if self.mode.demo => Some(None),
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Duration, FixedOffset, TimeZone, Utc};
    use teams_core::{
        ForcedAvailability, ForcedKind, PresenceStatus, WorkLocation, WorkLocationKind,
        WorkLocationSource,
    };

    use super::{
        NOTE_LIMIT, NoteDuration, STATUS_CHOICES, StateSummary, StatusDuration, choice_label,
        clamp_note, duration_target, end_of_day, end_of_week, state_summary, until_label,
        with_forced, work_location_label,
    };
    use crate::data::PresenceKind;

    fn zone() -> FixedOffset {
        FixedOffset::east_opt(2 * 3600).unwrap()
    }

    fn local(day: u32, hour: u32, minute: u32) -> DateTime<FixedOffset> {
        zone()
            .with_ymd_and_hms(2026, 10, day, hour, minute, 0)
            .unwrap()
    }

    fn utc(day: u32, hour: u32, minute: u32, second: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, day, hour, minute, second)
            .unwrap()
    }

    #[test]
    fn relative_durations_add_to_now() {
        let now = local(9, 14, 0);
        assert_eq!(StatusDuration::HalfHour.expires_at(now), utc(9, 12, 30, 0));
        assert_eq!(StatusDuration::OneHour.expires_at(now), utc(9, 13, 0, 0));
        assert_eq!(StatusDuration::TwoHours.expires_at(now), utc(9, 14, 0, 0));
        assert_eq!(
            NoteDuration::FourHours.expires_at(now),
            Some(utc(9, 16, 0, 0))
        );
        assert_eq!(
            NoteDuration::OneHour.expires_at(now),
            Some(utc(9, 13, 0, 0))
        );
        assert_eq!(NoteDuration::Never.expires_at(now), None);
    }

    #[test]
    fn today_ends_at_local_midnight() {
        let expected = utc(9, 21, 59, 59) + Duration::milliseconds(999);
        assert_eq!(StatusDuration::Today.expires_at(local(9, 14, 0)), expected);
        assert_eq!(
            NoteDuration::Today.expires_at(local(9, 0, 5)),
            Some(expected)
        );
    }

    #[test]
    fn today_uses_the_local_date_not_the_utc_date() {
        let now = local(10, 0, 30);
        let expected = utc(10, 21, 59, 59) + Duration::milliseconds(999);
        assert_eq!(end_of_day(&now), expected);
    }

    #[test]
    fn week_ends_saturday_night_with_sunday_as_first_day() {
        let saturday = utc(10, 21, 59, 59) + Duration::milliseconds(999);
        let friday = local(9, 14, 0);
        assert_eq!(StatusDuration::ThisWeek.expires_at(friday), saturday);
        assert_eq!(end_of_week(&local(10, 8, 0)), saturday);
        assert_eq!(end_of_week(&local(4, 8, 0)), saturday);
        let next_saturday = utc(17, 21, 59, 59) + Duration::milliseconds(999);
        assert_eq!(end_of_week(&local(11, 8, 0)), next_saturday);
    }

    #[test]
    fn until_label_shows_the_time_today_and_the_day_later() {
        let now = local(9, 14, 0);
        assert_eq!(until_label(utc(9, 13, 30, 0), &now), "until 15:30");
        assert_eq!(until_label(utc(10, 13, 30, 0), &now), "until Sat 15:30");
    }

    #[test]
    fn forced_state_wins_over_the_live_presence_in_the_summary() {
        let now = local(9, 14, 0);
        let status = PresenceStatus {
            forced: Some(ForcedAvailability {
                kind: ForcedKind::DoNotDisturb,
                expires_at: Some(utc(9, 13, 30, 0)),
            }),
            ..PresenceStatus::default()
        };
        assert_eq!(
            state_summary(&status, PresenceKind::Available, &now),
            StateSummary {
                label: "Do not disturb",
                presence: PresenceKind::DoNotDisturb,
                until: Some("until 15:30".into()),
            }
        );
        let automatic = state_summary(&PresenceStatus::default(), PresenceKind::Away, &now);
        assert_eq!(automatic.label, "Away");
        assert_eq!(automatic.until, None);
        let unknown = state_summary(&PresenceStatus::default(), PresenceKind::Unknown, &now);
        assert_eq!(unknown.label, "Set status");
    }

    #[test]
    fn duration_needs_a_forced_state_other_than_available() {
        let forced = |kind| PresenceStatus {
            forced: Some(ForcedAvailability {
                kind,
                expires_at: None,
            }),
            ..PresenceStatus::default()
        };
        assert_eq!(duration_target(&PresenceStatus::default()), None);
        assert_eq!(duration_target(&forced(ForcedKind::Available)), None);
        assert_eq!(
            duration_target(&forced(ForcedKind::Busy)),
            Some(ForcedKind::Busy)
        );
    }

    #[test]
    fn optimistic_forced_state_sets_availability_and_reset_keeps_it() {
        let busy = ForcedAvailability {
            kind: ForcedKind::Busy,
            expires_at: None,
        };
        let status = with_forced(&PresenceStatus::default(), Some(busy));
        assert_eq!(status.availability, "Busy");
        assert_eq!(status.forced, Some(busy));
        let reset = with_forced(&status, None);
        assert_eq!(reset.availability, "Busy");
        assert_eq!(reset.forced, None);
    }

    #[test]
    fn note_text_is_cut_at_the_limit_by_characters() {
        let long = "ä".repeat(NOTE_LIMIT + 20);
        assert_eq!(clamp_note(&long).chars().count(), NOTE_LIMIT);
        assert_eq!(clamp_note("short"), "short");
    }

    #[test]
    fn work_location_labels_follow_teams() {
        let location = |kind, source| Some(WorkLocation { kind, source });
        assert_eq!(work_location_label(None), "Set work location");
        assert_eq!(
            work_location_label(location(WorkLocationKind::Office, WorkLocationSource::Set)),
            "In the office"
        );
        assert_eq!(
            work_location_label(location(
                WorkLocationKind::Office,
                WorkLocationSource::Scheduled
            )),
            "Planned in the office"
        );
        assert_eq!(
            work_location_label(location(WorkLocationKind::Remote, WorkLocationSource::Set)),
            "Working remotely"
        );
    }

    #[test]
    fn labels_follow_the_teams_menu() {
        let labels: Vec<&str> = STATUS_CHOICES
            .iter()
            .map(|kind| choice_label(*kind))
            .collect();
        assert_eq!(
            labels,
            [
                "Available",
                "Busy",
                "Do not disturb",
                "Be right back",
                "Appear away",
                "Appear offline"
            ]
        );
        assert_eq!(NoteDuration::DEFAULT, NoteDuration::Today);
    }
}
