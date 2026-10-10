use std::collections::HashMap;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use store::{ChannelNotificationLevel, ChannelNotifications};

use crate::call::CallModel;

pub const SOUND_THROTTLE: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    #[default]
    BottomRight,
}

impl Corner {
    pub const ALL: [Corner; 4] = [
        Corner::TopLeft,
        Corner::TopRight,
        Corner::BottomLeft,
        Corner::BottomRight,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Corner::TopLeft => "Top left",
            Corner::TopRight => "Top right",
            Corner::BottomLeft => "Bottom left",
            Corner::BottomRight => "Bottom right",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Corner::TopLeft => "top_left",
            Corner::TopRight => "top_right",
            Corner::BottomLeft => "bottom_left",
            Corner::BottomRight => "bottom_right",
        }
    }

    pub fn from_key(key: &str) -> Option<Corner> {
        Corner::ALL.into_iter().find(|corner| corner.key() == key)
    }

    pub fn is_bottom(self) -> bool {
        matches!(self, Corner::BottomLeft | Corner::BottomRight)
    }

    pub fn is_right(self) -> bool {
        matches!(self, Corner::TopRight | Corner::BottomRight)
    }

    pub fn next(self) -> Corner {
        let index = Corner::ALL
            .iter()
            .position(|corner| *corner == self)
            .unwrap_or(0);
        Corner::ALL[(index + 1) % Corner::ALL.len()]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub sound: bool,
    pub mentions_only: bool,
    pub preview: bool,
    pub corner: Corner,
    pub flash: bool,
    pub close_to_tray: bool,
    pub do_not_disturb: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            sound: true,
            mentions_only: false,
            preview: true,
            corner: Corner::BottomRight,
            flash: true,
            close_to_tray: true,
            do_not_disturb: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatKind {
    Direct,
    Group { member_count: usize },
    Channel { team: String, channel: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Preview {
    Text(String),
    Image,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChannelSignals {
    pub mentions_channel: bool,
    pub in_my_thread: bool,
    pub hidden: bool,
    pub is_reply: bool,
    pub notifications: ChannelNotifications,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incoming {
    pub conversation_id: String,
    pub message_id: String,
    pub kind: ChatKind,
    pub chat_title: String,
    pub sender_id: Option<String>,
    pub sender_name: String,
    pub preview: Preview,
    pub mentions_me: bool,
    pub muted: bool,
    pub signals: ChannelSignals,
    pub created_at: DateTime<Utc>,
}

impl Incoming {
    pub fn is_direct(&self) -> bool {
        self.kind == ChatKind::Direct
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Environment {
    pub chat_in_foreground: bool,
    pub system_quiet: bool,
    pub own_do_not_disturb: bool,
    pub in_call: bool,
}

pub fn in_call(server_in_call: bool, native_call: Option<&CallModel>) -> bool {
    server_in_call || native_call.is_some_and(CallModel::is_active)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    pub toast: bool,
    pub sound: bool,
}

pub fn channel_alerts(incoming: &Incoming) -> bool {
    if !matches!(incoming.kind, ChatKind::Channel { .. }) || incoming.mentions_me {
        return true;
    }
    let signals = incoming.signals;
    if signals.hidden {
        return false;
    }
    if signals.mentions_channel || signals.in_my_thread {
        return true;
    }
    let notifications = signals.notifications;
    notifications.level != ChannelNotificationLevel::Off
        && (!signals.is_reply || notifications.include_replies)
}

fn channel_toasts(incoming: &Incoming) -> bool {
    if !matches!(incoming.kind, ChatKind::Channel { .. }) || incoming.mentions_me {
        return true;
    }
    let signals = incoming.signals;
    signals.mentions_channel
        || signals.in_my_thread
        || signals.notifications.level == ChannelNotificationLevel::BannerAndFeed
}

const SILENT: Decision = Decision {
    toast: false,
    sound: false,
};

pub fn decide(settings: &Settings, incoming: &Incoming, environment: Environment) -> Decision {
    let suppressed = !channel_alerts(incoming)
        || !channel_toasts(incoming)
        || environment.chat_in_foreground
        || environment.system_quiet
        || environment.own_do_not_disturb
        || settings.do_not_disturb
        || (incoming.muted && !incoming.mentions_me)
        || (settings.mentions_only && !incoming.mentions_me && !incoming.is_direct());
    if suppressed {
        return SILENT;
    }
    Decision {
        toast: true,
        sound: settings.sound && !environment.in_call,
    }
}

#[derive(Default)]
pub struct SoundThrottle {
    last_played: HashMap<String, Instant>,
}

impl SoundThrottle {
    pub fn allow(&mut self, conversation_id: &str, now: Instant) -> bool {
        let allowed = self
            .last_played
            .get(conversation_id)
            .is_none_or(|last| now.duration_since(*last) >= SOUND_THROTTLE);
        if allowed {
            self.last_played.insert(conversation_id.to_owned(), now);
        }
        allowed
    }
}

pub fn should_flash(settings: &Settings, toast_shown: bool, window_active: bool) -> bool {
    settings.flash && toast_shown && !window_active
}

#[cfg(test)]
mod tests {
    use calling::{CallState, EndReason};

    use super::*;

    fn incoming(kind: ChatKind) -> Incoming {
        Incoming {
            conversation_id: "chat".into(),
            message_id: "1".into(),
            kind,
            chat_title: "Chat".into(),
            sender_id: Some("u1".into()),
            sender_name: "Mara".into(),
            preview: Preview::Text("Hello".into()),
            mentions_me: false,
            muted: false,
            signals: ChannelSignals::default(),
            created_at: Utc::now(),
        }
    }

    fn channel() -> Incoming {
        incoming(ChatKind::Channel {
            team: "Team".into(),
            channel: "General".into(),
        })
    }

    fn group() -> Incoming {
        incoming(ChatKind::Group { member_count: 5 })
    }

    #[test]
    fn plain_message_gets_toast_and_sound() {
        let decision = decide(&Settings::default(), &group(), Environment::default());
        assert_eq!(
            decision,
            Decision {
                toast: true,
                sound: true
            }
        );
    }

    #[test]
    fn foreground_chat_is_silent() {
        let environment = Environment {
            chat_in_foreground: true,
            ..Default::default()
        };
        assert_eq!(decide(&Settings::default(), &group(), environment), SILENT);
    }

    #[test]
    fn focus_assist_and_dnd_are_silent() {
        for environment in [
            Environment {
                system_quiet: true,
                ..Default::default()
            },
            Environment {
                own_do_not_disturb: true,
                ..Default::default()
            },
        ] {
            assert_eq!(decide(&Settings::default(), &group(), environment), SILENT);
        }
        let settings = Settings {
            do_not_disturb: true,
            ..Default::default()
        };
        assert_eq!(decide(&settings, &group(), Environment::default()), SILENT);
    }

    #[test]
    fn in_call_follows_server_presence_or_a_live_native_call() {
        let model = |state| {
            let mut model = CallModel::incoming("Bea", "8:orgid:b", "Bea");
            model.state = state;
            model
        };
        let since = std::time::Instant::now();
        assert!(!in_call(false, None));
        assert!(in_call(true, None));
        assert!(in_call(false, Some(&model(CallState::Connecting))));
        assert!(in_call(false, Some(&model(CallState::Connected { since }))));
        assert!(in_call(
            false,
            Some(&model(CallState::Reconnecting { since }))
        ));
        assert!(!in_call(false, Some(&model(CallState::Idle))));
        assert!(!in_call(
            false,
            Some(&model(CallState::Ended {
                reason: EndReason::LocalHangup
            }))
        ));
    }

    #[test]
    fn call_keeps_toast_but_drops_sound() {
        let environment = Environment {
            in_call: true,
            ..Default::default()
        };
        let decision = decide(&Settings::default(), &group(), environment);
        assert_eq!(
            decision,
            Decision {
                toast: true,
                sound: false
            }
        );
    }

    #[test]
    fn muted_chat_only_notifies_for_mentions() {
        let mut muted = group();
        muted.muted = true;
        assert_eq!(
            decide(&Settings::default(), &muted, Environment::default()),
            SILENT
        );
        muted.mentions_me = true;
        assert!(decide(&Settings::default(), &muted, Environment::default()).toast);
    }

    #[test]
    fn mentions_only_keeps_direct_and_mentions() {
        let settings = Settings {
            mentions_only: true,
            ..Default::default()
        };
        assert_eq!(decide(&settings, &group(), Environment::default()), SILENT);
        assert!(
            decide(
                &settings,
                &incoming(ChatKind::Direct),
                Environment::default()
            )
            .toast
        );
        let mut mention = group();
        mention.mentions_me = true;
        assert!(decide(&settings, &mention, Environment::default()).toast);
    }

    fn channel_with(level: ChannelNotificationLevel, include_replies: bool) -> Incoming {
        let mut message = channel();
        message.signals.notifications = ChannelNotifications {
            level,
            include_replies,
        };
        message
    }

    fn toast_of(incoming: &Incoming) -> bool {
        decide(&Settings::default(), incoming, Environment::default()).toast
    }

    #[test]
    fn banner_and_activity_posts_toast_and_feed() {
        let post = channel_with(ChannelNotificationLevel::BannerAndFeed, false);
        assert!(channel_alerts(&post));
        assert!(toast_of(&post));
    }

    #[test]
    fn activity_only_posts_feed_without_a_toast() {
        let post = channel_with(ChannelNotificationLevel::Feed, false);
        assert!(channel_alerts(&post));
        assert_eq!(
            decide(&Settings::default(), &post, Environment::default()),
            SILENT
        );
        assert!(channel_alerts(&channel()));
    }

    #[test]
    fn off_channel_stays_silent_except_for_mentions_and_my_threads() {
        let mut post = channel_with(ChannelNotificationLevel::Off, true);
        assert!(!channel_alerts(&post));
        assert_eq!(
            decide(&Settings::default(), &post, Environment::default()),
            SILENT
        );
        post.signals.is_reply = true;
        assert!(!channel_alerts(&post));
        let triggers: [fn(&mut Incoming); 3] = [
            |incoming| incoming.mentions_me = true,
            |incoming| incoming.signals.mentions_channel = true,
            |incoming| incoming.signals.in_my_thread = true,
        ];
        for trigger in triggers {
            let mut mention = channel_with(ChannelNotificationLevel::Off, false);
            trigger(&mut mention);
            assert!(channel_alerts(&mention));
            assert!(toast_of(&mention));
        }
    }

    #[test]
    fn replies_need_include_replies_or_my_thread() {
        for level in [
            ChannelNotificationLevel::Feed,
            ChannelNotificationLevel::BannerAndFeed,
        ] {
            let mut reply = channel_with(level, false);
            reply.signals.is_reply = true;
            assert!(!channel_alerts(&reply));
            reply.signals.in_my_thread = true;
            assert!(channel_alerts(&reply));
            let mut included = channel_with(level, true);
            included.signals.is_reply = true;
            assert!(channel_alerts(&included));
        }
        let mut banner_reply = channel_with(ChannelNotificationLevel::BannerAndFeed, true);
        banner_reply.signals.is_reply = true;
        assert!(toast_of(&banner_reply));
        let mut feed_reply = channel_with(ChannelNotificationLevel::Feed, true);
        feed_reply.signals.is_reply = true;
        assert!(!toast_of(&feed_reply));
    }

    #[test]
    fn hidden_channel_only_alerts_for_mentions_of_me() {
        let triggers: [fn(&mut Incoming); 3] = [
            |incoming| incoming.signals.mentions_channel = true,
            |incoming| incoming.signals.in_my_thread = true,
            |incoming| {
                incoming.signals.notifications.level = ChannelNotificationLevel::BannerAndFeed
            },
        ];
        for trigger in triggers {
            let mut message = channel();
            trigger(&mut message);
            message.signals.hidden = true;
            assert!(!channel_alerts(&message));
        }
        let mut mention = channel();
        mention.signals.hidden = true;
        mention.mentions_me = true;
        assert!(channel_alerts(&mention));
    }

    #[test]
    fn chats_ignore_channel_signals() {
        assert!(channel_alerts(&group()));
        assert!(channel_alerts(&incoming(ChatKind::Direct)));
    }

    #[test]
    fn sound_setting_off_keeps_toast() {
        let settings = Settings {
            sound: false,
            ..Default::default()
        };
        let decision = decide(&settings, &group(), Environment::default());
        assert_eq!(
            decision,
            Decision {
                toast: true,
                sound: false
            }
        );
    }

    #[test]
    fn throttle_allows_once_per_window_per_chat() {
        let mut throttle = SoundThrottle::default();
        let start = Instant::now();
        assert!(throttle.allow("a", start));
        assert!(!throttle.allow("a", start + Duration::from_secs(9)));
        assert!(throttle.allow("b", start + Duration::from_secs(9)));
        assert!(throttle.allow("a", start + Duration::from_secs(10)));
    }

    #[test]
    fn flash_needs_toast_inactive_window_and_setting() {
        let settings = Settings::default();
        assert!(should_flash(&settings, true, false));
        assert!(!should_flash(&settings, true, true));
        assert!(!should_flash(&settings, false, false));
        let off = Settings {
            flash: false,
            ..Default::default()
        };
        assert!(!should_flash(&off, true, false));
    }

    #[test]
    fn corner_keys_round_trip() {
        for corner in Corner::ALL {
            assert_eq!(Corner::from_key(corner.key()), Some(corner));
        }
        assert_eq!(Corner::BottomRight.next(), Corner::TopLeft);
    }
}
