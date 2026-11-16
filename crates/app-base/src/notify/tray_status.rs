use chatsvc::ForcedKind;

use crate::status_choice::StatusDuration;
pub use crate::status_choice::{STATUS_CHOICES, STATUS_DURATIONS, choice_label};

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

pub fn duration_id(duration: StatusDuration) -> &'static str {
    match duration {
        StatusDuration::HalfHour => "status-duration:30m",
        StatusDuration::OneHour => "status-duration:1h",
        StatusDuration::TwoHours => "status-duration:2h",
        StatusDuration::Today => "status-duration:today",
        StatusDuration::ThisWeek => "status-duration:week",
    }
}

pub fn parse_duration_id(id: &str) -> Option<StatusDuration> {
    match id {
        "status-duration:30m" => Some(StatusDuration::HalfHour),
        "status-duration:1h" => Some(StatusDuration::OneHour),
        "status-duration:2h" => Some(StatusDuration::TwoHours),
        "status-duration:today" => Some(StatusDuration::Today),
        "status-duration:week" => Some(StatusDuration::ThisWeek),
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
