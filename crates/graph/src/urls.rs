use chrono::{DateTime, SecondsFormat, Utc};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use session::GRAPH;

const ENCODE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

pub fn segment(value: &str) -> String {
    utf8_percent_encode(value, ENCODE).to_string()
}

pub fn me() -> String {
    format!("{GRAPH}/v1.0/me?$select=id,displayName,mail,userPrincipalName")
}

pub fn user_photo(user_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/users/{}/photos/48x48/$value",
        segment(user_id)
    )
}

pub fn user_profile(user_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/users/{}?$select=id,displayName,jobTitle,department,companyName,\
         officeLocation,mail,userPrincipalName,businessPhones,mobilePhone",
        segment(user_id)
    )
}

pub fn user_manager(user_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/users/{}/manager?$select=id,displayName,jobTitle",
        segment(user_id)
    )
}

pub fn user_direct_reports(user_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/users/{}/directReports?$select=id,displayName,jobTitle",
        segment(user_id)
    )
}

pub fn get_schedule() -> String {
    format!("{GRAPH}/v1.0/me/calendar/getSchedule")
}

pub fn chats(top: usize) -> String {
    format!(
        "{GRAPH}/v1.0/me/chats?$expand=members,lastMessagePreview\
         &$orderby=lastMessagePreview/createdDateTime%20desc&$top={top}"
    )
}

pub fn chat(chat_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/chats/{}?$expand=members,lastMessagePreview",
        segment(chat_id)
    )
}

pub fn chat_members(chat_id: &str) -> String {
    format!("{GRAPH}/v1.0/chats/{}/members", segment(chat_id))
}

pub fn chat_message(chat_id: &str, message_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/chats/{}/messages/{}",
        segment(chat_id),
        segment(message_id)
    )
}

pub fn soft_delete(user_id: &str, chat_id: &str, message_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/users/{}/chats/{}/messages/{}/softDelete",
        segment(user_id),
        segment(chat_id),
        segment(message_id)
    )
}

pub fn create_chat() -> String {
    format!("{GRAPH}/v1.0/chats")
}

pub fn chat_messages(chat_id: &str, before: Option<DateTime<Utc>>, top: usize) -> String {
    let mut url = format!(
        "{GRAPH}/v1.0/chats/{}/messages?$top={top}&$orderby=createdDateTime%20desc",
        segment(chat_id)
    );
    if let Some(before) = before {
        let stamp = before.to_rfc3339_opts(SecondsFormat::Millis, true);
        url.push_str(&format!(
            "&$filter=createdDateTime%20lt%20{}",
            segment(&stamp)
        ));
    }
    url
}

pub fn chat_message_collection(chat_id: &str) -> String {
    format!("{GRAPH}/v1.0/chats/{}/messages", segment(chat_id))
}

pub fn reply_with_quote(chat_id: &str) -> String {
    format!(
        "{GRAPH}/beta/chats/{}/messages/replyWithQuote",
        segment(chat_id)
    )
}

pub const CHAT_FILES_FOLDER: &str = "Microsoft Teams Chat Files";

pub fn chat_files_upload_session(file_name: &str) -> String {
    format!(
        "{GRAPH}/v1.0/me/drive/root:/{}/{}:/createUploadSession",
        segment(CHAT_FILES_FOLDER),
        segment(file_name)
    )
}

pub fn chat_files_item(file_name: &str) -> String {
    format!(
        "{GRAPH}/v1.0/me/drive/root:/{}/{}",
        segment(CHAT_FILES_FOLDER),
        segment(file_name)
    )
}

pub fn folder_item(drive_id: &str, folder_id: &str, file_name: &str) -> String {
    format!(
        "{GRAPH}/v1.0/drives/{}/items/{}:/{}",
        segment(drive_id),
        segment(folder_id),
        segment(file_name)
    )
}

pub fn folder_upload_session(drive_id: &str, folder_id: &str, file_name: &str) -> String {
    format!(
        "{GRAPH}/v1.0/drives/{}/items/{}:/{}:/createUploadSession",
        segment(drive_id),
        segment(folder_id),
        segment(file_name)
    )
}

