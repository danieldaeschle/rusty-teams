use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use chatsvc::{ChatListing, UserProfile, UserProfiles};
use chrono::{DateTime, Utc};
use futures_util::{StreamExt, stream};
use graph::{Chat, Member, Message};
use serde_json::Value;
use session::Session;

use super::chat::{
    NativeMember, aggregator_members, chat_type, graph_chat, graph_chat_from_aggregator,
    graph_member, one_on_one_ids, thread_members,
};
use super::message::graph_message;
use crate::error::{Error, Result};
use crate::remote::{ChatsPage, RemotePage};

const THREADS_IN_FLIGHT: usize = 6;
const OLDER_PAGE_MARKER: &str = "chatsvc:older";

/// Profiles looked up so far; `None` when the directory does not know the id, such as a bot.
static PROFILES: LazyLock<Mutex<HashMap<String, Option<UserProfile>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// Thread members by chat, kept while the thread's roster version stays the same.
static ROSTERS: LazyLock<Mutex<HashMap<String, (String, Vec<NativeMember>)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Chats, members and messages read from the Teams chat service instead of Graph.
pub struct NativeChats<'a> {
    session: &'a Session,
}

impl<'a> NativeChats<'a> {
    pub fn new(session: &'a Session) -> Self {
        NativeChats { session }
    }

    pub async fn first_page(&self, page_size: usize) -> Result<ChatsPage> {
        let url = ChatListing::new(self.session).first_page_url(page_size);
        self.page(&url).await
    }

    pub async fn page(&self, url: &str) -> Result<ChatsPage> {
        let listing = ChatListing::new(self.session);
        let page = listing.page(url).await?;
        let conversations: Vec<&Value> = page
            .conversations
            .iter()
            .filter(|conversation| chat_type(conversation).is_some())
            .collect();
        let keys: Vec<(String, String)> = conversations
            .iter()
            .map(|conversation| {
                (
                    conversation["id"].as_str().unwrap_or_default().to_owned(),
                    conversation["threadProperties"]["rosterVersion"].to_string(),
                )
            })
            .collect();
        let rosters: Vec<Vec<NativeMember>> = stream::iter(keys)
            .map(|(chat_id, version)| self.roster(&listing, chat_id, version))
            .buffered(THREADS_IN_FLIGHT)
            .collect()
            .await;
        let user_ids: Vec<String> = rosters
            .iter()
            .flatten()
            .map(|member| member.user_id.clone())
            .collect();
        self.learn_profiles(&user_ids).await;
        let items = conversations
            .into_iter()
            .zip(rosters)
            .filter_map(|(conversation, roster)| graph_chat(conversation, self.named(roster, true)))
            .collect();
        Ok(ChatsPage {
            items,
            next_link: page.next_url,
        })
    }

    pub async fn chat(&self, chat_id: &str) -> Result<Chat> {
        let chat = chatsvc::aggregator_chat(self.session, chat_id).await?;
        let mut members = aggregator_members(&chat);
        let user_ids: Vec<String> = members
            .iter()
            .map(|member| member.user_id.clone())
            .collect();
        self.learn_profiles(&user_ids).await;
        for member in &mut members {
            if member.name.is_none() {
                member.name = profile_name(&member.user_id);
            }
        }
        let members = members
            .iter()
            .map(|member| graph_member(member, profile_email(&member.user_id).as_deref()))
            .collect();
        graph_chat_from_aggregator(&chat, members)
            .ok_or_else(|| Error::Unsupported("a chat the chat service aggregator cannot describe"))
    }

    /// A chat just created here, before the chat service lists it.
    pub async fn created_chat(
        &self,
        chat_id: &str,
        kind: &str,
        topic: Option<&str>,
        user_ids: &[String],
    ) -> Result<Chat> {
        self.learn_profiles(user_ids).await;
        let members: Vec<NativeMember> = user_ids
            .iter()
            .map(|user_id| NativeMember {
                user_id: user_id.clone(),
                owner: kind != "oneOnOne",
                name: None,
            })
            .collect();
        serde_json::from_value(serde_json::json!({
            "id": chat_id,
            "topic": topic,
            "chatType": kind,
            "createdDateTime": Utc::now().to_rfc3339(),
            "members": self.named(members, false),
        }))
        .map_err(|_| Error::Unsupported("a new chat without a Graph shape"))
    }

