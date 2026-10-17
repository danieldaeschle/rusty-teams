#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Ringtone {
    #[default]
    Ring,
    Beep,
    Bop,
    Bounce,
    Bubbles,
    BubblesLoud,
    DripDrop,
    Eureka,
    Flutter,
    HighScore,
    NextLevel,
    Plink,
    Remix,
    RingRing,
    Ripple,
    Spacetime,
    WishGranted,
    Wobble,
}

impl Ringtone {
    pub const ALL: [Ringtone; 18] = [
        Ringtone::Ring,
        Ringtone::Beep,
        Ringtone::Bop,
        Ringtone::Bounce,
        Ringtone::Bubbles,
        Ringtone::BubblesLoud,
        Ringtone::DripDrop,
        Ringtone::Eureka,
        Ringtone::Flutter,
        Ringtone::HighScore,
        Ringtone::NextLevel,
        Ringtone::Plink,
        Ringtone::Remix,
        Ringtone::RingRing,
        Ringtone::Ripple,
        Ringtone::Spacetime,
        Ringtone::WishGranted,
        Ringtone::Wobble,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Ringtone::Ring => "Ring",
            Ringtone::Beep => "Beep",
            Ringtone::Bop => "Bop",
            Ringtone::Bounce => "Bounce",
            Ringtone::Bubbles => "Bubbles",
            Ringtone::BubblesLoud => "BubblesLoud",
            Ringtone::DripDrop => "DripDrop",
            Ringtone::Eureka => "Eureka",
            Ringtone::Flutter => "Flutter",
            Ringtone::HighScore => "HighScore",
            Ringtone::NextLevel => "NextLevel",
            Ringtone::Plink => "Plink",
            Ringtone::Remix => "Remix",
            Ringtone::RingRing => "RingRing",
            Ringtone::Ripple => "Ripple",
            Ringtone::Spacetime => "Spacetime",
            Ringtone::WishGranted => "WishGranted",
            Ringtone::Wobble => "Wobble",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Ringtone::Ring => "Default",
            Ringtone::BubblesLoud => "Bubbles (Loud)",
            Ringtone::DripDrop => "Drip Drop",
            Ringtone::HighScore => "High Score",
            Ringtone::NextLevel => "Next Level",
            Ringtone::RingRing => "Ring Ring",
            Ringtone::WishGranted => "Wish Granted",
            other => other.name(),
        }
    }

    pub fn file_name(self) -> &'static str {
        match self {
            Ringtone::Ring => "ring.mp3",
            Ringtone::Beep => "beep.mp3",
            Ringtone::Bop => "bop.mp3",
            Ringtone::Bounce => "bounce.mp3",
            Ringtone::Bubbles => "bubbles.mp3",
            Ringtone::BubblesLoud => "bubblesloud.mp3",
            Ringtone::DripDrop => "dripdrop.mp3",
            Ringtone::Eureka => "eureka.mp3",
            Ringtone::Flutter => "flutter.mp3",
            Ringtone::HighScore => "highscore.mp3",
            Ringtone::NextLevel => "nextlevel.mp3",
            Ringtone::Plink => "plink.mp3",
            Ringtone::Remix => "remix.mp3",
            Ringtone::RingRing => "ringring.mp3",
            Ringtone::Ripple => "ripple.mp3",
            Ringtone::Spacetime => "spacetime.mp3",
            Ringtone::WishGranted => "wishgranted.mp3",
            Ringtone::Wobble => "wobble.mp3",
        }
    }

    pub fn from_name(name: &str) -> Option<Ringtone> {
        Ringtone::ALL
            .into_iter()
            .find(|ringtone| ringtone.name().eq_ignore_ascii_case(name))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingtoneCallType {
    Regular,
    CallGroup,
    CallQueue,
    Delegate,
}

impl RingtoneCallType {
    pub const ALL: [RingtoneCallType; 4] = [
        RingtoneCallType::Regular,
        RingtoneCallType::CallGroup,
        RingtoneCallType::CallQueue,
        RingtoneCallType::Delegate,
    ];

    pub fn name(self) -> &'static str {
        match self {
            RingtoneCallType::Regular => "Regular",
            RingtoneCallType::CallGroup => "CallGroup",
            RingtoneCallType::CallQueue => "CallQueue",
            RingtoneCallType::Delegate => "Delegate",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            RingtoneCallType::Regular => "Calls for you",
            RingtoneCallType::CallGroup => "Forwarded calls",
            RingtoneCallType::CallQueue => "Call queue calls",
            RingtoneCallType::Delegate => "Delegated calls",
        }
    }

    pub fn from_name(name: &str) -> Option<RingtoneCallType> {
        RingtoneCallType::ALL
            .into_iter()
            .find(|call_type| call_type.name() == name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RingtoneAssignments {
    pub regular: Ringtone,
    pub call_group: Ringtone,
    pub call_queue: Ringtone,
    pub delegate: Ringtone,
}

impl RingtoneAssignments {
    pub fn get(&self, call_type: RingtoneCallType) -> Ringtone {
        match call_type {
            RingtoneCallType::Regular => self.regular,
            RingtoneCallType::CallGroup => self.call_group,
            RingtoneCallType::CallQueue => self.call_queue,
            RingtoneCallType::Delegate => self.delegate,
        }
    }

    pub fn set(&mut self, call_type: RingtoneCallType, ringtone: Ringtone) {
        match call_type {
            RingtoneCallType::Regular => self.regular = ringtone,
            RingtoneCallType::CallGroup => self.call_group = ringtone,
            RingtoneCallType::CallQueue => self.call_queue = ringtone,
            RingtoneCallType::Delegate => self.delegate = ringtone,
        }
    }

    pub fn custom(&self) -> Vec<(RingtoneCallType, Ringtone)> {
        RingtoneCallType::ALL
            .into_iter()
            .map(|call_type| (call_type, self.get(call_type)))
            .filter(|(_, ringtone)| *ringtone != Ringtone::Ring)
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertSound {
    Default,
    Vibe,
    Nudge,
    Tap,
    Flick,
    Pluck,
    Summon,
    Ping,
    DefaultUrgent,
    Prioritize,
    Escalate,
    Alarm,
}

impl AlertSound {
    pub const NORMAL: [AlertSound; 8] = [
        AlertSound::Default,
        AlertSound::Vibe,
        AlertSound::Nudge,
        AlertSound::Tap,
        AlertSound::Flick,
        AlertSound::Pluck,
        AlertSound::Summon,
        AlertSound::Ping,
    ];
    pub const URGENT: [AlertSound; 4] = [
        AlertSound::DefaultUrgent,
        AlertSound::Prioritize,
        AlertSound::Escalate,
        AlertSound::Alarm,
    ];

    pub fn name(self) -> &'static str {
        match self {
            AlertSound::Default => "Default",
            AlertSound::Vibe => "Vibe",
            AlertSound::Nudge => "Nudge",
            AlertSound::Tap => "Tap",
            AlertSound::Flick => "Flick",
            AlertSound::Pluck => "Pluck",
            AlertSound::Summon => "Summon",
            AlertSound::Ping => "Ping",
            AlertSound::DefaultUrgent => "DefaultUrgent",
            AlertSound::Prioritize => "Prioritize",
            AlertSound::Escalate => "Escalate",
            AlertSound::Alarm => "Alarm",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            AlertSound::DefaultUrgent => "Default",
            other => other.name(),
        }
    }

    pub fn file_name(self) -> &'static str {
        match self {
            AlertSound::Default | AlertSound::DefaultUrgent => {
                "00_teams_basic_notification_r4_default.mp3"
            }
            AlertSound::Vibe => "a2_teams_basic_notification_r4_vibe.mp3",
            AlertSound::Nudge => "a3_teams_basic_notification_r4_nudge.mp3",
            AlertSound::Tap => "a4_teams_basic_notification_r4_tap.mp3",
            AlertSound::Flick => "a5_teams_basic_notification_r4_flick.mp3",
            AlertSound::Pluck => "a6_teams_basic_notification_r4_pluck.mp3",
            AlertSound::Summon => "a7_teams_basic_notification_r4_summon.mp3",
            AlertSound::Ping => "a8_teams_basic_notification_r4_ping.mp3",
            AlertSound::Prioritize => "b2_teams_urgent_notification_r4_prioritize.mp3",
            AlertSound::Escalate => "b3_teams_urgent_notification_r4_escalate.mp3",
            AlertSound::Alarm => "b4_teams_urgent_notification_r4_alarm.mp3",
        }
    }

    pub fn from_name(name: &str) -> Option<AlertSound> {
        AlertSound::NORMAL
            .into_iter()
            .chain(AlertSound::URGENT)
            .find(|sound| sound.name() == name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallTone {
    OutgoingRinging,
    Reconnecting,
    MeetingJoined,
    CallEnded,
    MeetingLeft,
    MeetingReminder,
}

impl CallTone {
    pub const ALL: [CallTone; 6] = [
        CallTone::OutgoingRinging,
        CallTone::Reconnecting,
        CallTone::MeetingJoined,
        CallTone::CallEnded,
        CallTone::MeetingLeft,
        CallTone::MeetingReminder,
    ];

    pub fn file_name(self) -> &'static str {
        match self {
            CallTone::OutgoingRinging => "Teams_Call_Ringing.mp3",
            CallTone::Reconnecting => "Teams_Call_Reconnecting.mp3",
            CallTone::MeetingJoined => "Teams_Meet_Up_Jump_In.mp3",
            CallTone::CallEnded => "Teams_Call_Ended.mp3",
            CallTone::MeetingLeft => "Teams_Meet_Up_Leave.mp3",
            CallTone::MeetingReminder => "Teams_Meet_Up_Reminder.mp3",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ringtone_names_round_trip_and_files_are_the_lowercase_name() {
        for ringtone in Ringtone::ALL {
            assert_eq!(Ringtone::from_name(ringtone.name()), Some(ringtone));
            assert_eq!(
                ringtone.file_name(),
                format!("{}.mp3", ringtone.name().to_lowercase())
            );
        }
        assert_eq!(Ringtone::from_name("wishgranted"), Some(Ringtone::WishGranted));
        assert_eq!(Ringtone::from_name("Chime"), None);
        assert_eq!(Ringtone::Ring.label(), "Default");
    }

    #[test]
    fn assignments_list_only_the_types_that_do_not_ring_the_default() {
        let mut assignments = RingtoneAssignments::default();
        assert!(assignments.custom().is_empty());
        assignments.set(RingtoneCallType::Delegate, Ringtone::Plink);
        assignments.set(RingtoneCallType::Regular, Ringtone::Bop);
        assert_eq!(
            assignments.custom(),
            vec![
                (RingtoneCallType::Regular, Ringtone::Bop),
                (RingtoneCallType::Delegate, Ringtone::Plink)
            ]
        );
        assert_eq!(assignments.get(RingtoneCallType::CallQueue), Ringtone::Ring);
    }

    #[test]
    fn alert_sounds_round_trip_and_both_defaults_share_one_file() {
        for sound in AlertSound::NORMAL.into_iter().chain(AlertSound::URGENT) {
            assert_eq!(AlertSound::from_name(sound.name()), Some(sound));
            assert!(sound.file_name().ends_with(".mp3"));
        }
        assert_eq!(
            AlertSound::Default.file_name(),
            AlertSound::DefaultUrgent.file_name()
        );
        assert_eq!(AlertSound::DefaultUrgent.label(), "Default");
        assert_eq!(AlertSound::from_name("TACTILE_DEFAULT"), None);
    }
}
