use agents::{AgentAction, AgentBackend, AgentProfile, Environment, OutputFormat};
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
use super::settings_controls::{
    Choice, MenuEntry, PAGE_PADDING, dropdown, link_dropdown, section_header,
};
use super::settings_forms::resume_toggle;
use crate::i18n::{tr, tr_args};
use crate::runtime;
use crate::theme;
use crate::views::widgets::icon;

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
    pub base_url: Entity<InputState>,
    pub model: Entity<InputState>,
    pub api_key: Entity<InputState>,
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
    pub(super) models: Vec<String>,
    models_request: u64,
    pub(super) found: Vec<agents::CliHit>,
    scanned: bool,
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
            models: Vec::new(),
            models_request: 0,
            found: Vec::new(),
            scanned: false,
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
        self.models.clear();
        self.models_request += 1;
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
        let (base_url, model) = match &profile.backend {
            AgentBackend::OpenAi {
                base_url, model, ..
            } => (base_url.clone(), model.clone()),
            AgentBackend::Anthropic { model, .. } => (String::new(), model.clone()),
            AgentBackend::Command => (String::new(), String::new()),
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
            base_url: self.line_input(&base_url, window, cx, Self::commit_agent),
            model: self.line_input(&model, window, cx, Self::commit_agent),
            api_key: self.line_input("", window, cx, Self::commit_agent),
        });
        let inputs = self
            .agent_form
            .as_ref()
            .map(|form| (form.base_url.clone(), form.api_key.clone()));
        if let Some((base_url, api_key)) = inputs {
            self.watch_model_refresh(&base_url, cx);
            self.watch_model_refresh(&api_key, cx);
        }
        self.page = Page::Agent;
        if profile.is_chat() {
            self.refresh_models(cx);
        }
        cx.notify();
    }

    fn watch_model_refresh(&mut self, input: &Entity<InputState>, cx: &mut Context<Self>) {
        self.form_subscriptions
            .push(cx.subscribe(input, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Blur) {
                    this.commit_agent(cx);
                    this.refresh_models(cx);
                }
            }));
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
        let base_url = form.base_url.read(cx).value().trim().to_owned();
        let model = form.model.read(cx).value().trim().to_owned();
        let typed_key = form.api_key.read(cx).value().trim().to_owned();
        self.runs.update(cx, |runs, cx| {
            runs.update_settings(
                |settings| {
                    let Some(agent) = settings.agents.iter_mut().find(|agent| agent.id == id)
                    else {
                        return;
                    };
                    agent.name = name;
                    if agent.is_chat() {
                        match &mut agent.backend {
                            AgentBackend::OpenAi {
                                base_url: url,
                                model: current,
                                api_key,
                            } => {
                                *url = base_url;
                                *current = model;
                                if !typed_key.is_empty() {
                                    *api_key = typed_key;
                                }
                            }
                            AgentBackend::Anthropic {
                                model: current,
                                api_key,
                            } => {
                                *current = model;
                                if !typed_key.is_empty() {
                                    *api_key = typed_key;
                                }
                            }
                            AgentBackend::Command => {}
                        }
                    } else {
                        agent.command = command;
                        agent.open_session_command = open_session;
                        agent.mcp_config_flag = mcp_flag;
                        if let Environment::Wsl {
                            distro: Some(current),
                        } = &mut agent.environment
                        {
                            *current = distro;
                        }
                    }
                },
                cx,
            )
        });
    }

    pub(super) fn set_model(&mut self, model: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &self.agent_form else {
            return;
        };
        form.model
            .update(cx, |input, cx| input.set_value(model, window, cx));
        self.commit_agent(cx);
    }

    pub(super) fn refresh_models(&mut self, cx: &mut Context<Self>) {
        let Some(profile) = self.current_agent(cx) else {
            return;
        };
        if !profile.is_chat() {
            return;
        }
        self.models_request += 1;
        let ticket = self.models_request;
        self.probe = Probe::Running;
        cx.notify();
        let receiver = runtime::spawn(async move { agents::list_models(&profile).await });
        cx.spawn(async move |this, cx| {
            let result = receiver
                .await
                .unwrap_or_else(|_| Err("cancelled".to_owned()));
            this.update(cx, |this, cx| {
                if this.models_request != ticket {
                    return;
                }
                match result {
                    Ok(names) => {
                        let count = names.len();
                        this.models = names;
                        this.probe = Probe::Found(if count == 1 {
                            "1 model".to_owned()
                        } else {
                            format!("{count} models")
                        });
                    }
                    Err(error) => {
                        this.models.clear();
                        this.probe = Probe::Missing(error);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
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

    pub(super) fn set_write_with(&mut self, agent_id: String, cx: &mut Context<Self>) {
        self.runs.update(cx, |runs, cx| {
            runs.update_settings(|settings| settings.write_with = agent_id, cx)
        });
    }

    pub(super) fn rescan(&mut self, cx: &mut Context<Self>) {
        self.found = agents::scan_clis();
        self.scanned = true;
        cx.notify();
    }

    pub(super) fn set_resume_interrupted_runs(&mut self, resume: bool, cx: &mut Context<Self>) {
        self.runs.update(cx, |runs, cx| {
            runs.update_settings(|settings| settings.resume_interrupted_runs = resume, cx)
        });
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
        if profile.is_chat() {
            self.refresh_models(cx);
            return;
        }
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
        let chat = profile.is_chat();
        let mut action = AgentAction::answer_this(&id);
        action.extra_args.clear();
        let mut args = std::collections::HashMap::new();
        args.insert("name".to_owned(), profile.name.clone());
        action.label = tr_args("agents.settings.answer_with", &args);
        action.id = unique_id(&format!("answer-{id}"), |candidate| {
            self.runs
                .read(cx)
                .settings()
                .actions
                .iter()
                .any(|action| action.id == candidate)
        });
        self.runs.update(cx, |runs, cx| {
            runs.update_settings(
                |settings| {
                    settings.agents.push(profile);
                    if chat {
                        settings.actions.push(action);
                    }
                },
                cx,
            )
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
        action.label = tr("agents.settings.new_action");
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

fn agent_presets() -> [Preset; 9] {
    fn custom() -> AgentProfile {
        AgentProfile {
            id: "custom".to_owned(),
            name: tr("agents.settings.custom_agent"),
            environment: Environment::Host,
            output: OutputFormat::Text,
            ..AgentProfile::default()
        }
    }
    [
        (
            "Claude Code (WSL)",
            AgentProfile::claude_wsl as fn() -> AgentProfile,
        ),
        ("Claude Code (Windows)", AgentProfile::claude_host),
        ("Copilot CLI (WSL)", AgentProfile::copilot_wsl),
        ("Ollama", AgentProfile::ollama),
        ("OpenAI", AgentProfile::openai),
        ("OpenRouter", AgentProfile::openrouter),
        ("Anthropic", AgentProfile::anthropic),
        ("OpenAI-compatible", AgentProfile::openai_compatible),
        ("Custom", custom),
    ]
}

fn write_with_label(settings: &agents::AgentSettings) -> String {
    if settings.write_with.is_empty() {
        return "Microsoft Copilot".to_owned();
    }
    settings
        .agents
        .iter()
        .find(|agent| agent.id == settings.write_with && agent.is_chat())
        .map(|agent| agent.name.clone())
        .unwrap_or_else(|| "Microsoft Copilot".to_owned())
}

fn write_with_entries(
    settings: &agents::AgentSettings,
    this: &Entity<AgentsSettings>,
) -> Vec<MenuEntry> {
    let mut entries = vec![write_with_entry(
        this,
        "Microsoft Copilot",
        "",
        settings.write_with.is_empty(),
    )];
    for agent in settings.agents.iter().filter(|agent| agent.is_chat()) {
        entries.push(write_with_entry(
            this,
            &agent.name,
            &agent.id,
            settings.write_with == agent.id,
        ));
    }
    entries
}

fn write_with_entry(
    this: &Entity<AgentsSettings>,
    label: &str,
    agent_id: &str,
    selected: bool,
) -> MenuEntry {
    let (this, agent_id) = (this.clone(), agent_id.to_owned());
    let choose: Choice = std::rc::Rc::new(move |_, cx| {
        let agent_id = agent_id.clone();
        this.update(cx, |this, cx| this.set_write_with(agent_id, cx))
    });
    MenuEntry {
        label: label.to_owned(),
        selected,
        choose,
    }
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
        .hover(|row| row.bg(theme::popup_hover()))
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
        if !self.scanned {
            self.found = agents::scan_clis();
            self.scanned = true;
        }
        let settings = self.runs.read(cx).settings().clone();
        let found: Vec<agents::CliHit> = self
            .found
            .iter()
            .filter(|hit| !agents::already_installed(&settings.agents, &hit.profile))
            .cloned()
            .collect();
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
                if agent.is_chat() {
                    agent.backend_label()
                } else {
                    environment_label(&agent.environment)
                },
                agent.is_command().then(|| agent.command.clone()),
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
                .map(|agent| agent.name.clone())
                .unwrap_or_else(|| tr("agents.settings.unknown_agent"));
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
        let write_entries = write_with_entries(&settings, &this);
        let write_label = write_with_label(&settings);
        let rescan = this.clone();
        let found_rows = found.iter().enumerate().map(|(index, hit)| {
            let (this, profile) = (this.clone(), hit.profile.clone());
            list_row(
                ("found-cli", index),
                None,
                hit.profile.name.clone(),
                hit.binary.display().to_string(),
                Some(hit.profile.command.clone()),
            )
            .child(
                Button::new(("add-found", index))
                    .link()
                    .label(tr("common.add"))
                    .on_click(move |_, window, cx| {
                        let profile = profile.clone();
                        this.update(cx, |this, cx| this.add_agent(profile, window, cx))
                    }),
            )
        });
        let on_computer = tr("agents.settings.on_this_computer");
        let agents_header = tr("agents.settings.agents_header");
        let actions_header = tr("agents.settings.actions_header");
        v_flex()
            .w_full()
            .child(section_header(
                on_computer.clone(),
                Some(
                    Button::new("rescan-clis")
                        .link()
                        .label(tr("agents.settings.scan_again"))
                        .on_click(move |_, _, cx| {
                            rescan.update(cx, |this, cx| this.rescan(cx))
                        })
                        .into_any_element(),
                ),
            ))
            .child(
                v_flex()
                    .w_full()
                    .px(px(8.))
                    .children(found_rows)
                    .when(found.is_empty(), |rows| {
                        rows.child(
                            div()
                                .px(px(12.))
                                .py(px(8.))
                                .text_size(px(12.))
                                .text_color(theme::text_muted())
                                .child(tr("agents.settings.no_cli_found")),
                        )
                    }),
            )
            .child(section_header(
                agents_header.clone(),
                Some(link_dropdown("add-agent", "+ Add agent", preset_entries).into_any_element()),
            ))
            .child(v_flex().w_full().px(px(8.)).children(agent_rows))
            .child(section_header(
                actions_header,
                Some(add_action.into_any_element()),
            ))
            .child(v_flex().w_full().px(px(8.)).children(action_rows))
            .child(
                v_flex()
                    .w_full()
                    .px(px(PAGE_PADDING))
                    .pt(px(16.))
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(tr("agents.settings.write_with")),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme::text_muted())
                            .child(tr("agents.settings.rewrite_hint")),
                    )
                    .child(dropdown("write-with", write_label, write_entries)),
            )
            .child(resume_toggle(settings.resume_interrupted_runs, this))
            .child(
                div()
                    .w_full()
                    .px(px(PAGE_PADDING))
                    .pt(px(16.))
                    .pb(px(16.))
                    .text_size(px(12.))
                    .line_height(relative(1.5))
                    .text_color(theme::text_faint())
                    .child(tr("agents.settings.note")),
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
