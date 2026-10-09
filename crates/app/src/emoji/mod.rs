mod recent;
mod typing;

use std::ops::Range;
use std::sync::OnceLock;

pub use recent::Recent;
pub use typing::{active_query, closing_code, smiley_before_space, trailing_smiley, typed_char};

const INDEX: &str = include_str!("../../assets/emoji/emoji.tsv");

#[derive(Debug)]
pub struct Emoji {
    pub glyph: &'static str,
    pub codes: Vec<&'static str>,
    pub aliases: Vec<&'static str>,
}

impl Emoji {
    pub fn display_code(&self) -> &'static str {
        self.codes
            .iter()
            .find(|code| code.starts_with(char::is_alphabetic))
            .or(self.codes.first())
            .copied()
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub glyph: &'static str,
    pub code: &'static str,
    pub alias: Option<&'static str>,
    pub highlight: Range<usize>,
    pub recent: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Tier {
    Exact,
    CodePrefix,
    AliasPrefix,
    WordStart,
    Contains,
}

fn index() -> &'static [Emoji] {
    static PARSED: OnceLock<Vec<Emoji>> = OnceLock::new();
    PARSED.get_or_init(|| {
        INDEX
            .lines()
            .filter_map(|line| {
                let mut fields = line.split('\t');
                let glyph = fields.next()?;
                let codes = fields.next()?.split(' ').collect();
                let aliases = fields
                    .next()
                    .unwrap_or_default()
                    .split('|')
                    .filter(|alias| !alias.is_empty())
                    .collect();
                Some(Emoji {
                    glyph,
                    codes,
                    aliases,
                })
            })
            .collect()
    })
}

fn tier(candidate: &str, query: &str, separator: char) -> Option<(Tier, usize)> {
    if candidate == query {
        return Some((Tier::Exact, 0));
    }
    if candidate.starts_with(query) {
        return Some((Tier::CodePrefix, 0));
    }
    let start = candidate.find(query)?;
    let tier = if candidate[..start].ends_with(separator) {
        Tier::WordStart
    } else {
        Tier::Contains
    };
    Some((tier, start))
}

/// Tier, then the alias position: a label beats a loose tag. Only the label (position 0) can be exact.
fn best_match(emoji: &'static Emoji, query: &str) -> Option<(Tier, usize, Match)> {
    let code_hit = emoji
        .codes
        .iter()
        .filter_map(|code| tier(code, query, '_').map(|(tier, start)| (tier, start, *code)))
        .min_by_key(|(tier, ..)| *tier);
    let alias_query = query.replace('_', " ");
    let alias_hit = emoji
        .aliases
        .iter()
        .enumerate()
        .filter_map(|(position, alias)| {
            tier(alias, &alias_query, ' ').map(|(tier, start)| {
                let tier = match tier {
                    Tier::Exact if position == 0 => Tier::Exact,
                    Tier::Exact | Tier::CodePrefix => Tier::AliasPrefix,
                    other => other,
                };
                (tier, position, start, *alias)
            })
        })
        .min_by_key(|(tier, position, ..)| (*tier, *position));
    let code_wins = match (&code_hit, &alias_hit) {
        (Some(code), Some(alias)) => code.0 <= alias.0,
        (code, _) => code.is_some(),
    };
    if code_wins {
        let (tier, start, code) = code_hit?;
        return Some((
            tier,
            0,
            Match {
                glyph: emoji.glyph,
                code,
                alias: None,
                highlight: start..start + query.len(),
                recent: false,
            },
        ));
    }
    let (tier, position, start, alias) = alias_hit?;
    Some((
        tier,
        position,
        Match {
            glyph: emoji.glyph,
            code: emoji.display_code(),
            alias: Some(alias),
            highlight: start..start + alias_query.len(),
            recent: false,
        },
    ))
}

pub fn glyphs() -> impl Iterator<Item = &'static str> {
    index().iter().map(|emoji| emoji.glyph)
}

/// Ranked by: recent, exact, English prefix, German prefix, word start, contains, emoji order.
pub fn search(query: &str, recent: &[String], limit: usize) -> Vec<Match> {
    let query = query.to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<(usize, Tier, usize, usize, Match)> = index()
        .iter()
        .enumerate()
        .filter_map(|(order, emoji)| {
            let (tier, position, mut found) = best_match(emoji, &query)?;
            let recent_rank = recent
                .iter()
                .position(|glyph| glyph == emoji.glyph)
                .filter(|_| tier < Tier::Contains);
            found.recent = recent_rank.is_some();
            Some((
                recent_rank.unwrap_or(usize::MAX),
                tier,
                position,
                order,
                found,
            ))
        })
        .collect();
    hits.sort_by_key(|(recent_rank, tier, position, order, _)| {
        (*recent_rank, *tier, *position, *order)
    });
    hits.into_iter()
        .take(limit)
        .map(|(.., found)| found)
        .collect()
}

/// A full English code, or a German alias written with underscores.
pub fn lookup(code: &str) -> Option<&'static str> {
    let code = code.to_lowercase();
    if let Some(emoji) = index()
        .iter()
        .find(|emoji| emoji.codes.contains(&code.as_str()))
    {
        return Some(emoji.glyph);
    }
    let alias = code.replace('_', " ");
    index()
        .iter()
        .filter_map(|emoji| {
            let position = emoji.aliases.iter().position(|known| *known == alias)?;
            Some((position, emoji.glyph))
        })
        .min_by_key(|(position, _)| *position)
        .map(|(_, glyph)| glyph)
}

#[cfg(test)]
mod tests {
    use super::{Recent, lookup, search};

    fn glyphs(query: &str, recent: &[String]) -> Vec<&'static str> {
        search(query, recent, 7)
            .into_iter()
            .map(|found| found.glyph)
            .collect()
    }

    #[test]
    fn english_prefix_finds_thumbs() {
        let found = glyphs("th", &[]);
        assert!(found.contains(&"👍"));
        assert!(found.contains(&"👎"));
    }

    #[test]
    fn a_tag_never_beats_an_english_prefix() {
        let found = glyphs("th", &[]);
        assert!(!found.contains(&"🇹🇭"));
    }

    #[test]
    fn recent_emoji_rank_first() {
        let recent = vec!["🤔".to_owned()];
        assert_eq!(glyphs("th", &recent)[0], "🤔");
        assert!(search("th", &recent, 7)[0].recent);
    }

    #[test]
    fn german_alias_finds_english_code() {
        let found = search("dau", &[], 7);
        let thumbs = found.iter().find(|found| found.glyph == "👍").unwrap();
        assert_eq!(thumbs.code, "thumbsup");
        assert_eq!(thumbs.alias, Some("daumen hoch"));
        assert_eq!(thumbs.highlight, 0..3);
    }

    #[test]
    fn exact_alias_beats_alias_prefix() {
        let found = search("herz", &[], 40);
        let exact = found.iter().position(|found| found.glyph == "💔").unwrap();
        let prefix = found.iter().position(|found| found.glyph == "😍").unwrap();
        assert!(exact < prefix);
    }

    #[test]
    fn quick_reactions_lead_with_a_case_insensitive_query() {
        let found = search("Herz", &Recent::default().glyphs(), 7);
        assert_eq!(found[0].glyph, "❤️");
        assert_eq!(found[0].alias, Some("herz"));
    }

    #[test]
    fn lookup_takes_english_and_german_codes() {
        assert_eq!(lookup("thumbsup"), Some("👍"));
        assert_eq!(lookup("+1"), Some("👍"));
        assert_eq!(lookup("daumen_hoch"), Some("👍"));
        assert_eq!(lookup("ganzneu"), None);
    }
}
