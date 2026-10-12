use super::{RunEvent, truncate};

const STEP_LIMIT: usize = 80;

pub(super) fn parse_line(line: &str, all_output: &mut String) -> Vec<RunEvent> {
    all_output.push_str(line);
    all_output.push('\n');
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    vec![RunEvent::Step(truncate(trimmed, STEP_LIMIT))]
}

pub(super) fn finish(all_output: &str) -> Vec<RunEvent> {
    vec![RunEvent::Final(all_output.trim().to_string())]
}
