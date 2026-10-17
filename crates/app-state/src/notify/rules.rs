use std::collections::HashMap;
use std::time::{Duration, Instant};

use calling::Progress;
use chrono::{DateTime, Utc};
use store::{ChannelNotificationLevel, ChannelNotifications, MessageImportance};
use teams_core::{
    DesktopNotifications, MeetingChatNotifications, NotificationAccountSettings, NotificationPreset,
};

use super::sound_settings::SoundSettings;
use super::sounds::Ringtone;
use crate::call::{CallKind, CallModel};

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
    pub account: NotificationAccountSettings,
    pub corner: Corner,
    pub flash: bool,
    pub close_to_tray: bool,
    pub do_not_disturb: bool,
    pub mute_in_calls: bool,
    pub sounds: SoundSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            account: NotificationAccountSettings::default(),
            corner: Corner::BottomRight,
            flash: true,
            close_to_tray: true,
            do_not_disturb: false,
            mute_in_calls: false,
            sounds: SoundSettings::default(),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChatSignals {
    pub meeting: bool,
    pub participated: bool,
    pub desktop: DesktopNotifications,
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
    pub chat: ChatSignals,
    pub created_at: DateTime<Utc>,
    pub importance: MessageImportance,
}

impl Incoming {
    pub fn is_direct(&self) -> bool {
        self.kind == ChatKind::Direct
    }

