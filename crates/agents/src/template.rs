const PLACEHOLDERS: [&str; 11] = [
    "message",
    "sender",
    "chat",
    "conversation_id",
    "message_id",
    "thread",
    "extra",
    "cwd",
    "session_id",
    "agent",
    "prompt",
];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown placeholder {{{0}}}")]
pub struct UnknownPlaceholder(pub String);

#[derive(Clone, Debug, Default)]
pub struct Values {
    pub message: String,
    pub sender: String,
    pub chat: String,
    pub conversation_id: String,
    pub message_id: String,
    pub thread: String,
    pub extra: String,
    pub cwd: String,
    pub session_id: String,
    pub agent: String,
    pub prompt: String,
}

impl Values {
    fn get(&self, name: &str) -> Option<&str> {
        match name {
            "message" => Some(&self.message),
            "sender" => Some(&self.sender),
            "chat" => Some(&self.chat),
            "conversation_id" => Some(&self.conversation_id),
            "message_id" => Some(&self.message_id),
            "thread" => Some(&self.thread),
            "extra" => Some(&self.extra),
            "cwd" => Some(&self.cwd),
            "session_id" => Some(&self.session_id),
            "agent" => Some(&self.agent),
            "prompt" => Some(&self.prompt),
            _ => None,
        }
    }
}

pub(crate) fn is_known_placeholder(name: &str) -> bool {
    PLACEHOLDERS.contains(&name)
}

fn next_placeholder(text: &str) -> Option<(usize, usize, &str)> {
    let mut search_from = 0;
    while let Some(offset) = text[search_from..].find('{') {
        let start = search_from + offset;
        let rest = &text[start + 1..];
        if let Some(end) = rest.find('}') {
            let name = &rest[..end];
            let is_identifier = !name.is_empty()
                && name
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_');
            if is_identifier {
                return Some((start, start + end + 2, name));
            }
        }
        search_from = start + 1;
    }
    None
}

pub fn placeholders_in(template: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut remaining = template;
    while let Some((_, end, name)) = next_placeholder(remaining) {
        names.push(name.to_string());
        remaining = &remaining[end..];
    }
    names
}

pub fn render(template: &str, values: &Values) -> Result<String, UnknownPlaceholder> {
    let mut output = String::with_capacity(template.len());
    let mut remaining = template;
    while let Some((start, end, name)) = next_placeholder(remaining) {
        let value = values
            .get(name)
            .ok_or_else(|| UnknownPlaceholder(name.to_string()))?;
        output.push_str(&remaining[..start]);
        output.push_str(value);
        remaining = &remaining[end..];
    }
    output.push_str(remaining);
    Ok(output)
}

pub fn split_command(command: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_token = false;
    let mut quote: Option<char> = None;
    for character in command.chars() {
        match quote {
            Some(open) if character == open => quote = None,
            Some(_) => current.push(character),
            None if character == '"' || character == '\'' => {
                quote = Some(character);
                in_token = true;
            }
            None if character.is_whitespace() => {
                if in_token {
                    tokens.push(std::mem::take(&mut current));
                    in_token = false;
                }
            }
            None => {
                current.push(character);
                in_token = true;
            }
        }
    }
    if in_token {
        tokens.push(current);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values() -> Values {
        Values {
            sender: "Ada".to_string(),
            prompt: "line one\nline two".to_string(),
            session_id: "abc".to_string(),
            ..Values::default()
        }
    }

    #[test]
    fn renders_known_placeholders() {
        let rendered = render("{sender} id={session_id} {extra}!", &values()).unwrap();
        assert_eq!(rendered, "Ada id=abc !");
    }

    #[test]
    fn rejects_unknown_placeholder() {
        let error = render("hello {nope}", &values()).unwrap_err();
        assert_eq!(error, UnknownPlaceholder("nope".to_string()));
    }

    #[test]
    fn keeps_non_placeholder_braces() {
        assert_eq!(
            render("{\"a\": 1} { x }", &values()).unwrap(),
            "{\"a\": 1} { x }"
        );
    }

    #[test]
    fn does_not_rescan_substituted_values() {
        let mut substituted = values();
        substituted.sender = "{prompt}".to_string();
        assert_eq!(render("{sender}", &substituted).unwrap(), "{prompt}");
    }

    #[test]
    fn lists_placeholders() {
        assert_eq!(
            placeholders_in("a {message} b {x} {message}"),
            vec!["message", "x", "message"]
        );
    }

    #[test]
    fn splits_with_quotes() {
        assert_eq!(
            split_command(r#"sh -c "echo 'a b'" 'c d' "" e"#),
            vec!["sh", "-c", "echo 'a b'", "c d", "", "e"]
        );
    }

    #[test]
    fn prompt_stays_one_token() {
        let tokens = split_command("copilot -p {prompt} --x");
        let rendered: Vec<String> = tokens
            .iter()
            .map(|token| render(token, &values()).unwrap())
            .collect();
        assert_eq!(rendered, vec!["copilot", "-p", "line one\nline two", "--x"]);
    }
}