    pub async fn members(&self, chat_id: &str) -> Result<Vec<Member>> {
        let listing = ChatListing::new(self.session);
        let thread = listing.thread(chat_id).await?;
        let members = thread_members(&thread);
        let user_ids: Vec<String> = members
            .iter()
            .map(|member| member.user_id.clone())
            .collect();
        self.learn_profiles(&user_ids).await;
        let named = self.named(members, false);
        Ok(named
            .into_iter()
            .filter_map(|member| serde_json::from_value(member).ok())
            .collect())
    }

    pub async fn messages(
        &self,
        chat_id: &str,
        before: Option<DateTime<Utc>>,
        page_size: usize,
    ) -> Result<RemotePage> {
        let page = ChatListing::new(self.session)
            .messages(
                chat_id,
                before.map(|time| time.timestamp_millis()),
                page_size,
            )
            .await?;
        Ok(RemotePage {
            items: page
                .messages
                .iter()
                .filter_map(|message| graph_message(chat_id, message))
                .collect(),
            next_link: page.has_older.then(|| OLDER_PAGE_MARKER.to_owned()),
        })
    }

    pub async fn message(&self, chat_id: &str, message_id: &str) -> Result<Message> {
        let native = ChatListing::new(self.session)
            .message(chat_id, message_id)
            .await?;
        graph_message(chat_id, &native).ok_or(Error::Unsupported(
            "a chat service message without a Graph shape",
        ))
    }

    async fn roster(
        &self,
        listing: &ChatListing,
        chat_id: String,
        version: String,
    ) -> Vec<NativeMember> {
        let chat_id = chat_id.as_str();
        if chat_id.is_empty() {
            return Vec::new();
        }
        if let Some(pair) = one_on_one_ids(chat_id) {
            return pair
                .into_iter()
                .map(|user_id| NativeMember {
                    user_id,
                    owner: false,
                    name: None,
                })
                .collect();
        }
        if let Some((known, members)) = ROSTERS.lock().unwrap().get(chat_id)
            && *known == version
        {
            return members.clone();
        }
        match listing.thread(chat_id).await {
            Ok(thread) => {
                let members = thread_members(&thread);
                ROSTERS
                    .lock()
                    .unwrap()
                    .insert(chat_id.to_owned(), (version, members.clone()));
                members
            }
            Err(_) => ROSTERS
                .lock()
                .unwrap()
                .get(chat_id)
                .map(|(_, members)| members.clone())
                .unwrap_or_default(),
        }
    }

    async fn learn_profiles(&self, user_ids: &[String]) {
        let unknown: Vec<String> = {
            let known = PROFILES.lock().unwrap();
            let mut unknown: Vec<String> = user_ids
                .iter()
                .filter(|user_id| !known.contains_key(*user_id))
                .cloned()
                .collect();
            unknown.sort();
            unknown.dedup();
            unknown
        };
        if unknown.is_empty() {
            return;
        }
        let Ok(profiles) = UserProfiles::new(self.session).fetch(&unknown).await else {
            return;
        };
        let mut known = PROFILES.lock().unwrap();
        for user_id in unknown {
            known.insert(user_id, None);
        }
        for profile in profiles {
            known.insert(profile.user_id.clone(), Some(profile));
        }
    }

    /// `drop_unknown` leaves out ids the directory does not know, which in a 1:1 is a bot.
    fn named(&self, members: Vec<NativeMember>, drop_unknown: bool) -> Vec<Value> {
        let known = PROFILES.lock().unwrap();
        members
            .into_iter()
            .filter(|member| !drop_unknown || !matches!(known.get(&member.user_id), Some(None)))
            .map(|mut member| {
                let profile = known.get(&member.user_id).cloned().flatten();
                if member.name.is_none() {
                    member.name = profile
                        .as_ref()
                        .and_then(|profile| profile.display_name.clone());
                }
                graph_member(
                    &member,
                    profile
                        .as_ref()
                        .and_then(|profile| profile.email.as_deref()),
                )
            })
            .collect()
    }
}

/// What the directory lookups learned about a user, for callers outside the chat path.
pub fn known_profile(user_id: &str) -> Option<UserProfile> {
    PROFILES.lock().unwrap().get(user_id).cloned().flatten()
}

fn profile_name(user_id: &str) -> Option<String> {
    known_profile(user_id).and_then(|profile| profile.display_name)
}

fn profile_email(user_id: &str) -> Option<String> {
    known_profile(user_id).and_then(|profile| profile.email)
}
