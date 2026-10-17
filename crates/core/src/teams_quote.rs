use graph::{MentionTarget, OutgoingMention};
use serde_json::{Value, json};
use store::MessageRecord;

use crate::markdown::escape_html;
use crate::preview::preview_text;

const REPLY_ITEMTYPE: &str = "http://schema.skype.com/Reply";
const MENTION_ITEMTYPE: &str = "http://schema.skype.com/Mention";
const ORGID_PREFIX: &str = "8:orgid:";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TeamsQuote {
    pub html: String,
    pub quoted_message: Value,
}

/// Teams' `createBlockquoteTag`: no quote without sender, name or preview.
pub(crate) fn teams_quote(record: &MessageRecord) -> Option<TeamsQuote> {
    let sender_mri = user_mri(record.sender_id.as_deref()?);
    let sender_name = record.sender_name.as_deref().filter(|name| !name.is_empty())?;
    let preview = preview_text(&record.body_html).filter(|preview| !preview.is_empty())?;
    let time = record.created_at.timestamp_millis();
    let html = format!(
        "<blockquote itemscope=\"\" itemtype=\"{REPLY_ITEMTYPE}\" itemid=\"{}\"><strong itemprop=\"mri\" itemid=\"{}\">{}</strong><span itemprop=\"time\" itemid=\"{time}\"></span><p itemprop=\"preview\">{}</p></blockquote>",
        escape_html(&record.message_id),
        escape_html(&sender_mri),
        escape_html(sender_name),
        escape_html(&preview),
    );
    let quoted_message = json!({
        "messageId": record.message_id,
        "sender": sender_mri,
        "time": time,
    });
    Some(TeamsQuote {
        html,
        quoted_message,
    })
}

pub(crate) fn quote_reply_properties(
    quote: &TeamsQuote,
    mentions: &[OutgoingMention],
    importance: Option<&str>,
) -> Option<Value> {
    let mut properties = json!({
        "importance": importance.unwrap_or_default(),
        "subject": "",
        "qtdMsgs": Value::Array(vec![quote.quoted_message.clone()]).to_string(),
    });
    if !mentions.is_empty() {
        let entries = mentions
            .iter()
            .map(|mention| match &mention.target {
                MentionTarget::User { user_id } => Some(json!({
                    "@type": MENTION_ITEMTYPE,
                    "itemid": mention.id,
                    "mri": user_mri(user_id),
                    "mentionType": "person",
                    "displayName": mention.text,
                })),
                MentionTarget::Channel { .. } | MentionTarget::Team { .. } => None,
            })
            .collect::<Option<Vec<_>>>()?;
        properties["mentions"] = json!(Value::Array(entries).to_string());
    }
    Some(properties)
}

pub(crate) fn chatsvc_mention_html(html: &str, mentions: &[OutgoingMention]) -> String {
    let mut converted = html.to_owned();
    for mention in mentions {
        converted = converted.replace(
            &format!("<at id=\"{}\">", mention.id),
            &format!(
                "<span itemscope=\"\" itemtype=\"{MENTION_ITEMTYPE}\" itemid=\"{}\">",
                mention.id
            ),
        );
    }
    converted.replace("</at>", "</span>")
}

fn user_mri(user_id: &str) -> String {
    if user_id.starts_with("8:") {
        user_id.to_owned()
    } else {
        format!("{ORGID_PREFIX}{user_id}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mention(id: u32, target: MentionTarget) -> OutgoingMention {
        OutgoingMention {
            id,
            text: "Ada".into(),
            target,
        }
    }

    #[test]
    fn person_mentions_become_teams_spans_and_a_mentions_property() {
        let quote = TeamsQuote {
            html: String::new(),
            quoted_message: json!({"messageId": "1"}),
        };
        let mentions = [mention(
            0,
            MentionTarget::User {
                user_id: "user-ada".into(),
            },
        )];
        let html = chatsvc_mention_html("hi <at id=\"0\">Ada</at>", &mentions);
        assert_eq!(
            html,
            "hi <span itemscope=\"\" itemtype=\"http://schema.skype.com/Mention\" itemid=\"0\">Ada</span>"
        );
        let properties = quote_reply_properties(&quote, &mentions, Some("high")).unwrap();
        assert_eq!(properties["importance"], "high");
        assert_eq!(properties["qtdMsgs"], "[{\"messageId\":\"1\"}]");
        let entries: Value =
            serde_json::from_str(properties["mentions"].as_str().unwrap()).unwrap();
        assert_eq!(entries[0]["mri"], "8:orgid:user-ada");
        assert_eq!(entries[0]["mentionType"], "person");
    }

    #[test]
    fn a_channel_mention_has_no_chatsvc_shape() {
        let quote = TeamsQuote {
            html: String::new(),
            quoted_message: Value::Null,
        };
        let mentions = [mention(
            0,
            MentionTarget::Channel {
                channel_id: "19:c".into(),
            },
        )];
        assert!(quote_reply_properties(&quote, &mentions, None).is_none());
    }
}
