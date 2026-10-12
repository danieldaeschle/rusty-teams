mod claude;
mod copilot;
mod text;

use crate::profile::OutputFormat;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunEvent {
    Step(String),
    Text(String),
    Final(String),
    SessionId(String),
    Failed(String),
}

#[derive(Debug)]
pub struct OutputParser {
    format: OutputFormat,
    last_text: Option<String>,
    all_output: String,
}

impl OutputParser {
    pub fn new(format: OutputFormat) -> Self {
        Self {
            format,
            last_text: None,
            all_output: String::new(),
        }
    }

    pub fn feed(&mut self, line: &str) -> Vec<RunEvent> {
        match self.format {
            OutputFormat::ClaudeStreamJson => claude::parse_line(line),
            OutputFormat::CopilotJsonl => copilot::parse_line(line, &mut self.last_text),
            OutputFormat::Text => text::parse_line(line, &mut self.all_output),
        }
    }

    pub fn finish(&mut self) -> Vec<RunEvent> {
        match self.format {
            OutputFormat::Text => text::finish(&self.all_output),
            OutputFormat::ClaudeStreamJson | OutputFormat::CopilotJsonl => Vec::new(),
        }
    }
}

pub fn parse_line(format: OutputFormat, line: &str) -> Vec<RunEvent> {
    OutputParser::new(format).feed(line)
}

pub(crate) fn truncate(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        Some((byte_index, _)) => format!("{}...", &text[..byte_index]),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_respects_char_boundaries() {
        assert_eq!(truncate("abc", 3), "abc");
        assert_eq!(truncate("abcd", 3), "abc...");
        assert_eq!(truncate("äöüß", 2), "äö...");
    }
}
