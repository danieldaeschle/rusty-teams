use std::collections::HashSet;

use store::Store;

const META_KEY: &str = "notifications.followed_channels";

pub fn load(store: &Store) -> HashSet<String> {
    store
        .meta(META_KEY)
        .ok()
        .flatten()
        .map(|text| parse(&text))
        .unwrap_or_default()
}

pub fn save(store: &Store, channel_ids: &HashSet<String>) {
    let _ = store.set_meta(META_KEY, &serialize(channel_ids));
}

fn serialize(channel_ids: &HashSet<String>) -> String {
    let mut ids: Vec<&str> = channel_ids.iter().map(String::as_str).collect();
    ids.sort_unstable();
    ids.join("\n")
}

fn parse(text: &str) -> HashSet<String> {
    text.lines()
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_in_store() {
        let store = Store::open_in_memory().unwrap();
        assert!(load(&store).is_empty());
        let followed: HashSet<String> = ["19:a@thread.tacv2", "19:b@thread.tacv2"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        save(&store, &followed);
        assert_eq!(load(&store), followed);
        save(&store, &HashSet::new());
        assert!(load(&store).is_empty());
    }
}