pub fn channel_files_folder(team_id: &str, channel_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/teams/{}/channels/{}/filesFolder",
        segment(team_id),
        segment(channel_id)
    )
}

pub fn channel_tab(team_id: &str, channel_id: &str, tab_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/teams/{}/channels/{}/tabs/{}",
        segment(team_id),
        segment(channel_id),
        segment(tab_id)
    )
}

pub const CHILDREN_PAGE_SIZE: usize = 200;

pub fn folder_children(drive_id: &str, folder_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/drives/{}/items/{}/children?$top={CHILDREN_PAGE_SIZE}",
        segment(drive_id),
        segment(folder_id)
    )
}

pub fn folder_create(drive_id: &str, folder_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/drives/{}/items/{}/children",
        segment(drive_id),
        segment(folder_id)
    )
}

pub fn drive_item(drive_id: &str, item_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/drives/{}/items/{}",
        segment(drive_id),
        segment(item_id)
    )
}

pub fn shared_drive_item(share_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/shares/{}/driveItem",
        segment(share_id)
    )
}

pub fn drive_item_invite(drive_id: &str, item_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/drives/{}/items/{}/invite",
        segment(drive_id),
        segment(item_id)
    )
}

pub fn mark_chat_read(chat_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/chats/{}/markChatReadForUser",
        segment(chat_id)
    )
}

pub fn mark_chat_unread(chat_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/chats/{}/markChatUnreadForUser",
        segment(chat_id)
    )
}

pub fn hide_chat(chat_id: &str) -> String {
    format!("{GRAPH}/v1.0/chats/{}/hideForUser", segment(chat_id))
}

pub fn unhide_chat(chat_id: &str) -> String {
    format!("{GRAPH}/v1.0/chats/{}/unhideForUser", segment(chat_id))
}

pub fn chat_member(chat_id: &str, membership_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/chats/{}/members/{}",
        segment(chat_id),
        segment(membership_id)
    )
}

pub fn joined_teams() -> String {
    format!("{GRAPH}/v1.0/me/joinedTeams?$select=id,displayName")
}

pub fn channels(team_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/teams/{}/channels?$select=id,displayName,membershipType",
        segment(team_id)
    )
}

pub fn channel_messages(team_id: &str, channel_id: &str, top: usize) -> String {
    format!(
        "{GRAPH}/v1.0/teams/{}/channels/{}/messages?$top={top}&$expand=replies",
        segment(team_id),
        segment(channel_id)
    )
}

pub fn channel_post(team_id: &str, channel_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/teams/{}/channels/{}/messages",
        segment(team_id),
        segment(channel_id)
    )
}

pub fn channel_message(team_id: &str, channel_id: &str, message_id: &str) -> String {
    format!(
        "{}/{}",
        channel_post(team_id, channel_id),
        segment(message_id)
    )
}

pub fn channel_reply_post(team_id: &str, channel_id: &str, message_id: &str) -> String {
    format!(
        "{}/replies",
        channel_message(team_id, channel_id, message_id)
    )
}

pub fn channel_reply(team_id: &str, channel_id: &str, message_id: &str, reply_id: &str) -> String {
    format!(
        "{}/{}",
        channel_reply_post(team_id, channel_id, message_id),
        segment(reply_id)
    )
}

pub fn channel_replies(team_id: &str, channel_id: &str, message_id: &str, top: usize) -> String {
    format!(
        "{}?$top={top}",
        channel_reply_post(team_id, channel_id, message_id)
    )
}

pub fn channel_delta(
    team_id: &str,
    channel_id: &str,
    modified_after: Option<DateTime<Utc>>,
) -> String {
    let mut url = format!("{}/delta", channel_post(team_id, channel_id));
    if let Some(after) = modified_after {
        let stamp = after.to_rfc3339_opts(SecondsFormat::Millis, true);
        url.push_str(&format!(
            "?$filter=lastModifiedDateTime%20gt%20{}",
            segment(&stamp)
        ));
    }
    url
}

