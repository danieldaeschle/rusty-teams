use agents::{OutputFormat, OutputParser, RunEvent, parse_line};

fn parse_fixture(format: OutputFormat, name: &str) -> Vec<RunEvent> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let content = std::fs::read_to_string(path).unwrap();
    let mut parser = OutputParser::new(format);
    let mut events: Vec<RunEvent> = content.lines().flat_map(|line| parser.feed(line)).collect();
    events.extend(parser.finish());
    events
}

#[test]
fn claude_fixture() {
    let events = parse_fixture(OutputFormat::ClaudeStreamJson, "claude-stream.jsonl");
    assert!(events.contains(&RunEvent::Step("Reading hostname".to_string())));
    assert!(events.contains(&RunEvent::SessionId(
        "3f6a2c1e-6f53-4c8e-9a51-7c2b5d0e9a11".to_string()
    )));
    assert_eq!(
        events.last(),
        Some(&RunEvent::Final(
            "The first word in `/etc/hostname` is `HOST01`.".to_string()
        ))
    );
    assert!(events.contains(&RunEvent::Text(
        "The first word in `/etc/hostname` is `HOST01`.".to_string()
    )));
}

#[test]
fn copilot_fixture() {
    let events = parse_fixture(OutputFormat::CopilotJsonl, "copilot.jsonl");
    assert!(events.iter().any(
        |event| matches!(event, RunEvent::Step(step) if step.contains("Viewing file") && step.contains("/etc/hostname"))
    ));
    assert!(events.contains(&RunEvent::SessionId(
        "7d1e9b2a-3c4f-4a5b-8e6d-1f2a3b4c5d6e".to_string()
    )));
    assert!(
        matches!(events.last(), Some(RunEvent::Final(text)) if text.starts_with("I couldn't read"))
    );
}

#[test]
fn copilot_failure_without_text() {
    let events = parse_line(
        OutputFormat::CopilotJsonl,
        r#"{"type":"result","sessionId":"s","exitCode":1}"#,
    );
    assert!(matches!(events.last(), Some(RunEvent::Failed(_))));
}

#[test]
fn claude_tool_labels_and_failures() {
    let label = |name: &str, input: &str| {
        let line = format!(
            r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","name":"{name}","input":{input}}}]}}}}"#
        );
        parse_line(OutputFormat::ClaudeStreamJson, &line)
    };
    assert_eq!(
        label("WebFetch", r#"{"url":"https://example.com/a/b"}"#),
        vec![RunEvent::Step("Reading example.com".to_string())]
    );
    assert_eq!(
        label("Grep", r#"{"pattern":"foo"}"#),
        vec![RunEvent::Step("Searching \"foo\"".to_string())]
    );
    assert_eq!(
        label("Bash", r#"{"command":"ls"}"#),
        vec![RunEvent::Step("Running ls".to_string())]
    );
    assert_eq!(
        label("Mystery", "{}"),
        vec![RunEvent::Step("Mystery".to_string())]
    );
    assert_eq!(
        parse_line(
            OutputFormat::ClaudeStreamJson,
            r#"{"type":"result","subtype":"error_max_turns","is_error":true}"#
        ),
        vec![RunEvent::Failed("error_max_turns".to_string())]
    );
    assert!(parse_line(OutputFormat::ClaudeStreamJson, "not json").is_empty());
}

#[test]
fn text_format_accumulates() {
    let mut parser = OutputParser::new(OutputFormat::Text);
    assert_eq!(
        parser.feed("hello"),
        vec![RunEvent::Step("hello".to_string())]
    );
    assert!(parser.feed("").is_empty());
    parser.feed("world");
    assert_eq!(
        parser.finish(),
        vec![RunEvent::Final("hello\n\nworld".to_string())]
    );
}
