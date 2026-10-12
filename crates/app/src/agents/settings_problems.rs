use agents::SettingsProblem;

pub fn placeholder_problem(
    problems: &[SettingsProblem],
    item_id: &str,
    text: &str,
) -> Option<String> {
    problems.iter().find_map(|problem| match problem {
        SettingsProblem::UnknownPlaceholder {
            item_id: problem_item,
            placeholder,
        } if problem_item == item_id && text.contains(&format!("{{{placeholder}}}")) => {
            Some(format!("Unknown placeholder {{{placeholder}}}"))
        }
        _ => None,
    })
}

pub fn command_problem(problems: &[SettingsProblem], agent_id: &str, text: &str) -> Option<String> {
    let empty = problems.iter().any(
        |problem| matches!(problem, SettingsProblem::EmptyCommand { agent_id: id } if id == agent_id),
    );
    if empty {
        return Some("The command is empty".to_owned());
    }
    placeholder_problem(problems, agent_id, text)
}

pub fn agent_problem(problems: &[SettingsProblem], action_id: &str) -> Option<String> {
    problems.iter().find_map(|problem| match problem {
        SettingsProblem::UnknownAgent {
            action_id: problem_action,
            ..
        } if problem_action == action_id => Some("This agent no longer exists".to_owned()),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use agents::{AgentSettings, SettingsProblem};

    use super::{agent_problem, command_problem, placeholder_problem};

    fn broken() -> Vec<SettingsProblem> {
        let mut settings = AgentSettings::default();
        settings.agents[0].command = " {bogus}".to_owned();
        settings.agents[1].command.clear();
        settings.agents[0].open_session_command = "wt.exe {session_id}".to_owned();
        settings.actions[0].agent_id = "ghost".to_owned();
        settings.actions[1].extra_args = "--flag {nope}".to_owned();
        settings.validate()
    }

    #[test]
    fn unknown_placeholders_show_under_the_field_that_contains_them() {
        let problems = broken();
        assert_eq!(
            command_problem(&problems, "claude", " {bogus}").as_deref(),
            Some("Unknown placeholder {bogus}")
        );
        assert_eq!(
            placeholder_problem(&problems, "claude", "wt.exe {session_id}"),
            None
        );
        assert_eq!(
            placeholder_problem(&problems, "build-this", "--flag {nope}").as_deref(),
            Some("Unknown placeholder {nope}")
        );
        assert_eq!(placeholder_problem(&problems, "build-this", "--flag"), None);
    }

    #[test]
    fn an_empty_command_and_a_missing_agent_are_reported() {
        let problems = broken();
        assert_eq!(
            command_problem(&problems, "copilot", "").as_deref(),
            Some("The command is empty")
        );
        assert_eq!(
            agent_problem(&problems, "answer-this").as_deref(),
            Some("This agent no longer exists")
        );
        assert_eq!(agent_problem(&problems, "build-this"), None);
    }

    #[test]
    fn valid_settings_show_nothing() {
        let problems = AgentSettings::default().validate();
        assert_eq!(command_problem(&problems, "claude", "claude -p"), None);
        assert_eq!(agent_problem(&problems, "answer-this"), None);
    }
}
