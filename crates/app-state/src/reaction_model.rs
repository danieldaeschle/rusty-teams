use std::fmt::Display;

use chrono::{DateTime, Datelike, TimeZone, Utc};

use crate::rows::ReactionChip;

const FACE_LIMIT: usize = 3;
const TOOLTIP_NAME_LIMIT: usize = 3;
const TOOLTIP_LEAD_NAMES: usize = 2;
pub const UNKNOWN_REACTOR: &str = "Unknown";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reactor {
    pub user_id: Option<String>,
    pub name: String,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReactionTab {
    pub glyph: Option<String>,
    pub label: String,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReactionEntry {
    pub reactor: Reactor,
    pub label: String,
}

pub fn shows_faces(count: usize) -> bool {
    (1..=FACE_LIMIT).contains(&count)
}

pub fn tooltip_text(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [only] => (*only).to_owned(),
        names if names.len() <= TOOLTIP_NAME_LIMIT => {
            let (last, lead) = names.split_last().expect("not empty");
            format!("{} and {last}", lead.join(", "))
        }
        names => format!(
            "{} and {} others",
            names[..TOOLTIP_LEAD_NAMES].join(", "),
            names.len() - TOOLTIP_LEAD_NAMES
        ),
    }
}

pub fn chip_tooltip(chip: &ReactionChip) -> String {
    let names: Vec<&str> = chip
        .reactors
        .iter()
        .map(|reactor| reactor.name.as_str())
        .collect();
    tooltip_text(&names)
}

pub fn reaction_time_label<Zone>(time: DateTime<Utc>, now: &DateTime<Zone>) -> String
where
    Zone: TimeZone,
    Zone::Offset: Display,
{
    let local = time.with_timezone(&now.timezone());
    let day = local.date_naive();
    let today = now.date_naive();
    let clock = local.format("%H:%M");
    if day == today {
        format!("today at {clock}")
    } else if today.pred_opt() == Some(day) {
        format!("yesterday at {clock}")
    } else if day.year() == today.year() {
        format!("{} at {clock}", local.format("%b %-d"))
    } else {
        format!("{} at {clock}", local.format("%b %-d, %Y"))
    }
}

pub fn reaction_tabs(chips: &[ReactionChip]) -> Vec<ReactionTab> {
    if chips.len() < 2 {
        return Vec::new();
    }
    let total = chips.iter().map(|chip| chip.reactors.len()).sum();
    std::iter::once(ReactionTab {
        glyph: None,
        label: "All".to_owned(),
        count: total,
    })
    .chain(chips.iter().map(|chip| ReactionTab {
        glyph: Some(chip.glyph()),
        label: chip.label.clone(),
        count: chip.reactors.len(),
    }))
    .collect()
}

pub fn reaction_entries(chips: &[ReactionChip], tab: Option<&str>) -> Vec<ReactionEntry> {
    let mut entries: Vec<ReactionEntry> = chips
        .iter()
        .filter(|chip| tab.is_none_or(|glyph| chip.glyph() == glyph))
        .flat_map(|chip| {
            chip.reactors.iter().map(|reactor| ReactionEntry {
                reactor: reactor.clone(),
                label: chip.label.clone(),
            })
        })
        .collect();
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.reactor.created_at));
    entries
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, FixedOffset};

    use super::*;

    fn reactor(name: &str, created_at: Option<DateTime<Utc>>) -> Reactor {
        Reactor {
            user_id: None,
            name: name.to_owned(),
            created_at,
        }
    }

    fn chip(reaction_type: &str, reactors: Vec<Reactor>) -> ReactionChip {
        ReactionChip {
            reaction_type: reaction_type.to_owned(),
            label: crate::rows::reaction_label(reaction_type),
            count: reactors.len(),
            mine: false,
            reactors,
        }
    }

    fn utc(day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, day, hour, minute, 0)
            .unwrap()
    }

    #[test]
    fn faces_show_up_to_three_people_then_a_count() {
        assert!(!shows_faces(0));
        assert!(shows_faces(1));
        assert!(shows_faces(3));
        assert!(!shows_faces(4));
    }

    #[test]
    fn tooltip_lists_names_then_others() {
        assert_eq!(tooltip_text(&["Priya Nair"]), "Priya Nair");
        assert_eq!(tooltip_text(&["Priya Nair", "Lea"]), "Priya Nair and Lea");
        assert_eq!(tooltip_text(&["A", "B", "C"]), "A, B and C");
        assert_eq!(
            tooltip_text(&["Jonas Ortega", "Tobias Klein", "C", "D", "E"]),
            "Jonas Ortega, Tobias Klein and 3 others"
        );
    }

    #[test]
    fn time_label_reads_today_yesterday_or_date() {
        let now = FixedOffset::east_opt(0)
            .unwrap()
            .with_ymd_and_hms(2026, 10, 8, 12, 0, 0)
            .unwrap();
        assert_eq!(reaction_time_label(utc(8, 1, 5), &now), "today at 01:05");
        assert_eq!(
            reaction_time_label(utc(7, 17, 31), &now),
            "yesterday at 17:31"
        );
        assert_eq!(reaction_time_label(utc(4, 17, 31), &now), "Oct 4 at 17:31");
    }

    #[test]
    fn tabs_appear_only_for_several_kinds() {
        let one = vec![chip("like", vec![reactor("A", None)])];
        assert!(reaction_tabs(&one).is_empty());
        let two = vec![
            chip("like", vec![reactor("A", None), reactor("B", None)]),
            chip("heart", vec![reactor("C", None)]),
        ];
        let tabs = reaction_tabs(&two);
        assert_eq!(tabs.len(), 3);
        assert_eq!((tabs[0].glyph.clone(), tabs[0].count), (None, 3));
        assert_eq!(tabs[1].count, 2);
        assert_eq!(tabs[2].count, 1);
    }

    #[test]
    fn entries_are_newest_first_and_filtered_by_tab() {
        let chips = vec![
            chip(
                "like",
                vec![
                    reactor("old", Some(utc(1, 9, 0))),
                    reactor("none", None),
                    reactor("new", Some(utc(1, 9, 0) + Duration::hours(2))),
                ],
            ),
            chip("heart", vec![reactor("mid", Some(utc(1, 10, 0)))]),
        ];
        let names = |entries: Vec<ReactionEntry>| -> Vec<String> {
            entries
                .into_iter()
                .map(|entry| entry.reactor.name)
                .collect()
        };
        assert_eq!(
            names(reaction_entries(&chips, None)),
            ["new", "mid", "old", "none"]
        );
        let like = chips[0].glyph();
        assert_eq!(
            names(reaction_entries(&chips, Some(&like))),
            ["new", "old", "none"]
        );
    }
}
