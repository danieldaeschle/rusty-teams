use std::time::Duration;

const RECENT_WINDOW: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptEntry {
    pub id: String,
    pub speaker: String,
    pub text: String,
    pub offset: Duration,
}

#[derive(Debug, Clone, Default)]
pub struct Transcript {
    entries: Vec<TranscriptEntry>,
}

impl Transcript {
    pub fn note(&mut self, entry: TranscriptEntry) {
        let start = self.entries.len().saturating_sub(RECENT_WINDOW);
        let known = self.entries[start..]
            .iter()
            .rposition(|existing| existing.id == entry.id);
        match known {
            Some(position) => {
                let existing = &mut self.entries[start + position];
                existing.speaker = entry.speaker;
                existing.text = entry.text;
            }
            None => self.entries.push(entry),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn filtered(&self, query: &str) -> Vec<&TranscriptEntry> {
        let needle = query.trim().to_lowercase();
        self.entries
            .iter()
            .filter(|entry| {
                needle.is_empty()
                    || entry.text.to_lowercase().contains(&needle)
                    || entry.speaker.to_lowercase().contains(&needle)
            })
            .collect()
    }
}

pub fn clock_label(offset: Duration) -> String {
    let seconds = offset.as_secs();
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, speaker: &str, text: &str, seconds: u64) -> TranscriptEntry {
        TranscriptEntry {
            id: id.into(),
            speaker: speaker.into(),
            text: text.into(),
            offset: Duration::from_secs(seconds),
        }
    }

    fn sample() -> Transcript {
        let mut transcript = Transcript::default();
        transcript.note(entry("a", "Ana Example", "Welcome to the standup", 3));
        transcript.note(entry("b", "Bo Sample", "The build is green", 9));
        transcript.note(entry("c", "Ana Example", "Great, then we ship", 14));
        transcript
    }

    #[test]
    fn entries_keep_the_order_they_first_arrived_in() {
        let transcript = sample();
        let ids: Vec<&str> = transcript.filtered("").iter().map(|entry| entry.id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "c"]);
        assert_eq!(transcript.filtered("").len(), 3);
    }

    #[test]
    fn a_growing_partial_updates_its_entry_and_keeps_its_time_and_place() {
        let mut transcript = sample();
        transcript.note(entry("b", "Bo Sample", "The build is green again", 20));
        assert_eq!(transcript.filtered("").len(), 3);
        let updated = &transcript.filtered("")[1];
        assert_eq!(updated.text, "The build is green again");
        assert_eq!(updated.offset, Duration::from_secs(9));
    }

    #[test]
    fn an_old_id_far_back_is_a_new_entry() {
        let mut transcript = sample();
        for index in 0..RECENT_WINDOW {
            transcript.note(entry(&format!("x{index}"), "Ana Example", "filler", 30));
        }
        transcript.note(entry("a", "Ana Example", "again", 99));
        assert_eq!(transcript.filtered("").len(), 3 + RECENT_WINDOW + 1);
    }

    #[test]
    fn the_filter_matches_text_and_speaker_ignoring_case() {
        let transcript = sample();
        assert_eq!(transcript.filtered("BUILD").len(), 1);
        assert_eq!(transcript.filtered("ana").len(), 2);
        assert_eq!(transcript.filtered("  ship ").len(), 1);
        assert!(transcript.filtered("nothing like this").is_empty());
        assert_eq!(transcript.filtered("   ").len(), 3);
    }

    #[test]
    fn the_clock_counts_minutes_and_seconds_since_the_call_start() {
        assert_eq!(clock_label(Duration::from_secs(5)), "00:05");
        assert_eq!(clock_label(Duration::from_secs(75)), "01:15");
        assert_eq!(clock_label(Duration::from_secs(4_503)), "75:03");
    }
}
