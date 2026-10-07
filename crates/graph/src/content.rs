use session::{GRAPH, Request, Scope};

use crate::client::{Graph, ensure_graph_url};
use crate::error::{Error, Result};
use crate::models::Photo;
use crate::people::decode_binary;

const CHAT_PATH: &str = "/v1.0/chats/";
const CHANNEL_PATH: &str = "/v1.0/teams/";

impl Graph {
    /// Binary GET of a Graph hosted content URL (`.../hostedContents/{id}/$value`).
    pub async fn download_hosted_content(&self, url: &str) -> Result<Photo> {
        ensure_graph_url(url)?;
        let scope = hosted_content_scope(url).ok_or(Error::ForeignNextLink)?;
        let request = Request::binary_get(url);
        let answer = self
            .session()
            .batch(std::slice::from_ref(&request), &scope)
            .await?
            .remove(0);
        decode_binary(url, answer)
    }
}

pub fn hosted_content_scope(url: &str) -> Option<Scope> {
    let path = url.strip_prefix(GRAPH)?;
    if path.starts_with(CHAT_PATH) {
        Some(Scope::graph("Chat.Read"))
    } else if path.starts_with(CHANNEL_PATH) {
        Some(Scope::graph("ChannelMessage.Read.All"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_follows_the_conversation_kind() {
        let chat =
            "https://graph.microsoft.com/v1.0/chats/19%3Aa/messages/1/hostedContents/2/$value";
        let channel = "https://graph.microsoft.com/v1.0/teams/t/channels/c/messages/1/hostedContents/2/$value";
        assert_eq!(hosted_content_scope(chat).unwrap().name, "Chat.Read");
        assert_eq!(
            hosted_content_scope(channel).unwrap().name,
            "ChannelMessage.Read.All"
        );
        assert!(hosted_content_scope("https://graph.microsoft.com/v1.0/me").is_none());
    }
}
