use std::time::{Duration, Instant};

use super::rules::{ChatKind, Incoming, Preview};

pub const MAX_VISIBLE: usize = 3;
pub const MESSAGE_DURATION: Duration = Duration::from_secs(6);
pub const MENTION_DURATION: Duration = Duration::from_secs(10);
pub const MAX_TOTAL: Duration = Duration::from_secs(20);
pub const RESUME_REMAINING: Duration = Duration::from_secs(2);
pub const SENT_DURATION: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Copy)]
pub struct ToastTimer {
    first_shown: Instant,
    started: Instant,
    duration: Duration,
    paused: Option<Instant>,
    paused_total: Duration,
}

impl ToastTimer {
    pub fn new(now: Instant, mention: bool) -> Self {
        ToastTimer {
            first_shown: now,
            started: now,
            duration: base_duration(mention),
            paused: None,
            paused_total: Duration::ZERO,
        }
    }

    pub fn restart(&mut self, now: Instant, mention: bool) {
        self.started = now;
        self.duration = base_duration(mention);
        if self.paused.is_some() {
            self.paused = Some(now);
        }
    }

    pub fn pause(&mut self, now: Instant) {
        self.paused.get_or_insert(now);
    }

    pub fn resume(&mut self, now: Instant) {
        if let Some(paused_at) = self.paused.take() {
            self.paused_total += now.saturating_duration_since(paused_at);
            self.started = now;
            self.duration = RESUME_REMAINING;
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused.is_some()
    }

    fn elapsed(&self, now: Instant) -> Duration {
        self.paused
            .unwrap_or(now)
            .saturating_duration_since(self.started)
    }

    pub fn fraction_left(&self, now: Instant) -> f32 {
        let left = self.duration.saturating_sub(self.elapsed(now));
        (left.as_secs_f32() / self.duration.as_secs_f32()).clamp(0., 1.)
    }

    pub fn expired(&self, now: Instant) -> bool {
        if self.paused.is_some() {
            return false;
        }
        self.elapsed(now) >= self.duration
            || now
                .saturating_duration_since(self.first_shown)
                .saturating_sub(self.paused_total)
                >= MAX_TOTAL
    }
}

fn base_duration(mention: bool) -> Duration {
    if mention {
        MENTION_DURATION
    } else {
        MESSAGE_DURATION
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplyState {
    Closed,
    Open,
    Sent,
    Failed,
}

#[derive(Debug, Clone)]
pub struct ToastModel {
    pub id: u64,
    pub conversation_id: String,
    pub message_id: String,
    pub kind: ChatKind,
    pub chat_title: String,
    pub sender_id: Option<String>,
    pub sender_name: String,
    pub preview: Preview,
    pub mentions_me: bool,
    pub count: u32,
    pub timer: ToastTimer,
    pub reply: ReplyState,
    pub reply_text: String,
    pub hovered: bool,
    pub time: String,
}

impl ToastModel {
    pub fn holds_open(&self) -> bool {
        self.reply != ReplyState::Closed
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PushOutcome {
    pub id: u64,
    pub bundled: bool,
    pub became_mention: bool,
}

impl PushOutcome {
    pub fn plays_sound(self, throttle_allows: bool) -> bool {
        self.became_mention || (!self.bundled && throttle_allows)
    }
}

#[derive(Default)]
pub struct ToastStack {
    toasts: Vec<ToastModel>,
    next_id: u64,
}

impl ToastStack {
    pub fn push(&mut self, incoming: &Incoming, now: Instant) -> PushOutcome {
        if let Some(existing) = self
            .toasts
            .iter_mut()
            .find(|toast| toast.conversation_id == incoming.conversation_id)
        {
            let became_mention = incoming.mentions_me && !existing.mentions_me;
            existing.count += 1;
            existing.mentions_me |= incoming.mentions_me;
            existing.message_id = incoming.message_id.clone();
            existing.sender_id = incoming.sender_id.clone();
            existing.sender_name = incoming.sender_name.clone();
            existing.preview = incoming.preview.clone();
            let mention = existing.mentions_me;
            existing.timer.restart(now, mention);
            return PushOutcome {
                id: existing.id,
                bundled: true,
                became_mention,
            };
        }
        self.next_id += 1;
        self.toasts.push(ToastModel {
            id: self.next_id,
            conversation_id: incoming.conversation_id.clone(),
            message_id: incoming.message_id.clone(),
            kind: incoming.kind.clone(),
            chat_title: incoming.chat_title.clone(),
            sender_id: incoming.sender_id.clone(),
            sender_name: incoming.sender_name.clone(),
            preview: incoming.preview.clone(),
            mentions_me: incoming.mentions_me,
            count: 1,
            timer: ToastTimer::new(now, incoming.mentions_me),
            reply: ReplyState::Closed,
            reply_text: String::new(),
            hovered: false,
            time: String::new(),
        });
        PushOutcome {
            id: self.next_id,
            bundled: false,
            became_mention: false,
        }
    }

    pub fn get(&self, id: u64) -> Option<&ToastModel> {
        self.toasts.iter().find(|toast| toast.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut ToastModel> {
        self.toasts.iter_mut().find(|toast| toast.id == id)
    }

    pub fn remove(&mut self, id: u64) -> Option<ToastModel> {
        let index = self.toasts.iter().position(|toast| toast.id == id)?;
        Some(self.toasts.remove(index))
    }

    pub fn remove_conversation(&mut self, conversation_id: &str) -> Vec<u64> {
        let ids: Vec<u64> = self
            .toasts
            .iter()
            .filter(|toast| toast.conversation_id == conversation_id)
            .map(|toast| toast.id)
            .collect();
        self.toasts
            .retain(|toast| toast.conversation_id != conversation_id);
        ids
    }

    pub fn visible(&self) -> &[ToastModel] {
        let start = self.toasts.len().saturating_sub(MAX_VISIBLE);
        &self.toasts[start..]
    }

    pub fn hidden_count(&self) -> usize {
        self.toasts.len().saturating_sub(MAX_VISIBLE)
    }

    pub fn is_empty(&self) -> bool {
        self.toasts.is_empty()
    }

    pub fn expired(&self, now: Instant) -> Vec<u64> {
        self.toasts
            .iter()
            .filter(|toast| !toast.holds_open() && toast.timer.expired(now))
            .map(|toast| toast.id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn incoming(conversation: &str, mention: bool) -> Incoming {
        Incoming {
            conversation_id: conversation.into(),
            message_id: "m".into(),
            kind: ChatKind::Direct,
            chat_title: "Chat".into(),
            sender_id: None,
            sender_name: "Mara".into(),
            preview: Preview::Text("Hallo".into()),
            mentions_me: mention,
            muted: false,
        }
    }

    #[test]
    fn same_chat_bundles_into_one_toast_with_counter() {
        let mut stack = ToastStack::default();
        let now = Instant::now();
        let first = stack.push(&incoming("a", false), now);
        let second = stack.push(&incoming("a", false), now + Duration::from_secs(2));
        assert!(!first.bundled);
        assert!(second.bundled);
        assert_eq!(first.id, second.id);
        assert_eq!(stack.get(first.id).unwrap().count, 2);
        assert_eq!(stack.visible().len(), 1);
    }

    #[test]
    fn bundling_restarts_timer() {
        let mut stack = ToastStack::default();
        let now = Instant::now();
        let id = stack.push(&incoming("a", false), now).id;
        stack.push(&incoming("a", false), now + Duration::from_secs(5));
        assert!(stack.expired(now + Duration::from_secs(7)).is_empty());
        assert_eq!(stack.expired(now + Duration::from_secs(11)), vec![id]);
    }

    #[test]
    fn mention_in_burst_upgrades_and_plays_sound() {
        let mut stack = ToastStack::default();
        let now = Instant::now();
        stack.push(&incoming("a", false), now);
        let outcome = stack.push(&incoming("a", true), now);
        assert!(outcome.became_mention);
        assert!(outcome.plays_sound(false));
        assert!(stack.get(outcome.id).unwrap().mentions_me);
        let again = stack.push(&incoming("a", true), now);
        assert!(!again.plays_sound(true));
    }

    #[test]
    fn only_first_of_burst_plays_sound() {
        let mut stack = ToastStack::default();
        let now = Instant::now();
        assert!(stack.push(&incoming("a", false), now).plays_sound(true));
        assert!(!stack.push(&incoming("a", false), now).plays_sound(true));
        assert!(!stack.push(&incoming("b", false), now).plays_sound(false));
    }

    #[test]
    fn shows_three_newest_and_counts_the_rest() {
        let mut stack = ToastStack::default();
        let now = Instant::now();
        for chat in ["a", "b", "c", "d", "e"] {
            stack.push(&incoming(chat, false), now);
        }
        let visible: Vec<&str> = stack
            .visible()
            .iter()
            .map(|toast| toast.conversation_id.as_str())
            .collect();
        assert_eq!(visible, ["c", "d", "e"]);
        assert_eq!(stack.hidden_count(), 2);
    }

    #[test]
    fn mention_lasts_ten_seconds() {
        let now = Instant::now();
        let timer = ToastTimer::new(now, true);
        assert!(!timer.expired(now + Duration::from_secs(9)));
        assert!(timer.expired(now + Duration::from_secs(10)));
    }

    #[test]
    fn hard_cap_beats_restarts() {
        let mut stack = ToastStack::default();
        let start = Instant::now();
        let id = stack.push(&incoming("a", false), start).id;
        for second in [5, 10, 15, 19] {
            stack.push(&incoming("a", false), start + Duration::from_secs(second));
        }
        assert!(stack.expired(start + Duration::from_secs(19)).is_empty());
        assert_eq!(stack.expired(start + Duration::from_secs(20)), vec![id]);
    }

    #[test]
    fn pause_stops_expiry_and_resume_leaves_two_seconds() {
        let now = Instant::now();
        let mut timer = ToastTimer::new(now, false);
        timer.pause(now + Duration::from_secs(1));
        assert!(!timer.expired(now + Duration::from_secs(60)));
        let resumed = now + Duration::from_secs(60);
        timer.resume(resumed);
        assert!(!timer.expired(resumed + Duration::from_millis(1900)));
        assert!(timer.expired(resumed + Duration::from_secs(2)));
    }

    #[test]
    fn open_reply_field_holds_toast() {
        let mut stack = ToastStack::default();
        let now = Instant::now();
        let id = stack.push(&incoming("a", false), now).id;
        stack.get_mut(id).unwrap().reply = ReplyState::Open;
        assert!(stack.expired(now + Duration::from_secs(60)).is_empty());
    }

    #[test]
    fn remove_conversation_returns_ids() {
        let mut stack = ToastStack::default();
        let now = Instant::now();
        let id = stack.push(&incoming("a", false), now).id;
        stack.push(&incoming("b", false), now);
        assert_eq!(stack.remove_conversation("a"), vec![id]);
        assert_eq!(stack.visible().len(), 1);
    }
}
