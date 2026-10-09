use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use gpui_kit::*;
use teams_core::PersonProfile;

use crate::app_state::{AppEvent, AppState, Selection};
use crate::demo_profiles;
use crate::notice::short_error;
use crate::profile_model::is_stale;
use crate::views::new_chat::existing_one_on_one;

#[derive(Default)]
pub struct ProfileEntry {
    pub profile: Option<Arc<PersonProfile>>,
    pub fetching: bool,
    pub failed: bool,
    fetched_at: Option<Instant>,
}

impl ProfileEntry {
    fn is_fresh(&self, now: Instant) -> bool {
        self.profile.is_some() && self.fetched_at.is_some_and(|at| !is_stale(at, now))
    }
}

pub struct ProfileRequest {
    pub user_id: String,
    pub anchor: Point<Pixels>,
}

pub type ProfileCache = HashMap<String, ProfileEntry>;

impl AppState {
    pub fn open_profile_card(
        &mut self,
        user_id: &str,
        anchor: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.profile_request = Some(ProfileRequest {
            user_id: user_id.to_owned(),
            anchor,
        });
        self.request_profile(user_id, false, cx);
        self.request_avatars(vec![user_id.to_owned()], cx);
        cx.emit(AppEvent::Profile);
        cx.notify();
    }

    pub fn take_profile_request(&mut self) -> Option<ProfileRequest> {
        self.profile_request.take()
    }

    pub fn request_profile(&mut self, user_id: &str, force: bool, cx: &mut Context<Self>) {
        let now = Instant::now();
        let entry = self.profiles.entry(user_id.to_owned()).or_default();
        if entry.fetching || (!force && entry.is_fresh(now)) {
            return;
        }
        let owned_id = user_id.to_owned();
        let receiver = if self.mode.demo {
            crate::runtime::spawn(demo_profiles::load(owned_id.clone()))
        } else if let Some(engine) = self.engine.clone() {
            let id = owned_id.clone();
            crate::runtime::spawn(async move {
                engine
                    .person_profile(&id)
                    .await
                    .map_err(|error| short_error(&error))
            })
        } else {
            entry.failed = true;
            cx.notify();
            return;
        };
        entry.fetching = true;
        entry.failed = false;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = receiver
                .await
                .unwrap_or_else(|_| Err("cancelled".to_owned()));
            this.update(cx, |state, cx| state.finish_profile(&owned_id, result, cx))
                .ok();
        })
        .detach();
    }

    fn finish_profile(
        &mut self,
        user_id: &str,
        result: Result<PersonProfile, String>,
        cx: &mut Context<Self>,
    ) {
        let entry = self.profiles.entry(user_id.to_owned()).or_default();
        entry.fetching = false;
        match result {
            Ok(profile) => {
                let related: Vec<String> = profile
                    .manager
                    .iter()
                    .chain(&profile.direct_reports)
                    .map(|person| person.id.clone())
                    .collect();
                entry.profile = Some(Arc::new(profile));
                entry.failed = false;
                entry.fetched_at = Some(Instant::now());
                self.request_avatars(related, cx);
            }
            Err(_) => entry.failed = true,
        }
        cx.notify();
    }

    pub fn copy_profile_email(&mut self, email: &str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(email.to_owned()));
        self.raise_notice("Email copied".to_owned(), None, cx);
    }

    pub fn open_chat_with(&mut self, user_id: &str, cx: &mut Context<Self>) {
        let existing =
            existing_one_on_one(&self.sidebar.chats, self.directory.me.as_ref(), user_id)
                .map(|chat| chat.id.clone());
        if let Some(chat_id) = existing {
            self.select(Selection::Chat(chat_id), cx);
            return;
        }
        if self.mode.read_only {
            self.raise_notice("Read-only mode: chat not created".to_owned(), None, cx);
            return;
        }
        let Some(engine) = self.engine.clone() else {
            self.raise_notice("No chat with this person yet".to_owned(), None, cx);
            return;
        };
        let user_id = user_id.to_owned();
        let receiver = crate::runtime::spawn(async move {
            engine
                .create_one_on_one(&user_id)
                .await
                .map_err(|error| short_error(&error))
        });
        cx.spawn(async move |this, cx| {
            let outcome = receiver
                .await
                .unwrap_or_else(|_| Err("cancelled".to_owned()));
            this.update(cx, |state, cx| match outcome {
                Ok(chat_id) => {
                    state.reload_sidebar(cx);
                    state.select(Selection::Chat(chat_id), cx);
                }
                Err(reason) => {
                    state.raise_notice(format!("Chat could not be opened: {reason}"), None, cx)
                }
            })
            .ok();
        })
        .detach();
    }
}
