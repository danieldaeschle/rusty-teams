use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::rows::Receipt;

pub(super) const RECEIPT_FADE: Duration = Duration::from_millis(600);

/// Marks a receipt that turned from sent to read while the chat was open, never one seen on first paint.
#[derive(Default)]
pub(super) struct ReceiptFade {
    conversation_id: String,
    previous: HashMap<String, Receipt>,
    fading: HashMap<String, Instant>,
}

impl ReceiptFade {
    /// Returns true when a new fade started, so the caller can rebuild once it is over.
    pub(super) fn apply(
        &mut self,
        conversation_id: &str,
        settled: bool,
        receipts: &mut HashMap<String, Receipt>,
        now: Instant,
    ) -> bool {
        if self.conversation_id != conversation_id || !settled {
            self.conversation_id = conversation_id.to_owned();
            self.previous.clear();
            self.fading.clear();
            if !settled {
                return false;
            }
        }
        let mut started = false;
        let baseline_known = !self.previous.is_empty();
        for (message_id, receipt) in receipts.iter() {
            if baseline_known
                && *receipt == Receipt::Read
                && self.previous.get(message_id) == Some(&Receipt::Sent)
            {
                self.fading.insert(message_id.clone(), now);
                started = true;
            }
        }
        self.fading
            .retain(|_, since| now.duration_since(*since) < RECEIPT_FADE);
        self.previous.clone_from(receipts);
        for (message_id, receipt) in receipts.iter_mut() {
            if *receipt == Receipt::Read && self.fading.contains_key(message_id) {
                *receipt = Receipt::ReadLive;
            }
        }
        started
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipts(entries: &[(&str, Receipt)]) -> HashMap<String, Receipt> {
        entries
            .iter()
            .map(|(id, receipt)| ((*id).to_owned(), *receipt))
            .collect()
    }

    #[test]
    fn the_first_paint_never_fades() {
        let mut fade = ReceiptFade::default();
        let now = Instant::now();
        let mut first = receipts(&[("a", Receipt::Read)]);
        assert!(!fade.apply("chat", true, &mut first, now));
        assert_eq!(first["a"], Receipt::Read);
    }

    #[test]
    fn a_change_while_receipts_load_does_not_fade() {
        let mut fade = ReceiptFade::default();
        let now = Instant::now();
        let mut cached = receipts(&[("a", Receipt::Sent)]);
        fade.apply("chat", false, &mut cached, now);
        let mut fetched = receipts(&[("a", Receipt::Read)]);
        assert!(!fade.apply("chat", true, &mut fetched, now));
        assert_eq!(fetched["a"], Receipt::Read);
    }

    #[test]
    fn a_live_sent_to_read_change_fades_once() {
        let mut fade = ReceiptFade::default();
        let now = Instant::now();
        let mut baseline = receipts(&[("a", Receipt::Sent), ("b", Receipt::Sent)]);
        fade.apply("chat", true, &mut baseline, now);
        let mut live = receipts(&[("a", Receipt::Read), ("b", Receipt::Sent)]);
        assert!(fade.apply("chat", true, &mut live, now));
        assert_eq!(live["a"], Receipt::ReadLive);
        assert_eq!(live["b"], Receipt::Sent);
        let mut later = receipts(&[("a", Receipt::Read), ("b", Receipt::Sent)]);
        assert!(!fade.apply("chat", true, &mut later, now + RECEIPT_FADE));
        assert_eq!(later["a"], Receipt::Read);
    }

    #[test]
    fn switching_chats_drops_the_baseline() {
        let mut fade = ReceiptFade::default();
        let now = Instant::now();
        let mut first_chat = receipts(&[("a", Receipt::Sent)]);
        fade.apply("one", true, &mut first_chat, now);
        let mut other_chat = receipts(&[("a", Receipt::Read)]);
        assert!(!fade.apply("two", true, &mut other_chat, now));
    }
}
