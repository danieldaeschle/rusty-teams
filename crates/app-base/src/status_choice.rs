use chatsvc::ForcedKind;
use chrono::{DateTime, Datelike, Duration, NaiveTime, TimeZone, Utc};

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

pub fn after<Zone: TimeZone>(now: &DateTime<Zone>, span: Duration) -> DateTime<Utc> {
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
