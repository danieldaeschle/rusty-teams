use serde_json::{Value, json};

pub fn messaging_policy_body() -> Value {
    json!({"messagingPolicy": true})
}

/// Teams reads `messagingPolicy.allowPriorityMessages` and allows urgent messages unless it is `false`.
pub fn parse_allow_priority_messages(body: &Value) -> bool {
    body.pointer("/messagingPolicy/value/allowPriorityMessages")
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_body_asks_for_the_messaging_policy() {
        assert_eq!(messaging_policy_body(), json!({"messagingPolicy": true}));
    }

    #[test]
    fn priority_messages_follow_the_policy_and_default_to_allowed() {
        let denied = json!({"messagingPolicy": {"value": {"allowPriorityMessages": false}}});
        let allowed = json!({"messagingPolicy": {"value": {"allowPriorityMessages": true}}});
        assert!(!parse_allow_priority_messages(&denied));
        assert!(parse_allow_priority_messages(&allowed));
        assert!(parse_allow_priority_messages(&json!({})));
    }
}
