use std::collections::HashMap;

use crate::app_state::AppState;

pub fn local_names(state: &AppState) -> HashMap<String, String> {
    let mut names: HashMap<String, String> = HashMap::new();
    let known_people = state
        .directory
        .me
        .iter()
        .map(|me| (me.user_id.clone(), me.display_name.clone()))
        .chain(
            state
                .sidebar
                .chats
                .iter()
                .flat_map(|chat| chat.members.iter())
                .filter_map(|member| Some((member.user_id.clone()?, member.display_name.clone()))),
        );
    for (user_id, name) in known_people {
        names.entry(user_id).or_insert(name);
    }
    names
}

pub fn resolve_names(state: &AppState, user_ids: &[String]) -> HashMap<String, String> {
    let mut names = local_names(state);
    names.retain(|user_id, _| user_ids.contains(user_id));
    let missing: Vec<String> = user_ids
        .iter()
        .filter(|user_id| !names.contains_key(*user_id))
        .cloned()
        .collect();
    if let Ok(stored) = state.store.display_names(&missing) {
        names.extend(stored);
    }
    names
}
