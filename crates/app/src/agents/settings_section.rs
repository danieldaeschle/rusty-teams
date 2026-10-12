use agents::{AgentAction, AgentProfile, Environment, OutputFormat};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{InputEvent, InputState, TextareaState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::environment_label;
use super::runs::AgentRuns;
use super::settings_controls::{Choice, MenuEntry, PAGE_PADDING, link_dropdown, section_header};
use crate::runtime;
use crate::theme;
use crate::views::widgets::icon;

const NOTE: &str = "Agents get the message, its sender and up to 20 earlier messages. Nothing is sent to Teams without you.";
const DEFAULT_DISTRO: &str = "Ubuntu";
const PROMPT_MIN_ROWS: usize = 4;
const PROMPT_MAX_ROWS: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Page {
    List,
    Agent,
    Action,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) enum Probe {
    Idle,
    Running,
    Found(String),
    Missing(String),
}

pub(super) struct AgentForm {
    pub id: String,
    pub name: Entity<InputState>,
    pub distro: Entity<InputState>,
    pub command: Entity<InputState>,
    pub open_session: Entity<InputState>,
    pub mcp_flag: Entity<InputState>,
}

pub(super) struct ActionForm {
    pub id: String,
    pub label: Entity<InputState>,
    pub working_dir: Entity<InputState>,
    pub prompt: Entity<TextareaState>,
    pub extra_args: Entity<InputState>,
}

pub struct AgentsSettings {
    pub(super) runs: Entity<AgentRuns>,
    pub(super) page: Page,
    pub(super) agent_form: Option<AgentForm>,
    pub(super) action_form: Option<ActionForm>,
    pub(super) probe: Probe,
    form_subscriptions: Vec<Subscription>,
    _runs_subscription: Subscription,
}

pub fn unique_id(base: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(base) {
        return base.to_owned();
    }
    (2..)
        .map(|suffix| format!("{base}-{suffix}"))
        .find(|candidate| !taken(candidate))
        .unwrap_or_else(|| base.to_owned())
}

pub fn move_item<T>(items: &mut [T], index: usize, up: bool) -> bool {
    let target = if up {
        index.checked_sub(1)
    } else {
        Some(index + 1).filter(|target| *target < items.len())
    };
    match target {
        Some(target) if index < items.len() => {
            items.swap(index, target);
            true
        }
        _ => false,
    }
}

impl AgentsSettings {
    pub fn new(runs: Entity<AgentRuns>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&runs, |_, _, cx| cx.notify());
        AgentsSettings {
            runs,
            page: Page::List,
            agent_form: None,
            action_form: None,
            probe: Probe::Idle,
            form_subscriptions: Vec::new(),
            _runs_subscription: subscription,
        }
    }

    pub(super) fn go_back(&mut self, cx: &mut Context<Self>) {
        self.page = Page::List;
        self.agent_form = None;
        self.action_form = None;
        self.form_subscriptions.clear();
        self.probe = Probe::Idle;
        cx.notify();
    }

    fn line_input(
        &mut self,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
        on_change: fn(&mut Self, &mut Context<Self>),
    ) -> Entity<InputState> {
        let value = value.to_owned();
        let input = cx.new(|cx| InputState::new(window, cx).default_value(value));
        self.form_subscriptions.push(cx.subscribe(
            &input,
            move |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    on_change(this, cx);
                }
            },
        ));
        input
    }

    fn open_agent(&mut self, agent_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(profile) = self
            .runs
            .read(cx)
            .settings()
            .agents
            .iter()
            .find(|profile| profile.id == agent_id)
            .cloned()
        else {
            return;
        };
        self.go_back(cx);
        let distro = match &profile.environment {
            Environment::Wsl {
                distro: Some(distro),
            } => distro.clone(),
            _ => String::new(),
        };
        self.agent_form = Some(AgentForm {
            id: profile.id.clone(),
            name: self.line_input(&profile.name, window, cx, Self::commit_agent),
            distro: self.line_input(&distro, window, cx, Self::commit_agent),
            command: self.line_input(&profile.command, window, cx, Self::commit_agent),
            open_session: self.line_input(
                &profile.open_session_command,
                window,
                cx,
                Self::commit_agent,
            ),
            mcp_flag: self.line_input(&profile.mcp_config_flag, window, cx, Self::commit_agent),
        });
        self.page = Page::Agent;
        cx.notify();
    }

    fn open_action(&mut self, action_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(action) = self
            .runs
            .read(cx)
            .settings()
            .actions
            .iter()
            .find(|action| action.id == action_id)
            .cloned()
        else {
            return;
        };
        self.go_back(cx);
        let prompt = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(PROMPT_MIN_ROWS, PROMPT_MAX_ROWS)
                .default_value(action.prompt_template.clone())
        });
        self.form_subscriptions
            .push(cx.subscribe(&prompt, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.commit_action(cx);
                }
            }));
        self.action_form = Some(ActionForm {
            id: action.id.clone(),
            label: self.line_input(&action.label, window, cx, Self::commit_action),
            working_dir: self.line_input(&action.working_dir, window, cx, Self::commit_action),
            prompt,
            extra_args: self.line_input(&action.extra_args, window, cx, Self::commit_action),
        });
        self.page = Page::Action;
        cx.notify();
    }

    pub(super) fn commit_agent(&mut self, cx: &mut Context<Self>) {
        let Some(form) = &self.agent_form else {
            return;
        };
        let id = form.id.clone();
        let name = form.name.read(cx).value().to_string();
        let distro = form.distro.read(cx).value().trim().to_owned();
        let command = form.command.read(cx).value().to_string();
        let open_session = form.open_session.read(cx).value().to_string();
        let mcp_flag = form.mcp_flag.read(cx).value().trim().to_owned();
        self.runs.update(cx, |runs, cx| {
            runs.update_settings(
                |settings| {
                    let Some(agent) = settings.agents.iter_mut().find(|agent| agent.id == id)
                    else {
                        return;
                    };
                    agent.name = name;
                    agent.command = command;
                    agent.open_session_command = open_session;
                    agent.mcp_config_flag = mcp_flag;
                    if let Environment::Wsl {
                        distro: Some(current),
                    } = &mut agent.environment
                    {
                        *current = distro;
                    }
                },
                cx,
            )
        });
        self.probe = Probe::Idle;
    }

    pub(super) fn commit_action(&mut self, cx: &mut Context<Self>) {
        let Some(form) = &self.action_form else {
            return;
        };
        let id = form.id.clone();
        let label = form.label.read(cx).value().to_string();
        let working_dir = form.working_dir.read(cx).value().trim().to_owned();
        let prompt = form.prompt.read(cx).value().to_string();
        let extra_args = form.extra_args.read(cx).value().to_string();
        self.runs.update(cx, |runs, cx| {
            runs.update_settings(
                |settings| {
                    let Some(action) = settings.actions.iter_mut().find(|action| action.id == id)
                    else {
                        return;
                    };
                    action.label = label;
                    action.working_dir = working_dir;
                    action.prompt_template = prompt;
                    action.extra_args = extra_args;
                },
                cx,
            )
        });
    }

    pub(super) fn set_environment(
        &mut self,
        environment: Environment,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(form) = &self.agent_form else {
            return;
        };
        let id = form.id.clone();
        if matches!(environment, Environment::Wsl { distro: Some(_) })
            && form.distro.read(cx).value().trim().is_empty()
        {
            form.distro
                .update(cx, |input, cx| input.set_value(DEFAULT_DISTRO, window, cx));
        }
        let distro = form.distro.read(cx).value().trim().to_owned();
        let environment = match environment {
            Environment::Wsl { distro: Some(_) } => Environment::Wsl {
                distro: Some(distro),
            },
            other => other,
        };
        self.runs.update(cx, |runs, cx| {
            runs.update_settings(
                |settings| {
                    if let Some(agent) = settings.agents.iter_mut().find(|agent| agent.id == id) {
                        agent.environment = environment;
                    }
                },
                cx,
            )
        });
        self.probe = Probe::Idle;
    }

    pub(super) fn set_output(&mut self, output: OutputFormat, cx: &mut Context<Self>) {
        let Some(id) = self.agent_form.as_ref().map(|form| form.id.clone()) else {
            return;
        };
        self.runs.update(cx, |runs, cx| {
            runs.update_settings(
                |settings| {
                    if let Some(agent) = settings.agents.iter_mut().find(|agent| agent.id == id) {
                        agent.output = output;
                    }
                },
                cx,
            )
        });
    }

    pub(super) fn set_action_agent(&mut self, agent_id: String, cx: &mut Context<Self>) {
        self.update_action(cx, move |action| action.agent_id = agent_id);
    }

    pub(super) fn set_ask_extra(&mut self, ask: bool, cx: &mut Context<Self>) {
        self.update_action(cx, move |action| action.ask_extra_instructions = ask);
    }

    fn update_action(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut AgentAction)) {
        let Some(id) = self.action_form.as_ref().map(|form| form.id.clone()) else {
            return;
        };
        self.runs.update(cx, |runs, cx| {
            runs.update_settings(
                |settings| {
                    if let Some(action) = settings.actions.iter_mut().find(|action| action.id == id)
                    {
                        change(action);
                    }
                },
                cx,
            )
        });
    }

    pub(super) fn insert_placeholder(
        &mut self,
        placeholder: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(prompt) = self.action_form.as_ref().map(|form| form.prompt.clone()) else {
            return;
        };
        let text = format!("{{{placeholder}}}");
        prompt.update(cx, |prompt, cx| {
            prompt.insert(text, window, cx);
            prompt.focus(window, cx);
        });
        self.commit_action(cx);
    }

    pub(super) fn delete_agent(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.agent_form.as_ref().map(|form| form.id.clone()) else {
            return;
        };
        self.go_back(cx);
        self.runs.update(cx, |runs, cx| {
            runs.update_settings(
                |settings| settings.agents.retain(|agent| agent.id != id),
                cx,
            )
        });
    }

    pub(super) fn delete_action(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.action_form.as_ref().map(|form| form.id.clone()) else {
            return;
        };
        self.go_back(cx);
        self.runs.update(cx, |runs, cx| {
            runs.update_settings(
                |settings| settings.actions.retain(|action| action.id != id),
                cx,
            )
        });
    }

    pub(super) fn test_agent(&mut self, cx: &mut Context<Self>) {
        let Some(profile) = self.current_agent(cx) else {
            return;
        };
        if profile.command.trim().is_empty() {
            self.probe = Probe::Missing("The command is empty".to_owned());
            cx.notify();
            return;
        }
        self.probe = Probe::Running;
        cx.notify();
        let invocation = agents::version_check_invocation(&profile);
        let receiver = runtime::spawn(async move { agents::probe(&invocation).await });
        cx.spawn(async move |this, cx| {
            let result = receiver
                .await
                .unwrap_or_else(|_| Err("cancelled".to_owned()));
            this.update(cx, |this, cx| {
                this.probe = match result {
                    Ok(line) => Probe::Found(line),
                    Err(error) => Probe::Missing(error),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn current_agent(&self, cx: &App) -> Option<AgentProfile> {
        let id = &self.agent_form.as_ref()?.id;
        self.runs
            .read(cx)
            .settings()
            .agents
            .iter()
            .find(|agent| &agent.id == id)
            .cloned()
    }

    fn add_agent(&mut self, preset: AgentProfile, window: &mut Window, cx: &mut Context<Self>) {
        let mut profile = preset;
        let existing: Vec<String> = self
            .runs
            .read(cx)
            .settings()
            .agents
            .iter()
            .map(|agent| agent.id.clone())
            .collect();
        profile.id = unique_id(&profile.id, |candidate| {
            existing.iter().any(|id| id == candidate)
        });
        let id = profile.id.clone();
        self.runs.update(cx, |runs, cx| {
            runs.update_settings(|settings| settings.agents.push(profile), cx)
        });
        self.open_agent(&id, window, cx);
    }

    fn add_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let settings = self.runs.read(cx).settings();
        let agent_id = settings
            .agents
            .first()
            .map(|agent| agent.id.clone())
            .unwrap_or_default();
        let mut action = AgentAction::answer_this(&agent_id);
        action.label = "New action".to_owned();
        action.id = unique_id("action", |candidate| {
            settings.actions.iter().any(|action| action.id == candidate)
        });
        let id = action.id.clone();
        self.runs.update(cx, |runs, cx| {
            runs.update_settings(|settings| settings.actions.push(action), cx)
        });
        self.open_action(&id, window, cx);
    }

    fn move_action(&mut self, index: usize, up: bool, cx: &mut Context<Self>) {
        self.runs.update(cx, |runs, cx| {
            runs.update_settings(
                |settings| {
                    move_item(&mut settings.actions, index, up);
                },
                cx,
            )
        });
    }
}

type Preset = (&'static str, fn() -> AgentProfile);

fn agent_presets() -> [Preset; 4] {
    fn custom() -> AgentProfile {
        AgentProfile {
            id: "custom".to_owned(),
            name: "Custom agent".to_owned(),
            environment: Environment::Host,
            command: String::new(),
            output: OutputFormat::Text,
            open_session_command: String::new(),
            mcp_config_flag: String::new(),
        }
    }
    [
        ("Claude Code (WSL)", AgentProfile::claude_wsl),
        ("Claude Code (Windows)", AgentProfile::claude_host),
        ("Copilot CLI (WSL)", AgentProfile::copilot_wsl),
        ("Custom", custom),
    ]
}

fn list_row(
    id: (&'static str, usize),
    leading: Option<AnyElement>,
    title: String,
    detail: String,
    mono: Option<String>,
) -> Stateful<Div> {
    h_flex()
        .id(id)
        .w_full()
        .px(px(12.))
        .py(px(8.))
        .gap(px(12.))
        .items_center()
        .rounded(px(8.))
        .cursor_pointer()
        .hover(|row| row.bg(theme::row_hover()))
        .children(leading)
        .child(
            v_flex()
                .flex_1()
                .min_w(px(0.))
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(12.))
                        .text_color(theme::text_muted())
                        .child(detail),
                )
                .children(mono.map(|command| {
                    div()
                        .truncate()
                        .text_size(px(11.))
                        .text_color(theme::text_faint())
                        .child(command)
                })),
        )
}

fn small_button(id: (&'static str, usize), name: IconName, enabled: bool) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(28.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(7.))
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(|button| button.bg(theme::surface_raised()))
        })
        .when(!enabled, |button| button.opacity(0.4))
        .child(icon(name, 14., theme::text_muted()))
}

impl AgentsSettings {
    fn list_page(&mut self, cx: &mut Context<Self>) -> Div {
        let settings = self.runs.read(cx).settings().clone();
        let this = cx.entity();
        let preset_entries: Vec<MenuEntry> = agent_presets()
            .into_iter()
            .map(|(label, build)| {
                let this = this.clone();
                let choose: Choice = std::rc::Rc::new(move |window, cx| {
                    this.update(cx, |this, cx| this.add_agent(build(), window, cx))
                });
                MenuEntry {
                    label: label.to_owned(),
                    selected: false,
                    choose,
                }
            })
            .collect();
        let add_action = {
            let this = this.clone();
            Button::new("add-action")
                .link()
                .label("+ Add action")
                .on_click(move |_, window, cx| {
                    this.update(cx, |this, cx| this.add_action(window, cx))
                })
        };
        let action_count = settings.actions.len();
        let agent_rows = settings.agents.iter().enumerate().map(|(index, agent)| {
            let (open, id) = (cx.entity(), agent.id.clone());
            list_row(
                ("agent-row", index),
                Some(
                    div()
                        .size(px(28.))
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .rounded(px(7.))
                        .bg(theme::surface_raised())
                        .child(icon(IconName::Bot, 14., theme::accent_text()))
                        .into_any_element(),
                ),
                agent.name.clone(),
                environment_label(&agent.environment),
                Some(agent.command.clone()),
            )
            .on_click(move |_, window, cx| {
                open.update(cx, |this, cx| this.open_agent(&id, window, cx))
            })
        });
        let action_rows = settings.actions.iter().enumerate().map(|(index, action)| {
            let (open, id) = (cx.entity(), action.id.clone());
            let (up, down) = (cx.entity(), cx.entity());
            let agent_name = settings
                .agents
                .iter()
                .find(|agent| agent.id == action.agent_id)
                .map_or("Unknown agent", |agent| agent.name.as_str());
            list_row(
                ("action-row", index),
                None,
                action.label.clone(),
                format!("{agent_name} - {}", action.working_dir),
                None,
            )
            .on_click(move |_, window, cx| {
                open.update(cx, |this, cx| this.open_action(&id, window, cx))
            })
            .child(
                h_flex()
                    .flex_none()
                    .child(
                        small_button(("action-up", index), IconName::ArrowUp, index > 0).on_click(
                            move |_, _, cx| {
                                cx.stop_propagation();
                                up.update(cx, |this, cx| this.move_action(index, true, cx))
                            },
                        ),
                    )
                    .child(
                        small_button(
                            ("action-down", index),
                            IconName::ArrowDown,
                            index + 1 < action_count,
                        )
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            down.update(cx, |this, cx| this.move_action(index, false, cx))
                        }),
                    ),
            )
        });
        v_flex()
            .w_full()
            .child(section_header(
                "AGENTS",
                Some(link_dropdown("add-agent", "+ Add agent", preset_entries).into_any_element()),
            ))
            .child(v_flex().w_full().px(px(8.)).children(agent_rows))
            .child(section_header(
                "ACTIONS - IN THIS ORDER IN THE MENU",
                Some(add_action.into_any_element()),
            ))
            .child(v_flex().w_full().px(px(8.)).children(action_rows))
            .child(
                div()
                    .w_full()
                    .px(px(PAGE_PADDING))
                    .pt(px(16.))
                    .pb(px(16.))
                    .text_size(px(12.))
                    .line_height(relative(1.5))
                    .text_color(theme::text_faint())
                    .child(NOTE),
            )
    }
}

impl Render for AgentsSettings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.page {
            Page::List => self.list_page(cx).into_any_element(),
            Page::Agent => self.agent_page(window, cx).into_any_element(),
            Page::Action => self.action_page(window, cx).into_any_element(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{move_item, unique_id};

    #[test]
    fn ids_get_a_numeric_suffix_when_taken() {
        let taken = ["claude", "claude-2"];
        let is_taken = |candidate: &str| taken.contains(&candidate);
        assert_eq!(unique_id("copilot", is_taken), "copilot");
        assert_eq!(unique_id("claude", is_taken), "claude-3");
    }

    #[test]
    fn moving_swaps_neighbours_and_stops_at_the_ends() {
        let mut items = vec!["a", "b", "c"];
        assert!(move_item(&mut items, 1, true));
        assert_eq!(items, ["b", "a", "c"]);
        assert!(move_item(&mut items, 1, false));
        assert_eq!(items, ["b", "c", "a"]);
        assert!(!move_item(&mut items, 0, true));
        assert!(!move_item(&mut items, 2, false));
        assert_eq!(items, ["b", "c", "a"]);
    }
}
