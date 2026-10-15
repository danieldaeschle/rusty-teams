use serde_json::Value;

use crate::trouter_events::CallEnd;

const OUTCOME_KEYS: [&str; 6] = ["code", "subCode", "phrase", "reason", "resultCategories", "failureReason"];
const MAX_FIELDS: usize = 12;

pub fn outcome_summary(body: &Value) -> String {
    let mut fields = Vec::new();
    collect_fields(body, "", &mut fields);
    if fields.is_empty() {
        return format!("keys [{}]", top_level_keys(body));
    }
    fields.join(" ")
}

pub fn embedded_end(body: &Value) -> Option<CallEnd> {
    let holder = find_code_holder(body)?;
    Some(CallEnd {
        code: holder["code"].as_i64()?,
        sub_code: holder["subCode"].as_i64().unwrap_or_default(),
        phrase: holder["phrase"].as_str().unwrap_or_default().to_owned(),
        accepted_elsewhere_by: None,
    })
}

pub fn failed_participant_count(body: &Value) -> usize {
    body["participantInfos"].as_array().map_or(1, |infos| infos.len().max(1))
}

fn collect_fields(value: &Value, path: &str, fields: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if fields.len() >= MAX_FIELDS {
                    return;
                }
                let child_path = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
                if OUTCOME_KEYS.contains(&key.as_str()) && !child.is_object() {
                    fields.push(format!("{child_path}={}", scalar_text(child)));
                } else {
                    collect_fields(child, &child_path, fields);
                }
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                collect_fields(item, &format!("{path}[{index}]"), fields);
            }
        }
        _ => {}
    }
}

fn scalar_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn top_level_keys(body: &Value) -> String {
    body.as_object()
        .map(|map| map.keys().cloned().collect::<Vec<_>>().join(","))
        .unwrap_or_default()
}

fn find_code_holder(value: &Value) -> Option<&Value> {
    match value {
        Value::Object(map) => {
            if map.get("code").is_some_and(Value::is_i64) {
                return Some(value);
            }
            map.values().find_map(find_code_holder)
        }
        Value::Array(items) => items.iter().find_map(find_code_holder),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_summary_keeps_codes_and_drops_names() {
        let body = json!({"conversationEnd": {"reason": "noError", "code": 480, "subCode": 10037, "phrase": "No endpoint",
            "participant": {"displayName": "Bea", "id": "8:orgid:bea"}}});
        let summary = outcome_summary(&body);
        assert!(summary.contains("conversationEnd.code=480"));
        assert!(summary.contains("conversationEnd.subCode=10037"));
        assert!(summary.contains("conversationEnd.phrase=No endpoint"));
        assert!(!summary.contains("Bea") && !summary.contains("orgid"));
    }

    #[test]
    fn failures_count_every_listed_participant() {
        assert_eq!(failed_participant_count(&json!({"participantInfos": [{"id": "a"}, {"id": "b"}]})), 2);
        assert_eq!(failed_participant_count(&json!({"participantInfos": []})), 1);
        assert_eq!(failed_participant_count(&json!({})), 1);
    }

    #[test]
    fn a_body_without_codes_lists_its_keys() {
        assert_eq!(outcome_summary(&json!({"conversationEnd": {}, "links": {}})), "keys [conversationEnd,links]");
    }

    #[test]
    fn the_first_nested_code_becomes_the_end() {
        let body = json!({"participantInfos": [{"id": "x", "failure": {"code": 408, "subCode": 10408, "phrase": "timeout"}}]});
        let end = embedded_end(&body).unwrap();
        assert_eq!((end.code, end.sub_code, end.phrase.as_str()), (408, 10408, "timeout"));
        assert!(embedded_end(&json!({"conversationEnd": {"reason": "x"}})).is_none());
    }
}