    pub fn is_urgent(&self) -> bool {
        self.importance == MessageImportance::Urgent
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Environment {
    pub chat_in_foreground: bool,
    pub system_quiet: bool,
    pub own_do_not_disturb: bool,
    pub own_busy: bool,
    pub in_call: bool,
}

impl Environment {
    pub fn busy(&self) -> bool {
        self.in_call || self.own_busy
    }
}

pub fn in_call(server_in_call: bool, native_call: Option<&CallModel>) -> bool {
    server_in_call || native_call.is_some_and(CallModel::is_active)
}

pub fn should_ringback(native_call: Option<&CallModel>) -> bool {
    native_call.is_some_and(|model| {
        !model.incoming
            && model.kind != CallKind::Meeting
            && model.is_connecting()
            && matches!(model.progress, Some(Progress::Calling | Progress::Ringing))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    pub toast: bool,
    pub sound: bool,
    pub urgent_sound: bool,
}

pub fn alerts(incoming: &Incoming) -> bool {
    channel_alerts(incoming) && (!incoming.muted || incoming.mentions_me)
}

fn channel_alerts(incoming: &Incoming) -> bool {
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

fn account_toasts(account: &NotificationAccountSettings, incoming: &Incoming) -> bool {
    if matches!(incoming.kind, ChatKind::Channel { .. }) {
        return if incoming.mentions_me {
            account.personal_mentions.shows_banner()
        } else {
            account.preset != NotificationPreset::MentionsAndReplies
                || incoming.signals.in_my_thread
        };
    }
    let chat = incoming.chat;
    if chat.desktop == DesktopNotifications::Off {
        return false;
    }
    if incoming.mentions_me {
        return account.personal_mentions.shows_banner();
    }
    let meeting_muted = chat.meeting
        && match account.meeting_chats {
            MeetingChatNotifications::Mute => true,
            MeetingChatNotifications::MuteUntilJoin => !chat.participated,
            MeetingChatNotifications::Unmute => false,
        };
    !meeting_muted
        && (chat.desktop == DesktopNotifications::On || account.chat_messages.shows_banner())
}

pub fn shows_preview(settings: &Settings) -> bool {
    settings.account.show_preview
}

const SILENT: Decision = Decision {
    toast: false,
    sound: false,
    urgent_sound: false,
};

/// Teams lets urgent messages through Do not disturb ("You'll only get notifications for urgent messages and from your priority contacts").
pub fn decide(settings: &Settings, incoming: &Incoming, environment: Environment) -> Decision {
    let urgent = incoming.is_urgent();
    let suppressed = !alerts(incoming)
        || !channel_toasts(incoming)
        || environment.chat_in_foreground
        || environment.system_quiet
        || (environment.own_do_not_disturb && !urgent)
        || settings.do_not_disturb
        || (settings.mute_in_calls && environment.in_call)
        || (!urgent && !account_toasts(&settings.account, incoming));
    if suppressed {
        return SILENT;
    }
    let busy_allows = settings.sounds.sound_when_busy || !environment.busy();
    let urgent_sound = urgent && settings.sounds.urgent_sound && busy_allows;
    Decision {
        toast: true,
        sound: urgent_sound || (!urgent && settings.sounds.notification_sound && busy_allows),
        urgent_sound,
    }
}

fn busy_allows_sound(settings: &Settings, busy: bool) -> bool {
    !busy || settings.sounds.sound_when_busy
}

pub fn ringtone_for(settings: &Settings, in_call: bool, busy: bool) -> Option<Ringtone> {
    let audible = settings.sounds.call_and_meeting_sound && busy_allows_sound(settings, busy);
    audible.then_some(if in_call {
        Ringtone::Beep
    } else {
        settings.sounds.ringtones.regular
    })
}

pub fn meeting_start_sound(settings: &Settings, busy: bool) -> bool {
    settings.sounds.call_and_meeting_sound && busy_allows_sound(settings, busy)
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
    use teams_core::ActivityAlert;

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
            chat: ChatSignals::default(),
            created_at: Utc::now(),
            importance: Default::default(),
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
                sound: true,
                urgent_sound: false,
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

    fn urgent() -> Incoming {
        Incoming {
            importance: MessageImportance::Urgent,
            ..group()
        }
    }

    #[test]
    fn urgent_passes_do_not_disturb_and_the_account_rules_with_the_urgent_sound() {
        let dnd = Environment {
            own_do_not_disturb: true,
            ..Default::default()
        };
        assert_eq!(
            decide(&Settings::default(), &urgent(), dnd),
            Decision {
                toast: true,
                sound: true,
                urgent_sound: true,
            }
        );
        let mut chat_off = urgent();
        chat_off.chat.desktop = DesktopNotifications::Off;
        assert!(decide(&Settings::default(), &chat_off, Environment::default()).toast);
        let foreground = Environment {
            chat_in_foreground: true,
            ..Default::default()
        };
        assert_eq!(decide(&Settings::default(), &urgent(), foreground), SILENT);
    }

    #[test]
    fn urgent_sound_follows_its_own_setting() {
        let mut settings = Settings::default();
        settings.sounds.urgent_sound = false;
        let decision = decide(&settings, &urgent(), Environment::default());
        assert!(decision.toast && !decision.sound && !decision.urgent_sound);
        settings.sounds.urgent_sound = true;
        settings.sounds.notification_sound = false;
        let decision = decide(&settings, &urgent(), Environment::default());
        assert!(decision.sound && decision.urgent_sound);
        let important = Incoming {
            importance: MessageImportance::High,
            ..group()
        };
        assert!(!decide(&settings, &important, Environment::default()).sound);
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
    fn ringback_plays_only_while_an_outgoing_non_meeting_call_is_connecting() {
        let callees = [("8:orgid:a".to_owned(), "Ana".to_owned())];
        let outgoing = |progress: Option<Progress>, state: CallState| {
            let mut model = CallModel::people("Ana", &callees);
            model.progress = progress;
            model.state = state;
            model
        };
        let since = std::time::Instant::now();
        assert!(!should_ringback(None));
        assert!(!should_ringback(Some(&outgoing(
            None,
            CallState::Connecting
        ))));
        assert!(should_ringback(Some(&outgoing(
            Some(Progress::Calling),
            CallState::Connecting
        ))));
        assert!(should_ringback(Some(&outgoing(
            Some(Progress::Ringing),
            CallState::Connecting
        ))));
        assert!(!should_ringback(Some(&outgoing(
            Some(Progress::Ringing),
            CallState::Connected { since }
        ))));
        let ended = CallState::Ended {
            reason: EndReason::LocalHangup,
        };
        assert!(!should_ringback(Some(&outgoing(
            Some(Progress::Ringing),
            ended
        ))));
        let mut meeting = CallModel::meeting("Standup", false);
        meeting.progress = Some(Progress::Calling);
        assert!(!should_ringback(Some(&meeting)));
        let mut incoming = CallModel::incoming("Bea", "8:orgid:b", "Bea");
        incoming.progress = Some(Progress::Ringing);
        assert!(!should_ringback(Some(&incoming)));
    }

    #[test]
    fn calls_and_busy_keep_the_sound_unless_the_busy_setting_is_off() {
        let busy_environments = [
            Environment {
                in_call: true,
                ..Default::default()
            },
            Environment {
                own_busy: true,
                ..Default::default()
            },
        ];
        let quiet_when_busy = Settings {
            sounds: SoundSettings {
                sound_when_busy: false,
                ..Default::default()
            },
            ..Default::default()
        };
        for environment in busy_environments {
            assert!(decide(&Settings::default(), &group(), environment).sound);
            assert_eq!(
                decide(&quiet_when_busy, &group(), environment),
                Decision {
                    toast: true,
                    sound: false,
                    urgent_sound: false,
                }
            );
        }
        assert!(decide(&quiet_when_busy, &group(), Environment::default()).sound);
    }

    #[test]
    fn incoming_calls_ring_the_chosen_tone_and_beep_during_a_call() {
        let mut settings = Settings::default();
        settings.sounds.ringtones.regular = Ringtone::Bop;
        assert_eq!(ringtone_for(&settings, false, false), Some(Ringtone::Bop));
        assert_eq!(ringtone_for(&settings, true, true), Some(Ringtone::Beep));
        settings.sounds.sound_when_busy = false;
        assert_eq!(ringtone_for(&settings, false, true), None);
        assert_eq!(ringtone_for(&settings, false, false), Some(Ringtone::Bop));
        settings.sounds.call_and_meeting_sound = false;
        assert_eq!(ringtone_for(&settings, false, false), None);
        assert!(!meeting_start_sound(&settings, false));
    }

    #[test]
    fn meeting_start_sound_follows_the_call_and_busy_settings() {
        let mut settings = Settings::default();
        assert!(meeting_start_sound(&settings, false));
        assert!(meeting_start_sound(&settings, true));
        settings.sounds.sound_when_busy = false;
        assert!(!meeting_start_sound(&settings, true));
    }

    #[test]
    fn mute_in_calls_drops_the_toast_only_while_in_a_call() {
        let settings = Settings {
            mute_in_calls: true,
            ..Default::default()
        };
        let in_call = Environment {
            in_call: true,
            ..Default::default()
        };
        assert_eq!(decide(&settings, &group(), in_call), SILENT);
        assert!(decide(&settings, &group(), Environment::default()).toast);
    }

    #[test]
    fn muted_chat_only_alerts_and_notifies_for_mentions() {
        let mut muted = group();
        muted.muted = true;
        assert!(!alerts(&muted));
        assert_eq!(
            decide(&Settings::default(), &muted, Environment::default()),
            SILENT
        );
        muted.mentions_me = true;
        assert!(alerts(&muted));
        assert!(decide(&Settings::default(), &muted, Environment::default()).toast);
    }

    fn with_account(change: impl FnOnce(&mut NotificationAccountSettings)) -> Settings {
        let mut settings = Settings::default();
        change(&mut settings.account);
        settings
    }

    fn toasts(settings: &Settings, incoming: &Incoming) -> bool {
        decide(settings, incoming, Environment::default()).toast
    }

    fn mention(mut incoming: Incoming) -> Incoming {
        incoming.mentions_me = true;
        incoming
    }

    #[test]
    fn chat_message_notifications_off_keeps_personal_mentions() {
        let settings = with_account(|account| account.chat_messages = ActivityAlert::Off);
        assert!(!toasts(&settings, &group()));
        assert!(!toasts(&settings, &incoming(ChatKind::Direct)));
        assert!(toasts(&settings, &mention(group())));
    }

    #[test]
    fn personal_mentions_only_in_activity_give_no_banner() {
        let settings = with_account(|account| account.personal_mentions = ActivityAlert::OnlyFeed);
        assert!(!toasts(&settings, &mention(group())));
        assert!(!toasts(&settings, &mention(channel())));
        assert!(toasts(&settings, &group()));
    }

    #[test]
    fn mentions_and_replies_preset_drops_plain_channel_posts() {
        let settings =
            with_account(|account| account.preset = NotificationPreset::MentionsAndReplies);
        let mut post = channel_with(ChannelNotificationLevel::BannerAndFeed, true);
        assert!(!toasts(&settings, &post));
        post.signals.in_my_thread = true;
        assert!(toasts(&settings, &post));
        assert!(toasts(&settings, &mention(channel())));
        assert!(toasts(&settings, &group()));
    }

    fn meeting_chat(participated: bool) -> Incoming {
        let mut message = group();
        message.chat.meeting = true;
        message.chat.participated = participated;
        message
    }

    #[test]
    fn meeting_chats_stay_quiet_until_i_take_part() {
        let settings = Settings::default();
        assert!(!toasts(&settings, &meeting_chat(false)));
        assert!(toasts(&settings, &meeting_chat(true)));
        assert!(toasts(&settings, &mention(meeting_chat(false))));
    }

    #[test]
    fn meeting_chat_mute_and_unmute() {
        let muted = with_account(|account| account.meeting_chats = MeetingChatNotifications::Mute);
        assert!(!toasts(&muted, &meeting_chat(true)));
        assert!(toasts(&muted, &mention(meeting_chat(true))));
        let unmuted =
            with_account(|account| account.meeting_chats = MeetingChatNotifications::Unmute);
        assert!(toasts(&unmuted, &meeting_chat(false)));
    }

    #[test]
    fn per_chat_desktop_preference_overrides_the_account() {
        let mut silenced = group();
        silenced.chat.desktop = DesktopNotifications::Off;
        assert!(!toasts(&Settings::default(), &silenced));
        assert!(!toasts(&Settings::default(), &mention(silenced)));
        let settings = with_account(|account| account.chat_messages = ActivityAlert::Off);
        let mut selected = group();
        selected.chat.desktop = DesktopNotifications::On;
        assert!(toasts(&settings, &selected));
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
            sounds: SoundSettings {
                notification_sound: false,
                ..Default::default()
            },
            ..Default::default()
        };
        let decision = decide(&settings, &group(), Environment::default());
        assert_eq!(
            decision,
            Decision {
                toast: true,
                sound: false,
                urgent_sound: false,
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
