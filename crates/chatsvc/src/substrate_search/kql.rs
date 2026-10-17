const NOT_SOFT_DELETED: &str = "NOT (isClientSoftDeleted:TRUE)";
const SENDER_MRI_FIELD: &str =
    "Extension_SkypeSpaces_ConversationPost_Extension_FromSkypeInternalId_String";
const THREAD_TYPE_FIELD: &str =
    "Extension_SkypeSpaces_ConversationPost_Extension_ThreadType_String";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageFilter {
    InChat { thread_id: String },
    MentionsMe,
    HasAttachment,
    FromSender { mri: String },
}

impl MessageFilter {
    fn kql(&self, user_id: &str) -> String {
        match self {
            MessageFilter::InChat { thread_id } => {
                format!("clientthreadid:{thread_id} AND NOT ({THREAD_TYPE_FIELD}:(topic OR space))")
            }
            MessageFilter::MentionsMe => format!("mentions:{}", user_id.replace('-', "")),
            MessageFilter::HasAttachment => "hasattachment:yes".to_owned(),
            MessageFilter::FromSender { mri } => format!("({SENDER_MRI_FIELD}:{mri})"),
        }
    }
}

pub fn message_query_string(text: &str, filters: &[MessageFilter], user_id: &str) -> String {
    let mut clauses = vec![NOT_SOFT_DELETED.to_owned()];
    clauses.extend(filters.iter().map(|filter| filter.kql(user_id)));
    let text = text.trim();
    if !text.is_empty() {
        clauses.push(text.to_owned());
    }
    clauses.join(" AND ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_excludes_soft_deleted_messages_like_teams() {
        assert_eq!(
            message_query_string("christian", &[], "user"),
            "NOT (isClientSoftDeleted:TRUE) AND christian"
        );
    }

    #[test]
    fn mentions_me_uses_the_object_id_without_dashes() {
        let query = message_query_string(
            "release",
            &[MessageFilter::MentionsMe, MessageFilter::HasAttachment],
            "0a1b2c3d-0000-1111-2222-333344445555",
        );
        assert_eq!(
            query,
            "NOT (isClientSoftDeleted:TRUE) AND mentions:0a1b2c3d000011112222333344445555 AND hasattachment:yes AND release"
        );
    }

    #[test]
    fn in_chat_scopes_to_the_thread_and_skips_channel_posts() {
        let query = message_query_string(
            "",
            &[MessageFilter::InChat {
                thread_id: "19:abc@thread.v2".to_owned(),
            }],
            "user",
        );
        assert_eq!(
            query,
            "NOT (isClientSoftDeleted:TRUE) AND clientthreadid:19:abc@thread.v2 AND NOT (Extension_SkypeSpaces_ConversationPost_Extension_ThreadType_String:(topic OR space))"
        );
    }

    #[test]
    fn sender_filter_matches_the_exact_mri() {
        let query = message_query_string(
            "x",
            &[MessageFilter::FromSender {
                mri: "8:orgid:abc".to_owned(),
            }],
            "user",
        );
        assert!(query.contains(
            "(Extension_SkypeSpaces_ConversationPost_Extension_FromSkypeInternalId_String:8:orgid:abc)"
        ));
    }
}
