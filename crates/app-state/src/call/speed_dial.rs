use gpui_kit::Context;
use store::Store;

use crate::app_state::AppState;

const META_KEY: &str = "call_speed_dial";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeedDialEntry {
    pub user_id: String,
    pub name: String,
}

pub fn load(store: &Store) -> Vec<SpeedDialEntry> {
    store
        .meta(META_KEY)
        .ok()
        .flatten()
        .map(|text| parse(&text))
        .unwrap_or_default()
}

fn save(store: &Store, entries: &[SpeedDialEntry]) {
    let _ = store.set_meta(META_KEY, &serialize(entries));
}

fn serialize(entries: &[SpeedDialEntry]) -> String {
    entries
        .iter()
        .map(|entry| {
            format!(
                "{}\t{}",
                entry.user_id,
                entry.name.replace(['\t', '\n'], " ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse(text: &str) -> Vec<SpeedDialEntry> {
    text.lines()
        .filter_map(|line| line.split_once('\t'))
        .filter(|(user_id, _)| !user_id.is_empty())
        .map(|(user_id, name)| SpeedDialEntry {
            user_id: user_id.to_owned(),
            name: name.to_owned(),
        })
        .collect()
}

fn with_added(entries: &[SpeedDialEntry], added: SpeedDialEntry) -> Option<Vec<SpeedDialEntry>> {
    if entries.iter().any(|entry| entry.user_id == added.user_id) {
        return None;
    }
    let mut next = entries.to_vec();
    next.push(added);
    Some(next)
}

impl AppState {
    pub fn on_speed_dial(&self, user_id: &str) -> bool {
        self.speed_dial.iter().any(|entry| entry.user_id == user_id)
    }

    pub fn add_to_speed_dial(&mut self, user_id: &str, name: &str, cx: &mut Context<Self>) {
        let added = SpeedDialEntry {
            user_id: user_id.to_owned(),
            name: name.to_owned(),
        };
        let Some(next) = with_added(&self.speed_dial, added) else {
            return;
        };
        save(&self.store, &next);
        self.speed_dial = next;
        cx.notify();
    }

    pub fn remove_from_speed_dial(&mut self, user_id: &str, cx: &mut Context<Self>) {
        self.speed_dial.retain(|entry| entry.user_id != user_id);
        save(&self.store, &self.speed_dial);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(user_id: &str, name: &str) -> SpeedDialEntry {
        SpeedDialEntry {
            user_id: user_id.to_owned(),
            name: name.to_owned(),
        }
    }

    #[test]
    fn entries_survive_a_round_trip_in_order() {
        let entries = vec![entry("b", "Grace\tHopper"), entry("a", "Ada Lovelace")];
        assert_eq!(
            parse(&serialize(&entries)),
            [entry("b", "Grace Hopper"), entry("a", "Ada Lovelace")]
        );
        assert_eq!(parse("broken\n\tno id\n"), []);
    }

    #[test]
    fn adding_a_person_twice_keeps_one_entry() {
        let entries = vec![entry("a", "Ada")];
        assert_eq!(with_added(&entries, entry("a", "Ada")), None);
        assert_eq!(
            with_added(&entries, entry("b", "Grace")),
            Some(vec![entry("a", "Ada"), entry("b", "Grace")])
        );
    }
}
