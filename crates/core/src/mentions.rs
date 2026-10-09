use graph::{MentionTarget, OutgoingMention};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::markdown::escape_html;

/// `text` is the display name exactly as it appears after the `@` in the message text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MentionInput {
    pub target: MentionTarget,
    pub text: String,
}

impl MentionInput {
    pub fn user(user_id: &str, text: &str) -> Self {
        MentionInput {
            target: MentionTarget::User {
                user_id: user_id.to_owned(),
            },
            text: text.to_owned(),
        }
    }

    pub fn channel(channel_id: &str, text: &str) -> Self {
        MentionInput {
            target: MentionTarget::Channel {
                channel_id: channel_id.to_owned(),
            },
            text: text.to_owned(),
        }
    }

    pub fn team(team_id: &str, text: &str) -> Self {
        MentionInput {
            target: MentionTarget::Team {
                team_id: team_id.to_owned(),
            },
            text: text.to_owned(),
        }
    }
}

/// Mentions are matched in order, each at the first `@text` after the previous one. Unmatched mentions stay plain text.
pub(crate) fn apply_mentions(
    html: &str,
    inputs: &[MentionInput],
) -> (String, Vec<OutgoingMention>) {
    let mut output = String::with_capacity(html.len());
    let mut outgoing = Vec::new();
    let mut cursor = 0;
    for input in inputs {
        let escaped = escape_html(&input.text);
        let needle = format!("@{escaped}");
        let Some(offset) = html[cursor..].find(&needle) else {
            continue;
        };
        let id = outgoing.len() as u32;
        output.push_str(&html[cursor..cursor + offset]);
        output.push_str(&format!("<at id=\"{id}\">{escaped}</at>"));
        cursor += offset + needle.len();
        outgoing.push(OutgoingMention {
            id,
            text: input.text.clone(),
            target: input.target.clone(),
        });
    }
    output.push_str(&html[cursor..]);
    (output, outgoing)
}

pub(crate) fn ensure_allowed_in_chat(inputs: &[MentionInput]) -> Result<()> {
    if inputs
        .iter()
        .any(|input| !matches!(input.target, MentionTarget::User { .. }))
    {
        return Err(Error::Unsupported("a channel or team mention in a chat"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mentions_become_at_tags_with_sequential_ids() {
        let (html, mentions) = apply_mentions(
            "hi @Ada Example and @Bo, see @General",
            &[
                MentionInput::user("u1", "Ada Example"),
                MentionInput::user("u2", "Bo"),
                MentionInput::channel("19:c", "General"),
            ],
        );
        assert_eq!(
            html,
            "hi <at id=\"0\">Ada Example</at> and <at id=\"1\">Bo</at>, see <at id=\"2\">General</at>"
        );
        assert_eq!(
            mentions
                .iter()
                .map(|mention| mention.id)
                .collect::<Vec<_>>(),
            [0, 1, 2]
        );
    }

    #[test]
    fn repeated_names_are_matched_left_to_right() {
        let (html, mentions) = apply_mentions(
            "@Ada then @Ada",
            &[
                MentionInput::user("u1", "Ada"),
                MentionInput::user("u2", "Ada"),
            ],
        );
        assert_eq!(html, "<at id=\"0\">Ada</at> then <at id=\"1\">Ada</at>");
        assert_eq!(mentions.len(), 2);
    }

    #[test]
    fn unmatched_mentions_are_dropped_and_ids_stay_dense() {
        let (html, mentions) = apply_mentions(
            "@Bo only",
            &[
                MentionInput::user("u1", "Ada"),
                MentionInput::user("u2", "Bo"),
            ],
        );
        assert_eq!(html, "<at id=\"0\">Bo</at> only");
        assert_eq!(mentions.len(), 1);
        assert_eq!(mentions[0].id, 0);
    }

    #[test]
    fn names_are_html_escaped_like_the_body() {
        let (html, mentions) = apply_mentions(
            &crate::markdown_to_html("ping @Ada & Co <x>"),
            &[MentionInput::user("u1", "Ada & Co")],
        );
        assert_eq!(html, "ping <at id=\"0\">Ada &amp; Co</at> &lt;x&gt;");
        assert_eq!(mentions[0].text, "Ada & Co");
    }

    #[test]
    fn mentions_inside_formatted_draft_html_become_at_tags() {
        let (html, mentions) = apply_mentions(
            &crate::Draft::from_markdown("**hi @Ada** and `@Bo`").to_html(),
            &[MentionInput::user("u1", "Ada")],
        );
        assert_eq!(html, "<b>hi <at id=\"0\">Ada</at></b> and <code>@Bo</code>");
        assert_eq!(mentions.len(), 1);
    }

    #[test]
    fn chats_only_take_user_mentions() {
        assert!(ensure_allowed_in_chat(&[MentionInput::user("u", "A")]).is_ok());
        assert!(ensure_allowed_in_chat(&[MentionInput::team("t", "Squad")]).is_err());
    }
}