pub fn people_search(query: &str, top: usize) -> String {
    let term = query.replace('"', "");
    let search = segment(&format!("\"displayName:{term}\" OR \"mail:{term}\""));
    format!(
        "{GRAPH}/v1.0/users?$search={search}&$select=id,displayName,mail,userPrincipalName,jobTitle,department&$top={top}"
    )
}

pub fn user_by_address(address: &str) -> String {
    let escaped = address.replace('\'', "''");
    let filter = segment(&format!(
        "mail eq '{escaped}' or userPrincipalName eq '{escaped}'"
    ));
    format!(
        "{GRAPH}/v1.0/users?$filter={filter}&$select=id,displayName,mail,userPrincipalName,jobTitle,department"
    )
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn chat_ids_are_path_encoded() {
        assert_eq!(segment("19:abc@thread.v2"), "19%3Aabc%40thread.v2");
    }

    #[test]
    fn reply_with_quote_uses_the_beta_endpoint() {
        assert_eq!(
            reply_with_quote("19:abc@thread.v2"),
            "https://graph.microsoft.com/beta/chats/19%3Aabc%40thread.v2/messages/replyWithQuote"
        );
    }

    #[test]
    fn upload_session_urls_encode_the_file_name() {
        assert_eq!(
            chat_files_upload_session("Q3 plan (1).docx"),
            "https://graph.microsoft.com/v1.0/me/drive/root:/Microsoft%20Teams%20Chat%20Files/Q3%20plan%20%281%29.docx:/createUploadSession"
        );
        assert_eq!(
            folder_upload_session("b!d", "01F", "a.pdf"),
            "https://graph.microsoft.com/v1.0/drives/b%21d/items/01F:/a.pdf:/createUploadSession"
        );
    }

    #[test]
    fn chat_state_write_urls() {
        assert_eq!(
            mark_chat_unread("19:abc@thread.v2"),
            "https://graph.microsoft.com/v1.0/chats/19%3Aabc%40thread.v2/markChatUnreadForUser"
        );
        assert_eq!(
            hide_chat("19:abc@thread.v2"),
            "https://graph.microsoft.com/v1.0/chats/19%3Aabc%40thread.v2/hideForUser"
        );
        assert_eq!(
            unhide_chat("19:abc@thread.v2"),
            "https://graph.microsoft.com/v1.0/chats/19%3Aabc%40thread.v2/unhideForUser"
        );
        assert_eq!(
            chat_member("19:abc@thread.v2", "MTox"),
            "https://graph.microsoft.com/v1.0/chats/19%3Aabc%40thread.v2/members/MTox"
        );
    }

    #[test]
    fn chats_url_orders_newest_first() {
        let url = chats(50);
        assert!(url.starts_with("https://graph.microsoft.com/v1.0/me/chats?"));
        assert!(url.contains("$expand=members,lastMessagePreview"));
        assert!(url.contains("$orderby=lastMessagePreview/createdDateTime%20desc"));
        assert!(url.ends_with("$top=50"));
    }

    #[test]
    fn messages_url_without_cursor() {
        let url = chat_messages("19:abc@thread.v2", None, 30);
        assert_eq!(
            url,
            "https://graph.microsoft.com/v1.0/chats/19%3Aabc%40thread.v2/messages?$top=30&$orderby=createdDateTime%20desc"
        );
    }

    #[test]
    fn messages_url_with_cursor_filters_older() {
        let before = Utc.with_ymd_and_hms(2026, 10, 6, 9, 0, 0).unwrap();
        let url = chat_messages("c", Some(before), 50);
        assert!(
            url.ends_with("&$filter=createdDateTime%20lt%202026-10-06T09%3A00%3A00.000Z"),
            "{url}"
        );
    }

    #[test]
    fn channel_messages_expand_replies() {
        let url = channel_messages("team-1", "19:chan@thread.tacv2", 20);
        assert!(url.contains("/teams/team-1/channels/19%3Achan%40thread.tacv2/messages?"));
        assert!(url.ends_with("$top=20&$expand=replies"));
    }
}
