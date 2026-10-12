use serde_json::Value;

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedChats {
    pub folder_id: String,
    pub hierarchy_version: i64,
    pub chat_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedChannels {
    pub order_version: String,
    pub channel_ids: Vec<String>,
}

pub(super) fn hierarchy_version(body: &Value) -> Result<i64> {
    let value = body
        .get("folderHierarchyVersion")
        .ok_or_else(|| Error::UnexpectedAnswer("folderHierarchyVersion missing".into()))?;
    let parsed = match value {
        Value::Number(number) => number.as_i64().or_else(|| {
            number
                .as_f64()
                .filter(|float| float.is_finite() && float.fract() == 0.0)
                .filter(|float| {
                    *float >= i64::MIN as f64 && *float < i64::MAX as f64
                })
                .map(|float| float as i64)
        }),
        Value::String(text) => text.trim().parse::<i64>().ok(),
        _ => None,
    };
    parsed.ok_or_else(|| {
        Error::UnexpectedAnswer(format!(
            "folderHierarchyVersion is not an integer ({})",
            json_type_name(value)
        ))
    })
}

fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(number) if number.is_f64() => "float",
        Value::Number(_) => "integer",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

pub fn parse_pinned_chats(body: &Value) -> Result<PinnedChats> {
    let hierarchy_version = hierarchy_version(body)?;
    let folders = body
        .get("conversationFolders")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::UnexpectedAnswer("conversationFolders missing".into()))?;
    let favorites = folders
        .iter()
        .find(|folder| {
            folder.get("folderType").and_then(Value::as_str) == Some("Favorites")
                && folder.get("isDeleted").and_then(Value::as_bool) != Some(true)
        })
        .ok_or(Error::NoFavoritesFolder)?;
    let folder_id = favorites
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::UnexpectedAnswer("Favorites folder has no id".into()))?
        .to_owned();
    let chat_ids = favorites
        .get("conversationFolderItems")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("conversationId").and_then(Value::as_str))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Ok(PinnedChats {
        folder_id,
        hierarchy_version,
        chat_ids,
    })
}

pub fn parse_pinned_channels(body: &Value) -> Result<PinnedChannels> {
    let order_version = match body.get("orderVersion") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        _ => return Err(Error::UnexpectedAnswer("orderVersion missing".into())),
    };
    let channel_ids = body
        .get("pinChannelOrder")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Ok(PinnedChannels {
        order_version,
        channel_ids,
    })
}

pub fn error_code(body: &Value) -> Option<&str> {
    body.pointer("/error/errorCode")
        .or_else(|| body.pointer("/error/code"))
        .and_then(Value::as_str)
}

pub fn error_text(body: &Value) -> String {
    match body {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn folders_body() -> Value {
        json!({
            "folderHierarchyVersion": 1791368146075i64,
            "conversationFolderOrder": ["t~u~Favorites"],
            "conversationFolders": [
                {"id": "t~u~RecentChats", "folderType": "RecentChats", "conversationFolderItems": []},
                {"id": "t~u~Old", "folderType": "Favorites", "isDeleted": true, "conversationFolderItems": []},
                {"id": "t~u~Favorites", "name": "Team", "folderType": "Favorites", "conversationFolderItems": [
                    {"conversationId": "19:a@thread.v2", "createdTime": 1},
                    {"conversationId": "48:notes"},
                    {"threadType": "chat"}
                ]}
            ]
        })
    }

    #[test]
    fn picks_favorites_by_type_not_name() {
        let pinned = parse_pinned_chats(&folders_body()).unwrap();
        assert_eq!(pinned.folder_id, "t~u~Favorites");
        assert_eq!(pinned.hierarchy_version, 1791368146075);
        assert_eq!(pinned.chat_ids, ["19:a@thread.v2", "48:notes"]);
    }

    #[test]
    fn missing_favorites_folder_is_typed() {
        let body = json!({"folderHierarchyVersion": 1, "conversationFolders": []});
        assert!(matches!(
            parse_pinned_chats(&body),
            Err(Error::NoFavoritesFolder)
        ));
    }

    #[test]
    fn missing_version_is_unexpected() {
        assert!(matches!(
            parse_pinned_chats(&json!({"conversationFolders": []})),
            Err(Error::UnexpectedAnswer(_))
        ));
    }

    fn version_error(body: &Value) -> String {
        match hierarchy_version(body) {
            Err(Error::UnexpectedAnswer(message)) => message,
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn hierarchy_version_accepts_integer_string_and_integral_float() {
        let expected = 1791368146075i64;
        for value in [
            json!(expected),
            json!("1791368146075"),
            json!(" 1791368146075 "),
            json!(1791368146075.0),
        ] {
            assert_eq!(
                hierarchy_version(&json!({"folderHierarchyVersion": value})).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn hierarchy_version_rejects_fractional_float() {
        assert_eq!(
            version_error(&json!({"folderHierarchyVersion": 1.5})),
            "folderHierarchyVersion is not an integer (float)"
        );
    }

    #[test]
    fn hierarchy_version_names_missing_key_and_wrong_type() {
        assert_eq!(version_error(&json!({})), "folderHierarchyVersion missing");
        assert_eq!(
            version_error(&json!({"folderHierarchyVersion": "abc"})),
            "folderHierarchyVersion is not an integer (string)"
        );
        assert_eq!(
            version_error(&json!({"folderHierarchyVersion": {}})),
            "folderHierarchyVersion is not an integer (object)"
        );
    }

    #[test]
    fn channel_state_accepts_numeric_and_text_versions() {
        let numeric = parse_pinned_channels(
            &json!({"orderVersion": 42, "pinChannelOrder": ["19:c@thread.tacv2"]}),
        )
        .unwrap();
        assert_eq!(numeric.order_version, "42");
        assert_eq!(numeric.channel_ids, ["19:c@thread.tacv2"]);
        let text = parse_pinned_channels(&json!({"orderVersion": "v7"})).unwrap();
        assert_eq!(text.order_version, "v7");
        assert!(text.channel_ids.is_empty());
    }
}
