pub fn mask_conversation_id(conversation_id: &str) -> String {
    let Some((prefix, rest)) = conversation_id.split_once(':') else {
        return "xxx".into();
    };
    if prefix == "48" {
        return conversation_id.to_owned();
    }
    let suffix = rest.find('@').map(|at| &rest[at..]).unwrap_or_default();
    format!("{prefix}:xxx{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_kind_and_domain_only() {
        assert_eq!(
            mask_conversation_id("19:abc_def@unq.gbl.spaces"),
            "19:xxx@unq.gbl.spaces"
        );
        assert_eq!(
            mask_conversation_id("19:meeting_abc@thread.v2"),
            "19:xxx@thread.v2"
        );
        assert_eq!(mask_conversation_id("19:abc"), "19:xxx");
    }

    #[test]
    fn notes_chat_is_not_personal() {
        assert_eq!(mask_conversation_id("48:notes"), "48:notes");
    }

    #[test]
    fn unknown_shapes_are_fully_masked() {
        assert_eq!(mask_conversation_id("opaque"), "xxx");
    }
}
