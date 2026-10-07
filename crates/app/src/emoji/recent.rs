const LIMIT: usize = 24;
const META_KEY: &str = "emoji_recent";
const TEAMS_QUICK_REACTIONS: [&str; 6] = ["👍", "❤️", "😆", "😮", "😢", "😡"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recent(Vec<String>);

impl Default for Recent {
    fn default() -> Self {
        Recent(TEAMS_QUICK_REACTIONS.map(str::to_owned).to_vec())
    }
}

impl Recent {
    pub fn load(store: &store::Store) -> Self {
        match store.meta(META_KEY) {
            Ok(Some(stored)) => Self::parse(&stored),
            _ => Self::default(),
        }
    }

    pub fn save(&self, store: &store::Store) {
        store.set_meta(META_KEY, &self.0.join("\n")).ok();
    }

    fn parse(stored: &str) -> Self {
        Recent(
            stored
                .lines()
                .filter(|glyph| !glyph.is_empty())
                .take(LIMIT)
                .map(str::to_owned)
                .collect(),
        )
    }

    pub fn glyphs(&self) -> &[String] {
        &self.0
    }

    pub fn push(&mut self, glyph: &str) {
        self.0.retain(|known| known != glyph);
        self.0.insert(0, glyph.to_owned());
        self.0.truncate(LIMIT);
    }
}

#[cfg(test)]
mod tests {
    use super::{LIMIT, Recent};

    #[test]
    fn starts_with_the_teams_quick_reactions() {
        assert_eq!(Recent::default().glyphs()[..2], ["👍", "❤️"]);
    }

    #[test]
    fn newest_first_without_duplicates() {
        let mut recent = Recent::parse("");
        recent.push("👍");
        recent.push("❤️");
        recent.push("👍");
        assert_eq!(recent.glyphs(), ["👍", "❤️"]);
    }

    #[test]
    fn keeps_at_most_the_limit() {
        let mut recent = Recent::parse("");
        for index in 0..LIMIT + 5 {
            recent.push(&index.to_string());
        }
        assert_eq!(recent.glyphs().len(), LIMIT);
        assert_eq!(Recent::parse(&recent.glyphs().join("\n")), recent);
    }
}
