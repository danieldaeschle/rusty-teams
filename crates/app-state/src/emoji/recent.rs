use crate::rows::reaction_glyph;

const LIMIT: usize = 24;
const META_KEY: &str = "emoji_recent";
const TEAMS_QUICK_REACTIONS: [&str; 6] = ["👍", "❤️", "😆", "😮", "😢", "😡"];
const COUNT_SEPARATOR: char = '\t';

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    glyph: String,
    uses: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recent(Vec<Entry>);

impl Default for Recent {
    fn default() -> Self {
        Recent(
            TEAMS_QUICK_REACTIONS
                .map(|glyph| Entry {
                    glyph: glyph.to_owned(),
                    uses: 0,
                })
                .to_vec(),
        )
    }
}

impl Recent {
    pub fn load(store: &store::Store) -> Self {
        match store.meta(META_KEY) {
            Ok(Some(stored)) => Self::parse(&stored),
            _ => Self::default(),
        }
    }

    pub fn reload(&mut self, store: &store::Store) {
        *self = Self::load(store);
    }

    pub fn save(&self, store: &store::Store) {
        store.set_meta(META_KEY, &self.serialize()).ok();
    }

    fn serialize(&self) -> String {
        self.0
            .iter()
            .map(|entry| format!("{}{COUNT_SEPARATOR}{}", entry.glyph, entry.uses))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn parse(stored: &str) -> Self {
        Recent(
            stored
                .lines()
                .filter(|line| !line.is_empty())
                .take(LIMIT)
                .map(|line| match line.split_once(COUNT_SEPARATOR) {
                    Some((glyph, uses)) => Entry {
                        glyph: glyph.to_owned(),
                        uses: uses.parse().unwrap_or(0),
                    },
                    None => Entry {
                        glyph: line.to_owned(),
                        uses: 0,
                    },
                })
                .collect(),
        )
    }

    pub fn glyphs(&self) -> Vec<String> {
        self.0.iter().map(|entry| entry.glyph.clone()).collect()
    }

    pub fn most_used(&self, count: usize) -> Vec<String> {
        let mut ranked: Vec<&Entry> = self.0.iter().collect();
        ranked.sort_by_key(|entry| std::cmp::Reverse(entry.uses));
        ranked
            .into_iter()
            .take(count)
            .map(|entry| entry.glyph.clone())
            .collect()
    }

    pub fn push(&mut self, glyph: &str) {
        self.bump(glyph, 0);
    }

    pub fn record_use(&mut self, glyph: &str) {
        self.bump(glyph, 1);
    }

    fn bump(&mut self, glyph: &str, uses: u32) {
        let previous_uses = self
            .0
            .iter()
            .position(|entry| reaction_glyph(&entry.glyph) == reaction_glyph(glyph))
            .map_or(0, |position| self.0.remove(position).uses);
        self.0.insert(
            0,
            Entry {
                glyph: glyph.to_owned(),
                uses: previous_uses + uses,
            },
        );
        self.0.truncate(LIMIT);
    }
}

#[cfg(test)]
mod tests {
    use super::{LIMIT, Recent};

    #[test]
    fn starts_with_the_teams_quick_reactions() {
        assert_eq!(Recent::default().glyphs()[..2], ["👍", "❤️"]);
        assert_eq!(Recent::default().most_used(4), ["👍", "❤️", "😆", "😮"]);
    }

    #[test]
    fn newest_first_without_duplicates() {
        let mut recent = Recent::parse("");
        recent.record_use("👍");
        recent.record_use("❤️");
        recent.record_use("👍");
        assert_eq!(recent.glyphs(), ["👍", "❤️"]);
    }

    #[test]
    fn keeps_at_most_the_limit() {
        let mut recent = Recent::parse("");
        for index in 0..LIMIT + 5 {
            recent.record_use(&index.to_string());
        }
        assert_eq!(recent.glyphs().len(), LIMIT);
        assert_eq!(Recent::parse(&recent.serialize()), recent);
    }

    #[test]
    fn ranks_by_count_then_recency() {
        let mut recent = Recent::parse("");
        for glyph in ["🎉", "🎉", "🔥", "👀", "👀", "✅", "🚀"] {
            recent.record_use(glyph);
        }
        assert_eq!(recent.most_used(4), ["👀", "🎉", "🚀", "✅"]);
    }

    #[test]
    fn matches_glyphs_with_and_without_variation_selector() {
        let mut recent = Recent::parse("");
        recent.record_use("\u{2764}\u{FE0F}");
        recent.record_use("\u{2764}");
        assert_eq!(recent.glyphs().len(), 1);
        assert_eq!(recent.most_used(1), ["\u{2764}"]);
    }

    #[test]
    fn push_moves_to_front_without_counting() {
        let mut recent = Recent::default();
        recent.push("😮");
        assert_eq!(recent.glyphs()[0], "😮");
        assert_eq!(recent.most_used(1), ["😮"]);
        recent.record_use("👍");
        assert_eq!(recent.most_used(1), ["👍"]);
    }

    #[test]
    fn legacy_values_without_counts_still_load() {
        let recent = Recent::parse("🎉\n👍\n");
        assert_eq!(recent.glyphs(), ["🎉", "👍"]);
        assert_eq!(recent.most_used(2), ["🎉", "👍"]);
    }

    #[test]
    fn counts_survive_a_round_trip() {
        let mut recent = Recent::parse("");
        recent.record_use("👍");
        recent.record_use("👍");
        recent.record_use("🔥");
        assert_eq!(Recent::parse(&recent.serialize()), recent);
        assert_eq!(recent.most_used(1), ["👍"]);
    }
}
