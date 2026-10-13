use std::rc::Rc;

use agents::{AgentAction, Environment};
use chrono::Utc;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, h_flex,
    input::{InputEvent, Textarea, TextareaState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::runs::{SessionTarget, StartOptions};
use super::session_options::{
    SessionOption, default_wsl_distro, external_options, load_external, mark_live_runs, run_options,
};
use super::start_dialog_pickers::{MenuPart, MenuRow, locked_select, picker_row, select};
use super::{
    mcp_bridge, message_context, raise_notice, runs_handle, start_run, start_run_when_mcp_ready,
};
use crate::app_state::AppState;
use crate::theme;
use crate::views::widgets::{icon, symbol};

const CARD_WIDTH: f32 = 520.;
const CARD_RADIUS: f32 = 10.;
const CARD_PADDING: f32 = 16.;
const CARD_MARGIN: f32 = 32.;
const SECTION_GAP: f32 = 12.;
const BACKDROP_OPACITY: f32 = 0.55;
const CLOSE_SIZE: f32 = 28.;
const BUTTON_HEIGHT: f32 = 32.;
const QUOTE_LINES: usize = 3;
const FIELD_MIN_ROWS: usize = 3;
const FIELD_MAX_ROWS: usize = 6;
const PROMPT_SIZE: f32 = 12.;
const DISABLED_OPACITY: f32 = 0.5;
const DEFAULT_PLACEHOLDER: &str = "e.g. only the backend, open a PR when done";
const SESSION_PLACEHOLDER: &str = "Type the message to send";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartRequest {
    pub action_id: String,
    pub conversation_id: String,
    pub message_id: String,
}

pub enum StartDialogEvent {
    Close,
}

pub struct StartDialog {
    app: Entity<AppState>,
    request: StartRequest,
    action: AgentAction,
    subtitle: String,
    sender: String,
    quote: String,
    needs_allow: bool,
    input: Entity<TextareaState>,
    show_prompt: bool,
    environment: Environment,
    default_working_dir: String,
    working_dir: String,
    recent_working_dirs: Vec<String>,
    offers_sessions: bool,
    scans_external: bool,
    run_sessions: Vec<SessionOption>,
    external_sessions: Vec<SessionOption>,
    selected_session: Option<SessionOption>,
    external_generation: u64,
}

impl EventEmitter<StartDialogEvent> for StartDialog {}

pub fn needs_dialog(action: &AgentAction, chat_allowed: bool) -> bool {
    action.ask_extra_instructions || !chat_allowed
}

impl StartDialog {
    pub fn new(
        app: Entity<AppState>,
        request: StartRequest,
        action: AgentAction,
        agent_name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let context =
            message_context::build(app.read(cx), &request.conversation_id, &request.message_id);
        let (sender, quote) = context
            .map(|context| (context.sender, context.message))
            .unwrap_or_default();
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(FIELD_MIN_ROWS, FIELD_MAX_ROWS)
                .placeholder(DEFAULT_PLACEHOLDER)
        });
        input.update(cx, |input, cx| input.focus(window, cx));
        cx.subscribe(&input, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        let runs = runs_handle(cx).map(|runs| runs.read(cx));
        let profile = runs.and_then(|runs| {
            runs.settings()
                .agents
                .iter()
                .find(|profile| profile.id == action.agent_id)
                .cloned()
        });
        let recent_working_dirs = runs
            .map(|runs| {
                runs.settings()
                    .recent_working_dirs_of(&action.agent_id)
                    .to_vec()
            })
            .unwrap_or_default();
        let offers_sessions = profile.as_ref().is_some_and(agents::supports_resume);
        let run_sessions = match runs {
            Some(runs) if offers_sessions => {
                run_options(&runs.for_agent(&action.agent_id), app.read(cx), Utc::now())
            }
            _ => Vec::new(),
        };
        let mut dialog = StartDialog {
            app,
            subtitle: agent_name.to_owned(),
            needs_allow: !mcp_bridge::chat_allowed(&request.conversation_id, cx),
            request,
            sender,
            quote,
            input,
            show_prompt: false,
            environment: profile
                .as_ref()
                .map(|profile| profile.environment.clone())
                .unwrap_or_default(),
            default_working_dir: action.working_dir.clone(),
            working_dir: action.working_dir.clone(),
            recent_working_dirs,
            offers_sessions,
            scans_external: profile
                .is_some_and(|profile| profile.output == agents::OutputFormat::ClaudeStreamJson)
                && offers_sessions,
            run_sessions,
            external_sessions: Vec::new(),
            selected_session: None,
            external_generation: 0,
            action,
        };
        dialog.reload_external(cx);
        dialog
    }

    fn reload_external(&mut self, cx: &mut Context<Self>) {
        self.external_sessions.clear();
        if !self.scans_external {
            return;
        }
        self.external_generation += 1;
        let generation = self.external_generation;
        let environment = self.environment.clone();
        let working_dir = self.working_dir.clone();
        let known_session_ids = runs_handle(cx)
            .map(|runs| runs.read(cx).session_ids())
            .unwrap_or_default();
        let demo = crate::agent_api::is_demo(cx);
        cx.spawn(async move |this, cx| {
            let scanned_dir = working_dir.clone();
            let loaded = cx
                .background_executor()
                .spawn(async move {
                    load_external(&environment, &scanned_dir, &known_session_ids, demo)
                })
                .await;
            this.update(cx, |this, cx| {
                if this.external_generation == generation {
                    this.external_sessions =
                        external_options(&loaded.sessions, &working_dir, Utc::now());
                    mark_live_runs(&mut this.run_sessions, &loaded.live_session_ids);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn choose_session(
        &mut self,
        session: Option<SessionOption>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let placeholder = if session.is_some() {
            SESSION_PLACEHOLDER
        } else {
            DEFAULT_PLACEHOLDER
        };
        self.input.update(cx, |input, cx| {
            input.set_placeholder(placeholder, window, cx)
        });
        self.selected_session = session;
        cx.notify();
    }

    fn choose_working_dir(&mut self, working_dir: String, cx: &mut Context<Self>) {
        if self.working_dir != working_dir {
            self.working_dir = working_dir;
            self.reload_external(cx);
        }
        cx.notify();
    }

    fn browse(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        let environment = self.environment.clone();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let text = path.to_string_lossy().into_owned();
            let working_dir = match &environment {
                Environment::Host => text,
                Environment::Wsl { distro } => {
                    let chosen = agents::wsl_unc_distro(&text).map(str::to_owned);
                    let wanted = match (&chosen, distro) {
                        (_, Some(distro)) => Some(distro.clone()),
                        (Some(_), None) => {
                            let demo = cx.update(|cx| crate::agent_api::is_demo(cx));
                            cx.background_executor()
                                .spawn(async move { default_wsl_distro(demo) })
                                .await
                        }
                        (None, None) => None,
                    };
                    if chosen.is_some() && wanted.is_some() && chosen != wanted {
                        cx.update(|cx| {
                            raise_notice(
                                "That folder is in a different WSL distro than the agent"
                                    .to_owned(),
                                cx,
                            )
                        });
                        return;
                    }
                    agents::linux_path(&text)
                }
            };
            this.update(cx, |this, cx| this.choose_working_dir(working_dir, cx))
                .ok();
        })
        .detach();
    }

    fn start_options(&self) -> StartOptions {
        match &self.selected_session {
            Some(session) => StartOptions {
                working_dir: None,
                remember_working_dir: false,
                session: Some(SessionTarget {
                    session_id: session.session_id.clone(),
                    working_dir: session.working_dir.clone(),
                    fork: session.fork,
                }),
            },
            None => StartOptions {
                working_dir: (self.working_dir != self.default_working_dir)
                    .then(|| self.working_dir.clone()),
                remember_working_dir: true,
                session: None,
            },
        }
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        let extra = self.input.read(cx).value().trim().to_owned();
        if self.selected_session.is_some() && extra.is_empty() {
            return;
        }
        let options = self.start_options();
        if self.needs_allow {
            mcp_bridge::allow_chat(&self.request.conversation_id, cx);
        }
        if self.needs_allow && mcp_bridge::endpoint(cx).is_none() {
            start_run_when_mcp_ready(
                self.request.action_id.clone(),
                self.request.conversation_id.clone(),
                self.request.message_id.clone(),
                extra,
                options,
                cx,
            );
        } else {
            start_run(
                &self.request.action_id,
                &self.request.conversation_id,
                &self.request.message_id,
                &extra,
                options,
                cx,
            );
        }
        cx.emit(StartDialogEvent::Close);
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => cx.emit(StartDialogEvent::Close),
            "enter" if event.keystroke.modifiers.control => self.start(cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    fn header(&self, cx: &mut Context<Self>) -> Div {
        h_flex()
            .w_full()
            .items_start()
            .gap(px(SECTION_GAP))
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::BOLD)
                            .child(self.action.label.clone()),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(theme::text_muted())
                            .child(self.subtitle.clone()),
                    ),
            )
            .child(
                div()
                    .id("agent-dialog-close")
                    .size(px(CLOSE_SIZE))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .hover(|button| button.bg(theme::popup_hover()))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(StartDialogEvent::Close)))
                    .child(symbol("close", 16., theme::text_muted())),
            )
    }

    fn quote(&self) -> impl IntoElement {
        let text = format!("{} - {}", self.sender, self.quote);
        let sender_style = HighlightStyle {
            color: Some(theme::text()),
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        };
        let sender_range = 0..self.sender.len();
        div()
            .w_full()
            .px(px(12.))
            .py(px(8.))
            .rounded(px(8.))
            .bg(theme::reply_surface())
            .text_size(px(12.))
            .line_height(relative(1.4))
            .text_color(theme::text_soft())
            .line_clamp(QUOTE_LINES)
            .child(StyledText::new(text).with_highlights([(sender_range, sender_style)]))
    }

    fn session_parts(&self, dialog: &Entity<Self>) -> Vec<MenuPart> {
        let row = |session: &SessionOption| {
            let dialog = dialog.clone();
            let chosen = session.clone();
            MenuPart::Row(MenuRow {
                label: session.label.clone(),
                meta: Some(session.meta.clone()),
                running: session.running,
                selected: self
                    .selected_session
                    .as_ref()
                    .is_some_and(|selected| selected.session_id == session.session_id),
                choose: Rc::new(move |window, cx| {
                    let chosen = chosen.clone();
                    dialog.update(cx, |dialog, cx| {
                        dialog.choose_session(Some(chosen), window, cx)
                    });
                }),
            })
        };
        let new_dialog = dialog.clone();
        let mut parts = vec![MenuPart::Row(MenuRow {
            label: "New session".to_owned(),
            meta: None,
            running: false,
            selected: self.selected_session.is_none(),
            choose: Rc::new(move |window, cx| {
                new_dialog.update(cx, |dialog, cx| dialog.choose_session(None, window, cx));
            }),
        })];
        if !self.run_sessions.is_empty() {
            parts.push(MenuPart::Separator);
            parts.push(MenuPart::Header("Recent sessions".to_owned()));
            parts.extend(self.run_sessions.iter().map(row));
        }
        if !self.external_sessions.is_empty() {
            parts.push(MenuPart::Separator);
            parts.push(MenuPart::Header(format!(
                "Other sessions in {}",
                self.working_dir
            )));
            parts.extend(self.external_sessions.iter().map(row));
        }
        parts
    }

    fn working_dir_parts(&self, dialog: &Entity<Self>) -> Vec<MenuPart> {
        let row = |working_dir: &str, meta: Option<&str>| {
            let dialog = dialog.clone();
            let chosen = working_dir.to_owned();
            MenuPart::Row(MenuRow {
                label: working_dir.to_owned(),
                meta: meta.map(str::to_owned),
                running: false,
                selected: self.working_dir == working_dir,
                choose: Rc::new(move |_, cx| {
                    let chosen = chosen.clone();
                    dialog.update(cx, |dialog, cx| dialog.choose_working_dir(chosen, cx));
                }),
            })
        };
        let browse_dialog = dialog.clone();
        let mut parts = vec![
            MenuPart::Header("Recent".to_owned()),
            row(&self.default_working_dir, Some("action default")),
        ];
        parts.extend(
            self.recent_working_dirs
                .iter()
                .filter(|working_dir| **working_dir != self.default_working_dir)
                .map(|working_dir| row(working_dir, None)),
        );
        parts.push(MenuPart::Separator);
        parts.push(MenuPart::Row(MenuRow {
            label: "Browse...".to_owned(),
            meta: None,
            running: false,
            selected: false,
            choose: Rc::new(move |_, cx| {
                browse_dialog.update(cx, |dialog, cx| dialog.browse(cx));
            }),
        }));
        parts
    }

    fn pickers(&self, cx: &mut Context<Self>) -> Div {
        let dialog = cx.entity();
        let session_row = self.offers_sessions.then(|| {
            let (value, meta, running) = match &self.selected_session {
                Some(session) => (
                    session.label.clone(),
                    Some(session.meta.clone()),
                    session.running,
                ),
                None => ("New session".to_owned(), None, false),
            };
            picker_row(
                "Session",
                select(
                    "agent-dialog-session",
                    value,
                    meta,
                    running,
                    self.session_parts(&dialog),
                ),
            )
        });
        let working_dir_control = match &self.selected_session {
            Some(session) => {
                locked_select(session.working_dir.clone(), "from session").into_any_element()
            }
            None => select(
                "agent-dialog-working-dir",
                self.working_dir.clone(),
                None,
                false,
                self.working_dir_parts(&dialog),
            )
            .into_any_element(),
        };
        v_flex()
            .w_full()
            .gap(px(8.))
            .children(session_row)
            .child(picker_row("Working dir", working_dir_control))
    }

    fn field(&self, window: &Window, cx: &App) -> Div {
        let focused = self.input.focus_handle(cx).contains_focused(window, cx);
        v_flex()
            .w_full()
            .gap(px(6.))
            .child(
                div()
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text_faint())
                    .child(if self.selected_session.is_some() {
                        "Message to the session"
                    } else {
                        "Extra instructions (optional)"
                    }),
            )
            .child(
                div()
                    .w_full()
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(8.))
                    .bg(theme::surface_raised())
                    .border_1()
                    .border_color(if focused {
                        theme::accent()
                    } else {
                        theme::surface_raised()
                    })
                    .child(Textarea::new(&self.input).appearance(false).bordered(false)),
            )
    }

    fn prompt_toggle(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        h_flex()
            .id("agent-dialog-prompt-toggle")
            .gap(px(4.))
            .items_center()
            .cursor_pointer()
            .text_size(px(12.))
            .text_color(theme::text_muted())
            .hover(|toggle| toggle.text_color(theme::text_soft()))
            .child(icon(
                if self.show_prompt {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                },
                12.,
                theme::text_muted(),
            ))
            .child("Show prompt")
            .on_click(cx.listener(|this, _, _, cx| {
                this.show_prompt = !this.show_prompt;
                cx.notify();
            }))
    }

    fn prompt_box(&self, cx: &mut Context<Self>) -> Div {
        let extra = self.input.read(cx).value().trim().to_owned();
        let prompt = if self.selected_session.is_some() {
            extra
        } else {
            message_context::preview_prompt(
                self.app.read(cx),
                &self.action,
                &self.request.conversation_id,
                &self.request.message_id,
                &extra,
            )
            .unwrap_or_default()
        };
        div()
            .w_full()
            .max_h(px(160.))
            .px(px(12.))
            .py(px(8.))
            .rounded(px(8.))
            .bg(theme::reply_surface())
            .overflow_hidden()
            .font_family(cx.theme().mono_font_family.clone())
            .text_size(px(PROMPT_SIZE))
            .line_height(relative(1.5))
            .text_color(theme::text_muted())
            .child(prompt)
    }

    fn buttons(&self, cx: &mut Context<Self>) -> Div {
        let blocked =
            self.selected_session.is_some() && self.input.read(cx).value().trim().is_empty();
        let start_label = if self.selected_session.is_some() {
            "Send"
        } else if self.needs_allow {
            "Allow and start"
        } else {
            "Start"
        };
        h_flex()
            .w_full()
            .items_center()
            .gap(px(8.))
            .child(self.prompt_toggle(cx))
            .child(div().flex_1())
            .child(
                h_flex()
                    .id("agent-dialog-cancel")
                    .h(px(BUTTON_HEIGHT))
                    .px(px(12.))
                    .items_center()
                    .rounded(px(8.))
                    .cursor_pointer()
                    .bg(theme::surface_raised())
                    .hover(|button| button.bg(theme::popup_hover()))
                    .text_size(px(13.))
                    .child("Cancel")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(StartDialogEvent::Close))),
            )
            .child(
                h_flex()
                    .id("agent-dialog-start")
                    .h(px(BUTTON_HEIGHT))
                    .px(px(12.))
                    .items_center()
                    .rounded(px(8.))
                    .bg(theme::accent())
                    .text_color(theme::on_accent())
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(start_label)
                    .when(blocked, |button| button.opacity(DISABLED_OPACITY))
                    .when(!blocked, |button| {
                        button
                            .cursor_pointer()
                            .hover(|button| button.bg(theme::accent_text()))
                            .on_click(cx.listener(|this, _, _, cx| this.start(cx)))
                    }),
            )
    }
}

