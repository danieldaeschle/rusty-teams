use std::collections::{HashMap, HashSet};

use gpui_kit::*;
use teams_core::PostFollowState;

use crate::app_state::AppState;
use crate::runtime;

pub const TURN_OFF_NOTIFICATIONS: &str = "Turn off notifications";
pub const TURN_ON_NOTIFICATIONS: &str = "Turn on notifications";

type PostKey = (String, String);

#[derive(Default)]
pub struct PostFollows {
    states: HashMap<PostKey, PostFollowState>,
    requested: HashSet<PostKey>,
}

fn post_key(channel_id: &str, root_id: &str) -> PostKey {
    (channel_id.to_owned(), root_id.to_owned())
}

/// Teams reads the explicit reply chain state first, then implicit participation.
pub fn post_followed(state: Option<PostFollowState>, participated: bool) -> bool {
    match state {
        Some(state) => state.followed(),
        None => participated,
    }
}

pub fn follow_label(followed: bool) -> &'static str {
    if followed {
        TURN_OFF_NOTIFICATIONS
    } else {
        TURN_ON_NOTIFICATIONS
    }
}

pub fn follow_notice(followed: bool) -> &'static str {
    if followed {
        "Notifications turned on"
    } else {
        "Notifications turned off"
    }
}

pub fn follow_failure(followed: bool) -> &'static str {
    if followed {
        "Couldn't turn on notifications"
    } else {
        "Couldn't turn off notifications"
    }
}

impl AppState {
    pub fn post_follow_state(&self, channel_id: &str, root_id: &str) -> Option<PostFollowState> {
        self.post_follows
            .states
            .get(&post_key(channel_id, root_id))
            .copied()
    }

    pub fn request_post_follow_state(
        &mut self,
        channel_id: &str,
        root_id: &str,
        cx: &mut Context<Self>,
    ) {
        let key = post_key(channel_id, root_id);
        if !self.post_follows.requested.insert(key) {
            return;
        }
        let Some(engine) = self.engine.clone() else {
            return;
        };
        let (channel, root) = (channel_id.to_owned(), root_id.to_owned());
        let requested = vec![root.clone()];
        let requested_channel = channel.clone();
        let receiver = runtime::spawn(async move {
            engine
                .post_follow_states(&requested_channel, &requested)
                .await
        });
        cx.spawn(async move |this, cx| {
            let outcome = receiver.await;
            this.update(cx, |state, cx| match outcome {
                Ok(Ok(states)) => {
                    if let Some(follow_state) = states.get(&root) {
                        state
                            .post_follows
                            .states
                            .insert(post_key(&channel, &root), *follow_state);
                        cx.notify();
                    }
                }
                _ => {
                    state
                        .post_follows
                        .requested
                        .remove(&post_key(&channel, &root));
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn set_post_followed(
        &mut self,
        channel_id: &str,
        root_id: &str,
        followed: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(engine_or_demo) = self.chat_action_engine(cx) else {
            return;
        };
        let key = post_key(channel_id, root_id);
        let previous = self.post_follows.states.get(&key).copied();
        let next = if followed {
            PostFollowState::ExplicitFollow
        } else {
            PostFollowState::ExplicitUnfollow
        };
        self.post_follows.states.insert(key.clone(), next);
        cx.notify();
        let Some(engine) = engine_or_demo else {
            self.raise_notice(follow_notice(followed).to_owned(), None, cx);
            return;
        };
        let (channel, root) = (channel_id.to_owned(), root_id.to_owned());
        self.run_channel_action(
            follow_failure(followed),
            async move {
                let annotation_failure =
                    engine.set_post_followed(&channel, &root, followed).await?;
                if let Some(error) = annotation_failure {
                    log::warn!("post follow annotation not written: {error}");
                }
                Ok(())
            },
            move |state, cx| {
                match previous {
                    Some(previous) => state.post_follows.states.insert(key, previous),
                    None => state.post_follows.states.remove(&key),
                };
                cx.notify();
            },
            Some(follow_notice(followed)),
            cx,
        );
    }
}

#[cfg(test)]
mod tests {
    use teams_core::PostFollowState;

    use super::{follow_failure, follow_label, follow_notice, post_followed};

    #[test]
    fn explicit_state_wins_over_participation() {
        assert!(!post_followed(
            Some(PostFollowState::ExplicitUnfollow),
            true
        ));
        assert!(post_followed(Some(PostFollowState::ExplicitFollow), false));
        assert!(post_followed(Some(PostFollowState::ImplicitFollow), false));
        assert!(post_followed(None, true));
        assert!(!post_followed(None, false));
    }

    #[test]
    fn labels_and_notices_use_teams_strings() {
        assert_eq!(follow_label(true), "Turn off notifications");
        assert_eq!(follow_label(false), "Turn on notifications");
        assert_eq!(follow_notice(false), "Notifications turned off");
        assert_eq!(follow_notice(true), "Notifications turned on");
        assert_eq!(follow_failure(false), "Couldn't turn off notifications");
        assert_eq!(follow_failure(true), "Couldn't turn on notifications");
    }
}
