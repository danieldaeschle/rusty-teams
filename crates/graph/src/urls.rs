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

const PHOTO_SIZE: &str = "240x240";

pub fn user_photo(user_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/users/{}/photos/{PHOTO_SIZE}/$value",
        segment(user_id)
    )
}

pub fn user_photo_original(user_id: &str) -> String {
    format!("{GRAPH}/v1.0/users/{}/photo/$value", segment(user_id))
}

pub fn team_photo(team_id: &str) -> String {
    format!("{GRAPH}/v1.0/teams/{}/photo/$value", segment(team_id))
}

pub fn user_profile(user_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/users/{}?$select=id,displayName,jobTitle,department,companyName,\
         officeLocation,mail,userPrincipalName,businessPhones,mobilePhone",
        segment(user_id)
    )
}

pub fn user_mail(user_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/users/{}?$select=mail,userPrincipalName",
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

pub fn chat_message(chat_id: &str, message_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/chats/{}/messages/{}",
        segment(chat_id),
        segment(message_id)
    )
}

pub fn soft_delete(user_id: &str, chat_id: &str, message_id: &str) -> String {
    user_chat_message_action(user_id, chat_id, message_id, "softDelete")
}

pub fn undo_soft_delete(user_id: &str, chat_id: &str, message_id: &str) -> String {
    user_chat_message_action(user_id, chat_id, message_id, "undoSoftDelete")
}

fn user_chat_message_action(
    user_id: &str,
    chat_id: &str,
    message_id: &str,
    action: &str,
) -> String {
    format!(
        "{GRAPH}/v1.0/users/{}/chats/{}/messages/{}/{action}",
        segment(user_id),
        segment(chat_id),
        segment(message_id)
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

pub fn channel_members(team_id: &str, channel_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/teams/{}/channels/{}/members?$select=id&$top=999",
        segment(team_id),
        segment(channel_id)
    )
}

pub fn channel_member_list(team_id: &str, channel_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/teams/{}/channels/{}/members?$top=999",
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

pub fn recent_drive_items(top: usize) -> String {
    format!("{GRAPH}/v1.0/me/drive/recent?$top={top}")
}

pub fn my_drive() -> String {
    format!("{GRAPH}/v1.0/me/drive?$select=id")
}

pub fn site_drive(site_id: &str) -> String {
    format!("{GRAPH}/v1.0/sites/{}/drive?$select=id", segment(site_id))
}

pub fn shared_with_me() -> String {
    format!("{GRAPH}/v1.0/me/drive/sharedWithMe?$top=100")
}

pub fn site(site_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/sites/{}?$select=id,displayName,webUrl",
        segment(site_id)
    )
}

pub fn shared_drive_item(share_id: &str) -> String {
    format!("{GRAPH}/v1.0/shares/{}/driveItem", segment(share_id))
}

pub fn drive_item_invite(drive_id: &str, item_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/drives/{}/items/{}/invite",
        segment(drive_id),
        segment(item_id)
    )
}

pub fn joined_teams() -> String {
    format!("{GRAPH}/v1.0/me/joinedTeams?$select=id,displayName")
}

pub fn team_tags(team_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/teams/{}/tags?$select=id,displayName,memberCount",
        segment(team_id)
    )
}

pub fn channels(team_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/teams/{}/channels?$select=id,displayName,membershipType",
        segment(team_id)
    )
}

pub fn channel_description(team_id: &str, channel_id: &str) -> String {
    format!(
        "{GRAPH}/v1.0/teams/{}/channels/{}?$select=description",
        segment(team_id),
        segment(channel_id)
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

pub fn people_search(query: &str, top: usize, with_alias: bool) -> String {
    let term = query.replace('"', "");
    let fields: &[&str] = if with_alias {
        &["displayName", "mail", "mailNickname", "userPrincipalName"]
    } else {
        &["displayName", "mail"]
    };
    let clauses: Vec<String> = fields
        .iter()
        .map(|field| format!("\"{field}:{term}\""))
        .collect();
    let search = segment(&clauses.join(" OR "));
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
    use super::*;

    #[test]
    fn channel_members_ask_for_ids_only_in_large_pages() {
        assert_eq!(
            channel_members("t 1", "19:c@thread.tacv2"),
            format!(
                "{GRAPH}/v1.0/teams/t%201/channels/19%3Ac%40thread.tacv2/members?$select=id&$top=999"
            )
        );
    }

    #[test]
    fn photos_ask_for_the_240_rendition_then_the_original() {
        assert_eq!(
            user_photo("a b"),
            format!("{GRAPH}/v1.0/users/a%20b/photos/240x240/$value")
        );
        assert_eq!(
            user_photo_original("a b"),
            format!("{GRAPH}/v1.0/users/a%20b/photo/$value")
        );
    }

    #[test]
    fn chat_ids_are_path_encoded() {
        assert_eq!(segment("19:abc@thread.v2"), "19%3Aabc%40thread.v2");
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
    fn channel_messages_expand_replies() {
        let url = channel_messages("team-1", "19:chan@thread.tacv2", 20);
        assert!(url.contains("/teams/team-1/channels/19%3Achan%40thread.tacv2/messages?"));
        assert!(url.ends_with("$top=20&$expand=replies"));
    }
}