impl Render for StartDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("agent-dialog-layer")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("agent-dialog-backdrop")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(black().opacity(BACKDROP_OPACITY))
                    .occlude()
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(StartDialogEvent::Close))),
            )
            .child(
                v_flex()
                    .id("agent-dialog-card")
                    .w(px(CARD_WIDTH))
                    .max_w(relative(1.))
                    .max_h(relative(1.))
                    .m(px(CARD_MARGIN))
                    .p(px(CARD_PADDING))
                    .gap(px(SECTION_GAP))
                    .rounded(px(CARD_RADIUS))
                    .bg(theme::popup())
                    .text_color(theme::text())
                    .occlude()
                    .capture_key_down(cx.listener(Self::on_key_down))
                    .child(self.header(cx))
                    .child(self.quote())
                    .when(self.needs_allow, |card| {
                        card.child(
                            div()
                                .text_size(px(12.))
                                .text_color(theme::text_soft())
                                .child("Agents can't see this chat yet."),
                        )
                    })
                    .child(self.pickers(cx))
                    .child(self.field(window, cx))
                    .when(self.show_prompt, |card| card.child(self.prompt_box(cx)))
                    .child(self.buttons(cx)),
            )
    }
}

#[cfg(test)]
mod tests {
    use agents::AgentAction;

    use super::needs_dialog;

    #[test]
    fn the_dialog_shows_for_extra_instructions_or_a_chat_agents_cannot_see() {
        let quiet = AgentAction::answer_this("claude");
        let asking = AgentAction::build_this("claude");
        assert!(!needs_dialog(&quiet, true));
        assert!(needs_dialog(&quiet, false));
        assert!(needs_dialog(&asking, true));
    }
}
