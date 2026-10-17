use store::MessageImportance;

/// Teams strings "Important!" / "Urgent!", shown in capitals (`text-transform: uppercase`).
pub const IMPORTANT_LABEL: &str = "IMPORTANT!";
pub const URGENT_LABEL: &str = "URGENT!";
pub const URGENT_TOAST_SUFFIX: &str = "sent an urgent message";

pub fn importance_label(importance: MessageImportance) -> Option<&'static str> {
    match importance {
        MessageImportance::Normal => None,
        MessageImportance::High => Some(IMPORTANT_LABEL),
        MessageImportance::Urgent => Some(URGENT_LABEL),
    }
}

pub fn urgent_toast_title(sender: &str) -> String {
    format!("{sender} {URGENT_TOAST_SUFFIX}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_match_the_teams_strings_in_capitals() {
        assert_eq!(importance_label(MessageImportance::Normal), None);
        assert_eq!(
            importance_label(MessageImportance::High),
            Some("IMPORTANT!")
        );
        assert_eq!(importance_label(MessageImportance::Urgent), Some("URGENT!"));
    }

    #[test]
    fn the_urgent_toast_title_names_the_sender() {
        assert_eq!(
            urgent_toast_title("Lea Schneider"),
            "Lea Schneider sent an urgent message"
        );
    }
}
