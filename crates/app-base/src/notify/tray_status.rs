//! Shared status labels for the tray menu. Keep in sync with
//! `app-state::own_status::{choice_label, state_label, STATUS_CHOICES, STATUS_DURATIONS}`.

use chatsvc::ForcedKind;

use super::platform::TrayStatusDuration;

pub const STATUS_CHOICES: [ForcedKind; 6] = [
    ForcedKind::Available,
    ForcedKind::Busy,
    ForcedKind::DoNotDisturb,
    ForcedKind::BeRightBack,
    ForcedKind::Away,
    ForcedKind::Offline,
];

pub const STATUS_DURATIONS: [TrayStatusDuration; 5] = [
    TrayStatusDuration::HalfHour,
    TrayStatusDuration::OneHour,
    TrayStatusDuration::TwoHours,
    TrayStatusDuration::Today,
    TrayStatusDuration::ThisWeek,
];

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

pub fn duration_label(duration: TrayStatusDuration) -> &'static str {
    match duration {
        TrayStatusDuration::HalfHour => "30 minutes",
        TrayStatusDuration::OneHour => "1 hour",
        TrayStatusDuration::TwoHours => "2 hours",
        TrayStatusDuration::Today => "Today",
        TrayStatusDuration::ThisWeek => "This week",
    }
}

pub fn duration_target(forced: Option<ForcedKind>) -> Option<ForcedKind> {
    forced.filter(|kind| *kind != ForcedKind::Available)
}

pub fn reset_label() -> &'static str {
    "Reset status"
}

pub fn duration_menu_label() -> &'static str {
    "Duration"
}

pub fn kind_id(kind: ForcedKind) -> &'static str {
    match kind {
        ForcedKind::Available => "status:available",
        ForcedKind::Busy => "status:busy",
        ForcedKind::DoNotDisturb => "status:dnd",
        ForcedKind::BeRightBack => "status:brb",
        ForcedKind::Away => "status:away",
        ForcedKind::Offline => "status:offline",
    }
}

pub fn parse_kind_id(id: &str) -> Option<ForcedKind> {
    match id {
        "status:available" => Some(ForcedKind::Available),
        "status:busy" => Some(ForcedKind::Busy),
        "status:dnd" => Some(ForcedKind::DoNotDisturb),
        "status:brb" => Some(ForcedKind::BeRightBack),
        "status:away" => Some(ForcedKind::Away),
        "status:offline" => Some(ForcedKind::Offline),
        _ => None,
    }
}

pub fn duration_id(duration: TrayStatusDuration) -> &'static str {
    match duration {
        TrayStatusDuration::HalfHour => "status-duration:30m",
        TrayStatusDuration::OneHour => "status-duration:1h",
        TrayStatusDuration::TwoHours => "status-duration:2h",
        TrayStatusDuration::Today => "status-duration:today",
        TrayStatusDuration::ThisWeek => "status-duration:week",
    }
}

pub fn parse_duration_id(id: &str) -> Option<TrayStatusDuration> {
    match id {
        "status-duration:30m" => Some(TrayStatusDuration::HalfHour),
        "status-duration:1h" => Some(TrayStatusDuration::OneHour),
        "status-duration:2h" => Some(TrayStatusDuration::TwoHours),
        "status-duration:today" => Some(TrayStatusDuration::Today),
        "status-duration:week" => Some(TrayStatusDuration::ThisWeek),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_choice_has_a_stable_id() {
        for kind in STATUS_CHOICES {
            assert_eq!(parse_kind_id(kind_id(kind)), Some(kind));
        }
    }

    #[test]
    fn every_duration_has_a_stable_id() {
        for duration in STATUS_DURATIONS {
            assert_eq!(parse_duration_id(duration_id(duration)), Some(duration));
        }
    }

    #[test]
    fn available_has_no_duration_target() {
        assert_eq!(duration_target(Some(ForcedKind::Available)), None);
        assert_eq!(
            duration_target(Some(ForcedKind::DoNotDisturb)),
            Some(ForcedKind::DoNotDisturb)
        );
    }
}
