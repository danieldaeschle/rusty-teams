use std::rc::Rc;

use agents::{Environment, OutputFormat};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, h_flex, input::Textarea, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::settings_controls::{
    Choice, MenuEntry, PAGE_PADDING, boxed, dropdown, field, hint, raised_button, text_field,
};
use super::settings_problems::{agent_problem, command_problem, placeholder_problem};
use super::settings_section::{AgentsSettings, Probe};
use crate::notify::switch;
use crate::theme;
use crate::views::widgets::icon;

const PLACEHOLDER_CHIPS: [&str; 9] = [
    "message",
    "sender",
    "chat",
    "thread",
    "extra",
    "conversation_id",
    "message_id",
    "session_id",
    "agent",
];

fn environment_choices() -> [(&'static str, Environment); 3] {
    [
        ("Windows", Environment::Host),
        ("WSL - default distro", Environment::Wsl { distro: None }),
        (
            "WSL - named distro",
            Environment::Wsl {
                distro: Some(String::new()),
            },
        ),
    ]
}

fn environment_kind(environment: &Environment) -> u8 {
    match environment {
        Environment::Host => 0,
        Environment::Wsl { distro: None } => 1,
        Environment::Wsl { distro: Some(_) } => 2,
    }
}

fn same_environment_kind(left: &Environment, right: &Environment) -> bool {
    environment_kind(left) == environment_kind(right)
}

fn output_choices() -> [(&'static str, OutputFormat); 3] {
    [
        (
            "Claude stream-json - live steps",
            OutputFormat::ClaudeStreamJson,
        ),
        ("Copilot JSON - live steps", OutputFormat::CopilotJsonl),
        ("Plain text - time and last line", OutputFormat::Text),
    ]
}

fn back_row(title: String, cx: &mut Context<AgentsSettings>) -> Stateful<Div> {
    h_flex()
        .id("agents-back")
        .w_full()
        .px(px(PAGE_PADDING))
        .pt(px(12.))
        .pb(px(8.))
        .gap(px(6.))
        .items_center()
        .cursor_pointer()
        .text_size(px(15.))
        .font_weight(FontWeight::BOLD)
        .child(icon(IconName::ArrowLeft, 16., theme::text()))
        .child(div().truncate().child(title))
        .on_click(cx.listener(|this, _, _, cx| this.go_back(cx)))
}

fn form(children: impl IntoIterator<Item = AnyElement>) -> Div {
    v_flex()
        .w_full()
        .px(px(PAGE_PADDING))
        .pb(px(16.))
        .gap(px(12.))
        .children(children)
}

impl AgentsSettings {
    pub(super) fn agent_page(&mut self, _: &mut Window, cx: &mut Context<Self>) -> Div {
        let (Some(form_state), Some(profile)) = (&self.agent_form, self.current_agent(cx)) else {
            return div();
        };
        let problems = self.runs.read(cx).settings().validate();
        let this = cx.entity();
        let environment_entries: Vec<MenuEntry> = environment_choices()
            .into_iter()
            .map(|(label, environment)| {
                let this = this.clone();
                let selected = same_environment_kind(&environment, &profile.environment);
                let choose: Choice = Rc::new(move |window, cx| {
                    let environment = environment.clone();
                    this.update(cx, |this, cx| this.set_environment(environment, window, cx))
                });
                MenuEntry {
                    label: label.to_owned(),
                    selected,
                    choose,
                }
            })
            .collect();
        let environment_label = environment_choices()
            .into_iter()
            .find(|(_, environment)| same_environment_kind(environment, &profile.environment))
            .map_or("Windows", |(label, _)| label);
        let output_entries: Vec<MenuEntry> = output_choices()
            .into_iter()
            .map(|(label, output)| {
                let this = this.clone();
                let choose: Choice =
                    Rc::new(move |_, cx| this.update(cx, |this, cx| this.set_output(output, cx)));
                MenuEntry {
                    label: label.to_owned(),
                    selected: output == profile.output,
                    choose,
                }
            })
            .collect();
        let output_label = output_choices()
            .into_iter()
            .find(|(_, output)| *output == profile.output)
            .map_or("", |(label, _)| label);
        let command_problem = command_problem(&problems, &profile.id, &profile.command);
        let open_problem =
            placeholder_problem(&problems, &profile.id, &profile.open_session_command);
        let named_distro = matches!(profile.environment, Environment::Wsl { distro: Some(_) });
        let status = match &self.probe {
            Probe::Idle => None,
            Probe::Running => Some(("Testing...".to_owned(), theme::text_muted())),
            Probe::Found(line) => Some((format!("Found: {line}"), theme::green())),
            Probe::Missing(error) => Some((error.clone(), theme::red_soft())),
        };
        let test_this = this.clone();
        let delete_this = this.clone();
        v_flex()
            .w_full()
            .child(back_row(profile.name.clone(), cx))
            .child(form([
                field(
                    "Name",
                    text_field(&form_state.name, false, false, cx),
                    None,
                    None,
                )
                .into_any_element(),
                field(
                    "Runs in",
                    dropdown(
                        "agent-environment",
                        environment_label.to_owned(),
                        environment_entries,
                    ),
                    Some(hint(
                        "Windows or a WSL distro. Paths below are in that system.",
                    )),
                    None,
                )
                .into_any_element(),
                div()
                    .w_full()
                    .when(named_distro, |slot| {
                        slot.child(field(
                            "Distro name",
                            text_field(&form_state.distro, false, false, cx),
                            None,
                            None,
                        ))
                    })
                    .into_any_element(),
                field(
                    "Command",
                    text_field(&form_state.command, true, command_problem.is_some(), cx),
                    Some(hint(
                        "The prompt goes to stdin unless the command has {prompt}.",
                    )),
                    command_problem,
                )
                .into_any_element(),
                field(
                    "Output",
                    dropdown("agent-output", output_label.to_owned(), output_entries),
                    Some(hint("Plain text shows only the time and the last line.")),
                    None,
                )
                .into_any_element(),
                field(
                    "MCP config flag",
                    text_field(&form_state.mcp_flag, true, false, cx),
                    Some(hint(
                        "Passes the Teams MCP endpoint to the agent. Empty = none.",
                    )),
                    None,
                )
                .into_any_element(),
                field(
                    "Open session",
                    text_field(&form_state.open_session, true, open_problem.is_some(), cx),
                    Some(hint("Available: {cwd} and {session_id}.")),
                    open_problem,
                )
                .into_any_element(),
                h_flex()
                    .w_full()
                    .gap(px(8.))
                    .items_center()
                    .child(
                        raised_button("agent-test", "Test").on_click(move |_, _, cx| {
                            test_this.update(cx, |this, cx| this.test_agent(cx))
                        }),
                    )
                    .children(status.map(|(text, color)| {
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(px(12.))
                            .text_color(color)
                            .child(text)
                    }))
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("agent-delete")
                            .cursor_pointer()
                            .text_size(px(13.))
                            .text_color(theme::red_soft())
                            .child("Delete")
                            .on_click(move |_, _, cx| {
                                delete_this.update(cx, |this, cx| this.delete_agent(cx))
                            }),
                    )
                    .into_any_element(),
            ]))
    }

    pub(super) fn action_page(&mut self, _: &mut Window, cx: &mut Context<Self>) -> Div {
        let Some(form_state) = &self.action_form else {
            return div();
        };
        let settings = self.runs.read(cx).settings().clone();
        let Some(action) = settings
            .actions
            .iter()
            .find(|action| action.id == form_state.id)
            .cloned()
        else {
            return div();
        };
        let problems = settings.validate();
        let this = cx.entity();
        let agent_entries: Vec<MenuEntry> = settings
            .agents
            .iter()
            .map(|agent| {
                let (this, agent_id) = (this.clone(), agent.id.clone());
                let selected = agent.id == action.agent_id;
                let choose: Choice = Rc::new(move |_, cx| {
                    let agent_id = agent_id.clone();
                    this.update(cx, |this, cx| this.set_action_agent(agent_id, cx))
                });
                MenuEntry {
                    label: agent.name.clone(),
                    selected,
                    choose,
                }
            })
            .collect();
        let agent_label = settings
            .agents
            .iter()
            .find(|agent| agent.id == action.agent_id)
            .map_or("Choose an agent".to_owned(), |agent| agent.name.clone());
        let agent_problem = agent_problem(&problems, &action.id);
        let prompt_problem = placeholder_problem(&problems, &action.id, &action.prompt_template);
        let args_problem = placeholder_problem(&problems, &action.id, &action.extra_args);
        let chips =
            h_flex()
                .w_full()
                .flex_wrap()
                .gap(px(4.))
                .children(PLACEHOLDER_CHIPS.iter().map(|placeholder| {
                    let this = this.clone();
                    div()
                        .id(*placeholder)
                        .px(px(6.))
                        .py(px(2.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .bg(theme::row_hover())
                        .hover(|chip| chip.bg(theme::surface_raised()))
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_size(px(11.))
                        .text_color(theme::accent_text())
                        .child(format!("{{{placeholder}}}"))
                        .on_click(move |_, window, cx| {
                            this.update(cx, |this, cx| {
                                this.insert_placeholder(placeholder, window, cx)
                            })
                        })
                }));
        let ask = action.ask_extra_instructions;
        let (ask_this, delete_this) = (this.clone(), this.clone());
        v_flex()
            .w_full()
            .child(back_row(action.label.clone(), cx))
            .child(form([
                h_flex()
                    .w_full()
                    .gap(px(8.))
                    .items_start()
                    .child(
                        field(
                            "Label",
                            text_field(&form_state.label, false, false, cx),
                            None,
                            None,
                        )
                        .flex_1()
                        .min_w(px(0.)),
                    )
                    .child(
                        field(
                            "Agent",
                            dropdown("action-agent", agent_label, agent_entries),
                            None,
                            agent_problem,
                        )
                        .flex_1()
                        .min_w(px(0.)),
                    )
                    .into_any_element(),
                field(
                    "Working dir",
                    text_field(&form_state.working_dir, true, false, cx),
                    Some(hint("A path inside the system the agent runs in.")),
                    None,
                )
                .into_any_element(),
                field(
                    "Prompt",
                    boxed(
                        Textarea::new(&form_state.prompt)
                            .appearance(false)
                            .bordered(false),
                        prompt_problem.is_some(),
                        cx,
                    ),
                    Some(chips.into_any_element()),
                    prompt_problem,
                )
                .into_any_element(),
                h_flex()
                    .id("action-ask-extra")
                    .w_full()
                    .gap(px(12.))
                    .items_center()
                    .cursor_pointer()
                    .child(
                        v_flex()
                            .flex_1()
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Ask for extra instructions"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme::text_muted())
                                    .child("Dialog before each run"),
                            ),
                    )
                    .child(switch(ask))
                    .on_click(move |_, _, cx| {
                        ask_this.update(cx, |this, cx| this.set_ask_extra(!ask, cx))
                    })
                    .into_any_element(),
                field(
                    "Extra args",
                    text_field(&form_state.extra_args, true, args_problem.is_some(), cx),
                    Some(hint("Added after the agent command.")),
                    args_problem,
                )
                .into_any_element(),
                h_flex()
                    .w_full()
                    .justify_end()
                    .child(
                        div()
                            .id("action-delete")
                            .cursor_pointer()
                            .text_size(px(13.))
                            .text_color(theme::red_soft())
                            .child("Delete")
                            .on_click(move |_, _, cx| {
                                delete_this.update(cx, |this, cx| this.delete_action(cx))
                            }),
                    )
                    .into_any_element(),
            ]))
    }
}

pub(super) fn resume_toggle(resume: bool, settings: Entity<AgentsSettings>) -> Stateful<Div> {
    h_flex()
        .id("resume-interrupted-runs")
        .w_full()
        .px(px(PAGE_PADDING))
        .pt(px(16.))
        .gap(px(12.))
        .items_center()
        .cursor_pointer()
        .child(
            v_flex()
                .flex_1()
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Resume interrupted runs"),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::text_muted())
                        .child("Continue running agents after the app restarts"),
                ),
        )
        .child(switch(resume))
        .on_click(move |_, _, cx| {
            settings.update(cx, |this, cx| this.set_resume_interrupted_runs(!resume, cx))
        })
}

#[cfg(test)]
mod tests {
    use agents::Environment;

    use super::same_environment_kind;

    #[test]
    fn environments_compare_by_kind_not_distro_name() {
        let named = |name: &str| Environment::Wsl {
            distro: Some(name.to_owned()),
        };
        assert!(same_environment_kind(&named("Ubuntu"), &named("Debian")));
        assert!(!same_environment_kind(
            &named("Ubuntu"),
            &Environment::Wsl { distro: None }
        ));
        assert!(!same_environment_kind(&Environment::Host, &named("Ubuntu")));
        assert!(same_environment_kind(
            &Environment::Host,
            &Environment::Host
        ));
    }
}
