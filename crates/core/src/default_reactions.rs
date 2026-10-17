use store::Store;

use crate::error::Result;

const META_ACCOUNT_DEFAULT_REACTIONS: &str = "default_reactions_account";
pub const TEAMS_DEFAULT_REACTIONS: [&str; 4] = ["like", "heart", "laugh", "surprised"];
pub const DEFAULT_REACTION_SLOTS: usize = 5;

pub fn store_account_default_reactions(
    store: &Store,
    reaction_ids: Option<&[String]>,
) -> Result<()> {
    let value = reaction_ids
        .map(|reaction_ids| serde_json::Value::from(reaction_ids.to_vec()).to_string())
        .unwrap_or_default();
    Ok(store.set_meta(META_ACCOUNT_DEFAULT_REACTIONS, &value)?)
}

/// The account setting `defaultMessageReactions`, Teams' own default set when it was never saved.
pub fn default_reactions(store: &Store) -> Vec<String> {
    store
        .meta(META_ACCOUNT_DEFAULT_REACTIONS)
        .ok()
        .flatten()
        .filter(|stored| !stored.is_empty())
        .and_then(|stored| serde_json::from_str::<Vec<String>>(&stored).ok())
        .unwrap_or_else(|| {
            TEAMS_DEFAULT_REACTIONS
                .iter()
                .map(|reaction_id| (*reaction_id).to_owned())
                .collect()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_to_the_teams_set_until_the_account_has_one() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(default_reactions(&store), TEAMS_DEFAULT_REACTIONS);
        let saved = vec!["fire".to_owned(), "like".to_owned()];
        store_account_default_reactions(&store, Some(&saved)).unwrap();
        assert_eq!(default_reactions(&store), saved);
        store_account_default_reactions(&store, Some(&[])).unwrap();
        assert!(default_reactions(&store).is_empty());
        store_account_default_reactions(&store, None).unwrap();
        assert_eq!(default_reactions(&store), TEAMS_DEFAULT_REACTIONS);
    }
}
