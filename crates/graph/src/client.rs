use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde_json::Value;
use session::{ApiResponse, GRAPH, Method, Request, Scope, Session};

use crate::error::{Error, Result};
use crate::models::{Channel, Chat, Member, Message, Team, User};
use crate::page::Page;
use crate::urls;

pub const CHAT_PAGE_SIZE: usize = 50;
pub const MESSAGE_PAGE_SIZE: usize = 50;
pub const BATCH_SIZE: usize = 20;
const PEOPLE_PAGE_SIZE: usize = 10;

pub struct Graph {
    session: Session,
}

impl Graph {
    pub fn new(session: Session) -> Self {
        Graph { session }
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub async fn me(&self) -> Result<User> {
        let body = self.get(&urls::me(), &Scope::graph("User.Read")).await?;
        Ok(serde_json::from_value(body)?)
    }

    pub async fn chats_page(&self, top: usize) -> Result<Page<Chat>> {
        self.get_page(&urls::chats(top), Scope::graph("Chat.Read"))
            .await
    }

    pub async fn chats_at(&self, next_link: &str) -> Result<Page<Chat>> {
        self.get_page(next_link, Scope::graph("Chat.Read")).await
    }

    pub async fn chat(&self, chat_id: &str) -> Result<Chat> {
        let body = self
            .get(&urls::chat(chat_id), &Scope::graph("Chat.Read"))
            .await?;
        Ok(serde_json::from_value(body)?)
    }

    pub async fn chat_members(&self, chat_id: &str) -> Result<Vec<Member>> {
        self.get_all(&urls::chat_members(chat_id), Scope::graph("Chat.Read"))
            .await
    }

    pub async fn chat_message(&self, chat_id: &str, message_id: &str) -> Result<Message> {
        let body = self
            .get(
                &urls::chat_message(chat_id, message_id),
                &Scope::graph("Chat.Read"),
            )
            .await?;
        Ok(serde_json::from_value(body)?)
    }

    pub async fn list_chats(&self, limit: usize) -> Result<Vec<Chat>> {
        let mut page = self.chats_page(limit.clamp(1, CHAT_PAGE_SIZE)).await?;
        let mut chats = std::mem::take(&mut page.items);
        while chats.len() < limit {
            let Some(mut next) = self.next_page(&page).await? else {
                break;
            };
            chats.append(&mut std::mem::take(&mut next.items));
            page = next;
        }
        chats.truncate(limit);
        Ok(chats)
    }

    pub async fn chat_messages(
        &self,
        chat_id: &str,
        before: Option<DateTime<Utc>>,
        top: usize,
    ) -> Result<Page<Message>> {
        let url = urls::chat_messages(chat_id, before, top.clamp(1, MESSAGE_PAGE_SIZE));
        self.get_page(&url, Scope::graph("Chat.Read")).await
    }

    pub async fn joined_teams(&self) -> Result<Vec<Team>> {
        self.get_all(&urls::joined_teams(), Scope::graph("Team.ReadBasic.All"))
            .await
    }

    pub async fn channels(&self, team_id: &str) -> Result<Vec<Channel>> {
        self.get_all(
            &urls::channels(team_id),
            Scope::graph("Channel.ReadBasic.All"),
        )
        .await
    }

    pub async fn channels_for_teams(
        &self,
        team_ids: &[String],
    ) -> Result<Vec<Result<Vec<Channel>>>> {
        let scope = Scope::graph("Channel.ReadBasic.All");
        let mut listings = Vec::with_capacity(team_ids.len());
        for chunk in team_ids.chunks(BATCH_SIZE) {
            let requests: Vec<Request> = chunk
                .iter()
                .map(|team_id| Request::get(urls::channels(team_id)))
                .collect();
            let answers = self.session.batch(&requests, &scope).await?;
            for (request, answer) in requests.iter().zip(answers) {
                listings.push(self.collect_pages(&request.url, answer, &scope).await);
            }
        }
        Ok(listings)
    }

    pub async fn channel_message(
        &self,
        team_id: &str,
        channel_id: &str,
        message_id: &str,
    ) -> Result<Message> {
        let url = urls::channel_message(team_id, channel_id, message_id);
        let body = self
            .get(&url, &Scope::graph("ChannelMessage.Read.All"))
            .await?;
        Ok(serde_json::from_value(body)?)
    }

    pub async fn channel_reply(
        &self,
        team_id: &str,
        channel_id: &str,
        message_id: &str,
        reply_id: &str,
    ) -> Result<Message> {
        let url = urls::channel_reply(team_id, channel_id, message_id, reply_id);
        let body = self
            .get(&url, &Scope::graph("ChannelMessage.Read.All"))
            .await?;
        Ok(serde_json::from_value(body)?)
    }

    pub async fn channel_replies(
        &self,
        team_id: &str,
        channel_id: &str,
        root_ids: &[String],
    ) -> Result<Vec<Result<Vec<Message>>>> {
        let scope = Scope::graph("ChannelMessage.Read.All");
        let mut listings = Vec::with_capacity(root_ids.len());
        for chunk in root_ids.chunks(BATCH_SIZE) {
            let requests: Vec<Request> = chunk
                .iter()
                .map(|root_id| {
                    Request::get(urls::channel_replies(
                        team_id,
                        channel_id,
                        root_id,
                        MESSAGE_PAGE_SIZE,
                    ))
                })
                .collect();
            let answers = self.session.batch(&requests, &scope).await?;
            for (request, answer) in requests.iter().zip(answers) {
                listings.push(self.collect_pages(&request.url, answer, &scope).await);
            }
        }
        Ok(listings)
    }

    pub async fn channel_delta(
        &self,
        team_id: &str,
        channel_id: &str,
        modified_after: Option<DateTime<Utc>>,
    ) -> Result<Page<Message>> {
        let url = urls::channel_delta(team_id, channel_id, modified_after);
        self.get_page(&url, Scope::graph("ChannelMessage.Read.All"))
            .await
    }

    pub async fn search_people(&self, query: &str) -> Result<Vec<User>> {
        let scope = Scope::graph("User.ReadBasic.All");
        let query = query.trim();
        if query.contains('@') {
            return Ok(self
                .get_page::<User>(&urls::user_by_address(query), scope)
                .await?
                .items);
        }
        let mut request = Request::get(urls::people_search(query, PEOPLE_PAGE_SIZE));
        request
            .headers
            .push(("ConsistencyLevel".to_owned(), "eventual".to_owned()));
        let answer = self
            .session
            .batch(std::slice::from_ref(&request), &scope)
            .await?
            .remove(0);
        self.collect_pages::<User>(&request.url, answer, &scope)
            .await
    }

    pub async fn channel_messages(
        &self,
        team_id: &str,
        channel_id: &str,
        top: usize,
    ) -> Result<Page<Message>> {
        let url = urls::channel_messages(team_id, channel_id, top.clamp(1, MESSAGE_PAGE_SIZE));
        self.get_page(&url, Scope::graph("ChannelMessage.Read.All"))
            .await
    }

    pub async fn channel_messages_at(&self, next_link: &str) -> Result<Page<Message>> {
        self.get_page(next_link, Scope::graph("ChannelMessage.Read.All"))
            .await
    }

    pub async fn channel_delta_at(&self, link: &str) -> Result<Page<Message>> {
        self.get_page(link, Scope::graph("ChannelMessage.Read.All"))
            .await
    }

    pub async fn next_page<T: DeserializeOwned>(&self, page: &Page<T>) -> Result<Option<Page<T>>> {
        let Some(link) = page.next_link.as_deref() else {
            return Ok(None);
        };
        Ok(Some(self.get_page(link, page.scope.clone()).await?))
    }

    pub(crate) async fn get(&self, url: &str, scope: &Scope) -> Result<Value> {
        ensure_graph_url(url)?;
        Ok(self
            .session
            .request(Method::Get, url, scope, None)
            .await?
            .body)
    }

    async fn get_page<T: DeserializeOwned>(&self, url: &str, scope: Scope) -> Result<Page<T>> {
        let body = self.get(url, &scope).await?;
        Page::parse(body, scope)
    }

    async fn collect_pages<T: DeserializeOwned>(
        &self,
        url: &str,
        answer: ApiResponse,
        scope: &Scope,
    ) -> Result<Vec<T>> {
        if !answer.is_success() {
            return Err(session::Error::api(answer.status, url, answer.body).into());
        }
        let mut page: Page<T> = Page::parse(answer.body, scope.clone())?;
        let mut items = std::mem::take(&mut page.items);
        while let Some(mut next) = self.next_page(&page).await? {
            items.append(&mut std::mem::take(&mut next.items));
            page = next;
        }
        Ok(items)
    }

    pub(crate) async fn get_all<T: DeserializeOwned>(
        &self,
        url: &str,
        scope: Scope,
    ) -> Result<Vec<T>> {
        let mut page: Page<T> = self.get_page(url, scope).await?;
        let mut items = std::mem::take(&mut page.items);
        while let Some(mut next) = self.next_page(&page).await? {
            items.append(&mut std::mem::take(&mut next.items));
            page = next;
        }
        Ok(items)
    }
}

pub(crate) fn ensure_graph_url(url: &str) -> Result<()> {
    match url.strip_prefix(GRAPH) {
        Some(rest) if rest.starts_with('/') => Ok(()),
        _ => Err(Error::ForeignNextLink),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_graph_urls_only() {
        assert!(ensure_graph_url("https://graph.microsoft.com/v1.0/me/chats?$skiptoken=x").is_ok());
        assert!(ensure_graph_url("https://graph.microsoft.com.evil.example/v1.0/me").is_err());
        assert!(ensure_graph_url("https://example.com/v1.0/me").is_err());
    }
}
