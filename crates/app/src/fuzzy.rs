pub fn score(query: &str, candidate: &str) -> Option<i64> {
    let query: Vec<char> = query
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if query.is_empty() {
        return Some(0);
    }
    let lowered: Vec<char> = candidate.to_lowercase().chars().collect();
    let plain = score_plain(&query, &lowered, false);
    let word_starts = score_plain(&query, &lowered, true);
    plain.max(word_starts)
}

fn is_word_start(candidate: &[char], index: usize) -> bool {
    index == 0 || !candidate[index - 1].is_alphanumeric()
}

fn score_plain(query: &[char], candidate: &[char], prefer_word_starts: bool) -> Option<i64> {
    let mut score = 0i64;
    let mut position = 0usize;
    let mut previous_match: Option<usize> = None;
    for &wanted in query {
        let mut occurrences =
            (position..candidate.len()).filter(|&index| candidate[index] == wanted);
        let found = if prefer_word_starts {
            (position..candidate.len())
                .find(|&index| candidate[index] == wanted && is_word_start(candidate, index))
                .or_else(|| occurrences.next())?
        } else {
            occurrences.next()?
        };
        score += 10;
        if previous_match.is_some_and(|previous| previous + 1 == found) {
            score += 15;
        }
        if is_word_start(candidate, found) {
            score += 20;
        } else {
            score -= (found - previous_match.map_or(0, |previous| previous + 1)) as i64;
        }
        previous_match = Some(found);
        position = found + 1;
    }
    Some(score)
}

pub fn rank<T: Clone>(query: &str, items: &[(String, T)], limit: usize) -> Vec<T> {
    let mut scored: Vec<(i64, usize, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(index, (label, _))| {
            score(query, label).map(|score| (score, label.chars().count(), index))
        })
        .collect();
    scored.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then(left.1.cmp(&right.1))
            .then(left.2.cmp(&right.2))
    });
    scored
        .into_iter()
        .take(limit)
        .map(|(_, _, index)| items[index].1.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsequence_matches_and_misses() {
        assert!(score("pgr", "Project Green").is_some());
        assert!(score("xyz", "Project Green").is_none());
        assert_eq!(score("", "anything"), Some(0));
    }

    #[test]
    fn contiguous_word_start_beats_scattered() {
        let tight = score("proj", "Project Green").unwrap();
        let scattered = score("proj", "Pxxrxxoxxj").unwrap();
        assert!(tight > scattered);
    }

    #[test]
    fn rank_orders_by_score_and_limits() {
        let items = vec![
            ("Alpha Team".to_owned(), 1),
            ("Beta".to_owned(), 2),
            ("Alphabet".to_owned(), 3),
        ];
        let ranked = rank("alph", &items, 5);
        assert_eq!(ranked.len(), 2);
        assert_eq!(rank("a", &items, 1).len(), 1);
    }

    #[test]
    fn initials_beat_a_contiguous_match_inside_a_word() {
        let items = vec![
            ("QA cw review".to_owned(), "meeting"),
            ("Ada Lovelace, Christoph Wiechert".to_owned(), "group"),
            ("Christoph Wiechert".to_owned(), "person"),
        ];
        assert_eq!(rank("cw", &items, 3), ["person", "group", "meeting"]);
    }

    #[test]
    fn case_is_ignored() {
        assert!(score("ALPHA", "alpha").is_some());
    }
}
