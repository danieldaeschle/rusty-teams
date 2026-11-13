use graph::{MentionTarget, OutgoingMention};
use serde_json::{Value, json};
use store::MessageRecord;

use crate::markdown::escape_html;
use crate::preview::preview_text;

const REPLY_ITEMTYPE: &str = "http://schema.skype.com/Reply";
const MENTION_ITEMTYPE: &str = "http://schema.skype.com/Mention";
const ORGID_PREFIX: &str = "8:orgid:";
const BOT_PREFIX: &str = "28:";
const FORMAT_VARIANT: &str = "TEAMS";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TeamsQuote {
    pub html: String,
    pub quoted_message: Value,
}

/// Teams' `createBlockquoteTag`: no quote without sender, name or preview.
pub(crate) fn teams_quote(record: &MessageRecord) -> Option<TeamsQuote> {
    let sender_mri = sender_mri(record)?;
    let sender_name = record
        .sender_name
        .as_deref()
        .filter(|name| !name.is_empty())?;
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

/// The `properties` Teams' `sendMessageInConv` writes; `None` when a mention has no chatsvc shape here.
pub(crate) fn message_properties(
    quote: Option<&TeamsQuote>,
    mentions: &[OutgoingMention],
    importance: Option<&str>,
    files: &Value,
) -> Option<Value> {
    let mention_entries = mention_entries(mentions)?;
    let quoted: Vec<Value> = quote
        .map(|quote| quote.quoted_message.clone())
        .into_iter()
        .collect();
    Some(json!({
        "importance": importance.unwrap_or_default(),
        "links": "[]",
        "mentions": Value::Array(mention_entries).to_string(),
        "onbehalfof": null,
        "files": files.to_string(),
        "qtdMsgs": Value::Array(quoted).to_string(),
        "formatVariant": FORMAT_VARIANT,
    }))
}

/// The chatsvc `mentions` entries; `None` when a mention is not a person.
pub(crate) fn mention_entries(mentions: &[OutgoingMention]) -> Option<Vec<Value>> {
    mentions.iter().map(mention_entry).collect()
}

fn mention_entry(mention: &OutgoingMention) -> Option<Value> {
    let MentionTarget::User { user_id } = &mention.target else {
        return None;
    };
    Some(json!({
        "@type": MENTION_ITEMTYPE,
        "itemid": mention.id.to_string(),
        "mri": user_mri(user_id),
        "mentionType": "person",
        "displayName": mention.text,
    }))
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

fn sender_mri(record: &MessageRecord) -> Option<String> {
    match (&record.sender_id, &record.sender_application_id) {
        (Some(user_id), _) => Some(user_mri(user_id)),
        (None, Some(application_id)) if application_id.contains(':') => {
            Some(application_id.clone())
        }
        (None, Some(application_id)) => Some(format!("{BOT_PREFIX}{application_id}")),
        (None, None) => None,
    }
}

fn user_mri(user_id: &str) -> String {
    if user_id.contains(':') {
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

    fn quote() -> TeamsQuote {
        TeamsQuote {
            html: String::new(),
            quoted_message: json!({"messageId": "1"}),
        }
    }

    #[test]
    fn a_quote_reply_gets_the_teams_property_set() {
        let properties = message_properties(Some(&quote()), &[], None, &json!([])).unwrap();
        assert_eq!(
            properties,
            json!({
                "importance": "",
                "links": "[]",
                "mentions": "[]",
                "onbehalfof": null,
                "files": "[]",
                "qtdMsgs": "[{\"messageId\":\"1\"}]",
                "formatVariant": "TEAMS",
            })
        );
    }

    #[test]
    fn person_mentions_become_teams_spans_and_a_mentions_property() {
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
        let properties =
            message_properties(Some(&quote()), &mentions, Some("high"), &json!([])).unwrap();
        assert_eq!(properties["importance"], "high");
        assert_eq!(properties["qtdMsgs"], "[{\"messageId\":\"1\"}]");
        let entries: Value =
            serde_json::from_str(properties["mentions"].as_str().unwrap()).unwrap();
        assert_eq!(
            entries,
            json!([{
                "@type": "http://schema.skype.com/Mention",
                "itemid": "0",
                "mri": "8:orgid:user-ada",
                "mentionType": "person",
                "displayName": "Ada",
            }])
        );
    }

    #[test]
    fn group_mentions_have_no_chatsvc_shape() {
        for target in [
            MentionTarget::Channel {
                channel_id: "19:c@thread.tacv2".into(),
            },
            MentionTarget::Team {
                team_id: "team-1".into(),
            },
            MentionTarget::Everyone {
                chat_id: "19:g@thread.v2".into(),
            },
            MentionTarget::Tag {
                tag_id: "tag-1".into(),
            },
        ] {
            let mentions = [mention(0, target)];
            assert!(message_properties(Some(&quote()), &mentions, None, &json!([])).is_none());
        }
    }

    #[test]
    fn a_bot_sender_is_quoted_with_its_28_mri() {
        let record = MessageRecord {
            message_id: "5".into(),
            sender_name: Some("Echo".into()),
            sender_application_id: Some("app-1".into()),
            body_html: "<p>pong</p>".into(),
            ..MessageRecord::default()
        };
        let quote = teams_quote(&record).unwrap();
        assert_eq!(quote.quoted_message["sender"], "28:app-1");
        assert!(quote.html.contains("itemid=\"28:app-1\">Echo</strong>"));
    }
}
