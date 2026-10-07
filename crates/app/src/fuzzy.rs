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
    score_plain(&query, &lowered)
}

fn score_plain(query: &[char], candidate: &[char]) -> Option<i64> {
    let mut score = 0i64;
    let mut position = 0usize;
    let mut previous_match: Option<usize> = None;
    for &wanted in query {
        let found = candidate[position..].iter().position(|&c| c == wanted)? + position;
        score += 10;
        if previous_match.is_some_and(|previous| previous + 1 == found) {
            score += 15;
        }
        if found == 0 || !candidate[found - 1].is_alphanumeric() {
            score += 20;
        }
        score -= (found - previous_match.map_or(0, |previous| previous + 1)) as i64;
        previous_match = Some(found);
        position = found + 1;
    }
    Some(score)
}

pub fn rank<T: Clone>(query: &str, items: &[(String, T)], limit: usize) -> Vec<T> {
    let mut scored: Vec<(i64, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(index, (label, _))| score(query, label).map(|score| (score, index)))
        .collect();
    scored.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
    scored
        .into_iter()
        .take(limit)
        .map(|(_, index)| items[index].1.clone())
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
    fn case_is_ignored() {
        assert!(score("ALPHA", "alpha").is_some());
    }
}
