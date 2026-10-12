use std::time::{Duration, Instant};

pub const BUBBLE_SHOWN: Duration = Duration::from_secs(5);
pub const BUBBLE_MAX: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatBubble {
    pub id: String,
    pub sender_id: Option<String>,
    pub name: String,
    pub text: String,
    pub at: Instant,
}

pub fn push_bubble(bubbles: &mut Vec<ChatBubble>, bubble: ChatBubble) {
    if bubbles.iter().any(|shown| shown.id == bubble.id) {
        return;
    }
    bubbles.push(bubble);
    let surplus = bubbles.len().saturating_sub(BUBBLE_MAX);
    bubbles.drain(..surplus);
}

pub fn shown_bubbles(bubbles: &[ChatBubble], now: Instant) -> Vec<&ChatBubble> {
    bubbles
        .iter()
        .filter(|bubble| now.saturating_duration_since(bubble.at) < BUBBLE_SHOWN)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bubble(id: &str, at: Instant) -> ChatBubble {
        ChatBubble {
            id: id.to_owned(),
            sender_id: None,
            name: "Ana".to_owned(),
            text: "hello".to_owned(),
            at,
        }
    }

    #[test]
    fn only_the_newest_three_stay() {
        let now = Instant::now();
        let mut bubbles = Vec::new();
        for id in ["a", "b", "c", "d"] {
            push_bubble(&mut bubbles, bubble(id, now));
        }
        let ids: Vec<&str> = bubbles.iter().map(|shown| shown.id.as_str()).collect();
        assert_eq!(ids, ["b", "c", "d"]);
    }

    #[test]
    fn a_message_never_shows_twice() {
        let now = Instant::now();
        let mut bubbles = Vec::new();
        push_bubble(&mut bubbles, bubble("a", now));
        push_bubble(&mut bubbles, bubble("a", now));
        assert_eq!(bubbles.len(), 1);
    }

    #[test]
    fn bubbles_vanish_after_five_seconds() {
        let now = Instant::now();
        let bubbles = vec![bubble("a", now)];
        assert_eq!(
            shown_bubbles(&bubbles, now + Duration::from_secs(4)).len(),
            1
        );
        assert!(shown_bubbles(&bubbles, now + BUBBLE_SHOWN).is_empty());
    }
}
