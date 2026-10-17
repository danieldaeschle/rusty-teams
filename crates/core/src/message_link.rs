use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::json;

const LINK_HOST: &str = "https://teams.microsoft.com";
const DEEP_LINK_HOST: &str = "https://teams.cloud.microsoft";

const ENCODE_COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelLinkInput {
    pub tenant_id: String,
    pub group_id: String,
    pub channel_id: String,
    pub team_name: String,
    pub channel_name: String,
    pub root_id: String,
    pub message_id: String,
    pub created_ms: i64,
}

fn encode(value: &str) -> String {
    utf8_percent_encode(value, ENCODE_COMPONENT).to_string()
}

/// `notes_oid` is the own object id, needed for the notes chat only.
pub fn chat_message_link(
    conversation_id: &str,
    message_id: &str,
    notes_oid: Option<&str>,
) -> String {
    let mut context = json!({"contextType": "chat"});
    if let Some(oid) = notes_oid {
        context["oid"] = json!(oid);
    }
    format!(
        "{LINK_HOST}/l/message/{conversation_id}/{message_id}?context={}",
        encode(&context.to_string())
    )
}

pub fn channel_message_link(input: &ChannelLinkInput) -> String {
    format!(
        "{LINK_HOST}/l/message/{}/{}?tenantId={}&groupId={}&parentMessageId={}&teamName={}&channelName={}&createdTime={}",
        input.channel_id,
        input.message_id,
        encode(&input.tenant_id),
        encode(&input.group_id),
        encode(&input.root_id),
        encode(&input.team_name),
        encode(&input.channel_name),
        input.created_ms
    )
}

pub fn channel_tab_link(channel_id: &str, tab_id: &str, label: &str) -> String {
    format!(
        "{LINK_HOST}/l/channel/{channel_id}/tab%3A%3A{}?label={}",
        encode(tab_id),
        encode(label)
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelAddress {
    pub tenant_id: String,
    pub group_id: String,
    pub channel_id: String,
    pub channel_name: String,
    pub membership_type: Option<String>,
}

pub fn channel_link(address: &ChannelAddress) -> String {
    let separate_roster = address.membership_type.as_deref().is_some_and(|kind| {
        kind.eq_ignore_ascii_case("private") || kind.eq_ignore_ascii_case("shared")
    });
    format!(
        "{DEEP_LINK_HOST}/l/channel/{}/{}?groupId={}&tenantId={}{}",
        encode(&address.channel_id),
        encode(&address.channel_name),
        encode(&address.group_id),
        encode(&address.tenant_id),
        if separate_roster { "&ngc=true" } else { "" }
    )
}

pub fn team_link(team_thread_id: &str, group_id: &str, tenant_id: &str) -> String {
    format!(
        "{DEEP_LINK_HOST}/l/team/{}/conversations?groupId={}&tenantId={}",
        encode(team_thread_id),
        encode(group_id),
        encode(tenant_id)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_link_keeps_ids_raw_and_encodes_context() {
        assert_eq!(
            chat_message_link("19:abc@thread.v2", "1791368000000", None),
            "https://teams.microsoft.com/l/message/19:abc@thread.v2/1791368000000?context=%7B%22contextType%22%3A%22chat%22%7D"
        );
    }

    #[test]
    fn notes_link_carries_the_object_id() {
        assert_eq!(
            chat_message_link("48:notes", "1791368000000", Some("aa-bb")),
            "https://teams.microsoft.com/l/message/48:notes/1791368000000?context=%7B%22contextType%22%3A%22chat%22%2C%22oid%22%3A%22aa-bb%22%7D"
        );
    }

    #[test]
    fn channel_link_encodes_names_and_orders_params() {
        let input = ChannelLinkInput {
            tenant_id: "tenant-1".into(),
            group_id: "group-1".into(),
            channel_id: "19:chan@thread.tacv2".into(),
            team_name: "R&D Team".into(),
            channel_name: "General: News".into(),
            root_id: "1791368000000".into(),
            message_id: "1791368000555".into(),
            created_ms: 1791368000555,
        };
        assert_eq!(
            channel_message_link(&input),
            "https://teams.microsoft.com/l/message/19:chan@thread.tacv2/1791368000555?tenantId=tenant-1&groupId=group-1&parentMessageId=1791368000000&teamName=R%26D%20Team&channelName=General%3A%20News&createdTime=1791368000555"
        );
    }

    #[test]
    fn tab_link_points_at_the_tab_in_its_channel() {
        assert_eq!(
            channel_tab_link("19:chan@thread.tacv2", "a1-b2", "Sprint board"),
            "https://teams.microsoft.com/l/channel/19:chan@thread.tacv2/tab%3A%3Aa1-b2?label=Sprint%20board"
        );
    }

    fn address(membership_type: Option<&str>) -> ChannelAddress {
        ChannelAddress {
            tenant_id: "tenant-1".into(),
            group_id: "group-1".into(),
            channel_id: "19:chan@thread.tacv2".into(),
            channel_name: "R&D News".into(),
            membership_type: membership_type.map(str::to_owned),
        }
    }

    #[test]
    fn channel_link_encodes_the_id_and_name_like_teams() {
        assert_eq!(
            channel_link(&address(Some("standard"))),
            "https://teams.cloud.microsoft/l/channel/19%3Achan%40thread.tacv2/R%26D%20News?groupId=group-1&tenantId=tenant-1"
        );
    }

    #[test]
    fn private_and_shared_channel_links_carry_ngc() {
        for kind in ["private", "shared"] {
            assert!(channel_link(&address(Some(kind))).ends_with("&tenantId=tenant-1&ngc=true"));
        }
        assert!(!channel_link(&address(None)).contains("ngc"));
    }

    #[test]
    fn team_link_points_at_the_conversations_of_the_team_thread() {
        assert_eq!(
            team_link("19:general@thread.tacv2", "group-1", "tenant-1"),
            "https://teams.cloud.microsoft/l/team/19%3Ageneral%40thread.tacv2/conversations?groupId=group-1&tenantId=tenant-1"
        );
    }
}
