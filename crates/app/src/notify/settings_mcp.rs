use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use mcp::install::{InstallReport, InstallState, Location, Target};
use mcp::{Access, AgentPolicy};

use super::settings_view::{control_row, switch, titled_with};
use crate::agent_api::{self, ServerStatus};
use crate::app_state::AppState;
use crate::data::is_one_on_one;
use crate::fuzzy;
use crate::theme;
use crate::views::switcher::{Candidate, candidate_avatar, candidates_from};
use crate::views::widgets::icon;

const COPIED_SHOWN: Duration = Duration::from_millis(1500);
const PORT_WIDTH: f32 = 72.;
const FIELD_HEIGHT: f32 = 36.;
const SMALL_BUTTON_HEIGHT: f32 = 28.;
const PICKER_ROW_HEIGHT: f32 = 30.;
const PICKER_MAX_ROWS: usize = 6;
const PICKER_RESULTS: usize = 40;
const TOKEN_TAIL: usize = 4;
const CHIP_MAX_WIDTH: f32 = 180.;
const WSL_WARNING: &str = "WSL agents cannot reach 127.0.0.1 on Windows. Add networkingMode=mirrored to .wslconfig and run wsl --shutdown.";

#[derive(Clone, Copy, PartialEq, Eq)]
enum McpTab {
    Server,
    Access,
    Install,
}

impl McpTab {
    const ALL: [McpTab; 3] = [McpTab::Server, McpTab::Access, McpTab::Install];

    fn label(self) -> &'static str {
        match self {
            McpTab::Server => "Server",
            McpTab::Access => "Access",
            McpTab::Install => "Install",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Chats,
    Channels,
}

impl Scope {
    fn access(self, policy: &AgentPolicy) -> &Access {
        match self {
            Scope::Chats => &policy.chats,
            Scope::Channels => &policy.channels,
        }
    }

    fn access_mut(self, policy: &mut AgentPolicy) -> &mut Access {
        match self {
            Scope::Chats => &mut policy.chats,
            Scope::Channels => &mut policy.channels,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Scope::Chats => "Chats",
            Scope::Channels => "Channels",
        }
    }

    fn add_label(self) -> &'static str {
        match self {
            Scope::Chats => "+ Add chat",
            Scope::Channels => "+ Add channel",
        }
    }

    fn contains(self, candidate: &Candidate) -> bool {
        use crate::app_state::Selection;
        matches!(
            (self, &candidate.selection),
            (Scope::Chats, Selection::Chat(_)) | (Scope::Channels, Selection::Channel(_))
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Copied {
    Url,
    Token,
    Manual,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InstallAction {
    Install,
    Update,
    Remove,
}

impl InstallAction {
    fn label(self) -> &'static str {
        match self {
            InstallAction::Install => "Install",
            InstallAction::Update => "Update",
            InstallAction::Remove => "Remove",
        }
    }

    fn busy_label(self) -> &'static str {
        match self {
            InstallAction::Install => "Installing...",
            InstallAction::Update => "Updating...",
            InstallAction::Remove => "Removing...",
        }
    }
}

fn target_logo(target: Target) -> &'static str {
    match target {
        Target::ClaudeCode => "CC",
        Target::Codex => "CX",
        Target::CopilotCli => "GH",
        Target::VsCode => "VS",
        Target::Cursor => "CU",
    }
}

fn target_action(state: &InstallState) -> Option<InstallAction> {
    if state.stale {
        Some(InstallAction::Update)
    } else if !state.installed_at.is_empty() {
        Some(InstallAction::Remove)
    } else if state.available {
        Some(InstallAction::Install)
    } else {
        None
    }
}

fn locations_label(locations: &[Location]) -> &'static str {
    let windows = locations.contains(&Location::Windows);
    let wsl = locations
        .iter()
        .any(|location| matches!(location, Location::Wsl { .. }));
    match (windows, wsl) {
        (true, true) => "Windows + WSL",
        (true, false) => "Windows",
        (false, true) => "WSL",
        (false, false) => "Installed",
    }
}

fn install_detail(target: Target, state: Option<&InstallState>) -> AnyElement {
    let muted = |text: &'static str| {
        div()
            .text_size(px(12.))
            .text_color(theme::text_muted())
            .child(text)
            .into_any_element()
    };
    let Some(state) = state else {
        return muted(target.config_hint());
    };
    if state.stale {
        return div()
            .text_size(px(12.))
            .text_color(theme::amber())
            .child("Out of date - Update")
            .into_any_element();
    }
    if !state.installed_at.is_empty() {
        return h_flex()
            .gap(px(6.))
            .items_center()
            .text_size(px(12.))
            .text_color(theme::green())
            .child(div().size(px(6.)).rounded_full().bg(theme::green()))
            .child(locations_label(&state.installed_at))
            .into_any_element();
    }
    if state.available {
        muted(target.config_hint())
    } else {
        muted("Not installed")
    }
}

fn wsl_warning() -> Div {
    note_box(
        h_flex()
            .gap(px(8.))
            .items_start()
            .child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme::amber())
                    .child("!"),
            )
            .child(div().flex_1().child(WSL_WARNING)),
        4.,
    )
}

pub struct McpView {
    app: Entity<AppState>,
    tab: McpTab,
    picker: Option<Scope>,
    port_input: Entity<InputState>,
    port_invalid: bool,
    search_input: Entity<InputState>,
    copied: Option<Copied>,
    install_states: Option<Vec<InstallState>>,
    install_busy: Option<(Target, InstallAction)>,
    install_errors: HashMap<Target, String>,
    wsl_unreachable: bool,
    _subscriptions: Vec<Subscription>,
}

impl McpView {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let port = agent_api::policy(cx).port;
        let port_input = cx.new(|cx| InputState::new(window, cx));
        port_input.update(cx, |input, cx| {
            input.set_value(port.to_string(), window, cx)
        });
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let mut subscriptions = vec![
            cx.subscribe_in(&port_input, window, Self::on_port_event),
            cx.subscribe_in(&search_input, window, |_, _, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
        ];
        if let Some(api) = agent_api::handle(cx) {
            subscriptions.push(cx.observe(&api, |_, _, cx| cx.notify()));
        }
        let mut view = McpView {
            app,
            tab: McpTab::Server,
            picker: None,
            port_input,
            port_invalid: false,
            search_input,
            copied: None,
            install_states: None,
            install_busy: None,
            install_errors: HashMap::new(),
            wsl_unreachable: false,
            _subscriptions: subscriptions,
        };
        view.refresh_wsl(cx);
        view
    }

    fn on_port_event(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
            return;
        }
        let current = agent_api::policy(cx).port;
        let typed = input
            .read(cx)
            .value()
            .trim()
            .parse::<u16>()
            .ok()
            .filter(|port| *port != 0);
        self.port_invalid = typed.is_none();
        if let Some(port) = typed
            && port != current
        {
            agent_api::update_policy(cx, |policy| policy.port = port);
        }
        self.refresh_wsl(cx);
        cx.notify();
    }

    fn refresh_wsl(&mut self, cx: &mut Context<Self>) {
        let port = agent_api::policy(cx).port;
        let task = cx.background_spawn(async move { mcp::install::wsl_reachable(port) });
        cx.spawn(async move |this, cx| {
            let reachable = task.await;
            this.update(cx, |this, cx| {
                this.wsl_unreachable = reachable == Some(false);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn refresh_installs(&mut self, cx: &mut Context<Self>) {
        let Some(endpoint) = agent_api::configured_endpoint(cx) else {
            return;
        };
        let demo = agent_api::is_demo(cx);
        let task = cx.background_spawn(async move {
            let homes = agent_api::agent_homes(demo);
            Target::ALL
                .map(|target| mcp::install::status(target, &endpoint, &homes))
                .to_vec()
        });
        cx.spawn(async move |this, cx| {
            let states = task.await;
            this.update(cx, |this, cx| {
                this.install_states = Some(states);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn run_install_action(
        &mut self,
        target: Target,
        action: InstallAction,
        cx: &mut Context<Self>,
    ) {
        let Some(endpoint) = agent_api::configured_endpoint(cx) else {
            return;
        };
        self.install_busy = Some((target, action));
        self.install_errors.remove(&target);
        let demo = agent_api::is_demo(cx);
        let task = cx.background_spawn(async move {
            let homes = agent_api::agent_homes(demo);
            match action {
                InstallAction::Remove => mcp::install::uninstall(target, &homes).map(|_| None),
                _ => mcp::install::install(target, &endpoint, &homes).map(Some),
            }
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.install_busy = None;
                match result {
                    Ok(Some(InstallReport::OpenUrl(url))) => cx.open_url(&url),
                    Ok(_) => {}
                    Err(error) => {
                        this.install_errors.insert(target, error.to_string());
                    }
                }
                this.refresh_installs(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn select_tab(&mut self, tab: McpTab, cx: &mut Context<Self>) {
        self.tab = tab;
        self.picker = None;
        match tab {
            McpTab::Install => {
                self.refresh_installs(cx);
                self.refresh_wsl(cx);
            }
            McpTab::Server => self.refresh_wsl(cx),
            McpTab::Access => {}
        }
        cx.notify();
    }

    fn copy(&mut self, target: Copied, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.copied = Some(target);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED_SHOWN).await;
            this.update(cx, |this, cx| {
                this.copied = None;
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn open_picker(&mut self, scope: Scope, window: &mut Window, cx: &mut Context<Self>) {
        self.picker = if self.picker == Some(scope) {
            None
        } else {
            Some(scope)
        };
        self.search_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        if self.picker.is_some() {
            self.search_input
                .update(cx, |input, cx| input.focus(window, cx));
        }
        cx.notify();
    }
}

fn small_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    inert_button(id, label)
        .cursor_pointer()
        .text_color(theme::text())
        .hover(|button| button.bg(theme::popup_hover()))
}

fn inert_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(SMALL_BUTTON_HEIGHT))
        .px(px(12.))
        .flex_none()
        .flex()
        .items_center()
        .rounded(px(7.))
        .bg(theme::surface_raised())
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text_faint())
        .child(label.into())
}

fn caption(label: &'static str) -> Div {
    div()
        .px(px(20.))
        .pt(px(16.))
        .pb(px(4.))
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text_muted())
        .child(label)
}

fn toggle_row(
    id: &'static str,
    title: &'static str,
    detail: &'static str,
    detail_color: Hsla,
    on: bool,
) -> Stateful<Div> {
    h_flex()
        .id(id)
        .w_full()
        .px(px(20.))
        .py(px(10.))
        .gap(px(12.))
        .items_center()
        .cursor_pointer()
        .hover(|row| row.bg(theme::row_hover()))
        .child(titled_with(title, detail, detail_color))
        .child(switch(on))
}

fn note_box(content: impl IntoElement, margin_top: f32) -> Div {
    h_flex()
        .mx(px(20.))
        .mt(px(margin_top))
        .mb(px(12.))
        .px(px(12.))
        .py(px(10.))
        .gap(px(8.))
        .rounded(px(8.))
        .bg(theme::row_hover())
        .text_size(px(12.))
        .text_color(theme::text_muted())
        .child(content)
}

fn masked_token(token: &str) -> String {
    let tail: String = token
        .chars()
        .rev()
        .take(TOKEN_TAIL)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("****...{tail}")
}

fn chat_kind_label(state: &AppState, conversation_id: &str) -> Option<&'static str> {
    let chat = state
        .sidebar
        .chats
        .iter()
        .find(|chat| chat.id == conversation_id)?;
    Some(if chat.kind.eq_ignore_ascii_case("meeting") {
        "Meeting"
    } else if is_one_on_one(chat) {
        "Chat"
    } else {
        "Group"
    })
}

impl Render for McpView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let tabs = h_flex()
            .mx(px(20.))
            .mt(px(12.))
            .mb(px(4.))
            .p(px(2.))
            .rounded(px(8.))
            .bg(theme::surface())
            .children(McpTab::ALL.map(|tab| {
                let active = tab == self.tab;
                let view = view.clone();
                div()
                    .id(tab.label())
                    .flex_1()
                    .h(px(SMALL_BUTTON_HEIGHT))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if active {
                        theme::text()
                    } else {
                        theme::text_muted()
                    })
                    .when(active, |tab| tab.bg(theme::surface_raised()))
                    .child(tab.label())
                    .on_click(move |_, _, cx| view.update(cx, |view, cx| view.select_tab(tab, cx)))
            }));
        let body = match self.tab {
            McpTab::Server => self.server_tab(cx).into_any_element(),
            McpTab::Access => self.access_tab(cx).into_any_element(),
            McpTab::Install => self.install_tab(cx).into_any_element(),
        };
        v_flex().w_full().flex_1().min_h_0().child(tabs).child(
            div()
                .id("mcp-body")
                .w_full()
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(body),
        )
    }
}

impl McpView {
    fn field_row(
        &self,
        label: &'static str,
        value: String,
        buttons: Vec<AnyElement>,
        cx: &App,
    ) -> Div {
        v_flex()
            .w_full()
            .px(px(20.))
            .pb(px(8.))
            .gap(px(4.))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme::text_faint())
                    .child(label),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap(px(8.))
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h(px(FIELD_HEIGHT))
                            .px(px(12.))
                            .flex()
                            .items_center()
                            .rounded(px(8.))
                            .bg(theme::surface())
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_size(px(12.))
                            .text_color(theme::text_soft())
                            .child(div().truncate().child(value)),
                    )
                    .children(buttons),
            )
    }

    fn copy_button(
        &self,
        id: &'static str,
        target: Copied,
        text: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = if self.copied == Some(target) {
            "Copied"
        } else {
            "Copy"
        };
        small_button(id, label)
            .on_click(cx.listener(move |this, _, _, cx| this.copy(target, text.clone(), cx)))
            .into_any_element()
    }

    fn server_tab(&self, cx: &mut Context<Self>) -> Div {
        let policy = agent_api::policy(cx);
        let (status, token) = match agent_api::handle(cx) {
            Some(api) => {
                let api = api.read(cx);
                (api.status().clone(), api.token().to_owned())
            }
            None => (ServerStatus::Off, String::new()),
        };
        let failed = matches!(status, ServerStatus::PortInUse(_) | ServerStatus::Failed(_));
        let (detail, detail_color) = match (policy.enabled, failed) {
            (false, _) => ("Off - agents cannot reach Rusty Teams", theme::text_muted()),
            (true, true) => ("Could not start", theme::red_soft()),
            (true, false) => (
                "Agents on this computer can read and propose",
                theme::text_muted(),
            ),
        };
        let enabled = policy.enabled;
        let running = matches!(status, ServerStatus::Running(_));
        let reinstall_note =
            agent_api::handle(cx).and_then(|api| api.read(cx).reinstall_note().map(str::to_owned));
        let warning = match &status {
            ServerStatus::PortInUse(port) => Some(format!(
                "Port {port} is in use by another program. Pick another port, then press Install again for each agent."
            )),
            ServerStatus::Failed(reason) => Some(reason.clone()),
            _ => None,
        }
        .or(reinstall_note);
        let url = agent_api::endpoint_url(policy.port);
        let regenerate = small_button("mcp-token-new", "New")
            .on_click(cx.listener(|_, _, _, cx| {
                if let Some(api) = agent_api::handle(cx) {
                    api.update(cx, |api, cx| api.regenerate_token(cx));
                }
            }))
            .into_any_element();
        v_flex()
            .w_full()
            .child(
                toggle_row("mcp-enabled", "MCP server", detail, detail_color, enabled).on_click(
                    cx.listener(move |_, _, _, cx| {
                        agent_api::update_policy(cx, |policy| policy.enabled = !enabled)
                    }),
                ),
            )
            .children(warning.map(|text| {
                note_box(
                    h_flex()
                        .gap(px(8.))
                        .items_start()
                        .child(
                            div()
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme::red_soft())
                                .child("!"),
                        )
                        .child(div().flex_1().child(text)),
                    4.,
                )
            }))
            .child(control_row(
                "Port",
                "Only this computer can connect",
                div()
                    .w(px(PORT_WIDTH))
                    .flex_none()
                    .child(Input::new(&self.port_input)),
            ))
            .when(self.port_invalid, |tab| {
                tab.child(
                    div()
                        .px(px(20.))
                        .pb(px(4.))
                        .text_size(px(12.))
                        .text_color(theme::red_soft())
                        .child("Enter a port from 1 to 65535. Nothing was saved."),
                )
            })
            .when(running && self.wsl_unreachable, |tab| {
                tab.child(wsl_warning())
            })
            .child(caption("CONNECTION"))
            .child(self.field_row(
                "URL",
                url.clone(),
                vec![self.copy_button("mcp-url-copy", Copied::Url, url, cx)],
                cx,
            ))
            .child(self.field_row(
                "Token",
                masked_token(&token),
                vec![
                    self.copy_button("mcp-token-copy", Copied::Token, token, cx),
                    regenerate,
                ],
                cx,
            ))
    }

    fn segmented(&self, scope: Scope, access: &Access, cx: &mut Context<Self>) -> Div {
        let options: [(&'static str, bool, fn() -> Access); 3] = [
            ("All", matches!(access, Access::All), || Access::All),
            ("Selected", matches!(access, Access::Selected(_)), || {
                Access::Selected(BTreeSet::new())
            }),
            ("None", matches!(access, Access::None), || Access::None),
        ];
        h_flex()
            .flex_none()
            .p(px(2.))
            .rounded(px(8.))
            .bg(theme::surface_raised())
            .children(options.map(|(label, active, make)| {
                div()
                    .id(SharedString::from(format!("{}-{label}", scope.title())))
                    .h(px(24.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if active {
                        theme::text()
                    } else {
                        theme::text_muted()
                    })
                    .when(active, |item| item.bg(theme::popup_hover()))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !active {
                            this.picker = None;
                            agent_api::update_policy(cx, |policy| {
                                *scope.access_mut(policy) = make()
                            });
                        }
                    }))
            }))
    }

    fn scope_detail(scope: Scope, access: &Access) -> String {
        match (scope, access) {
            (Scope::Chats, Access::All) => "All chats".to_owned(),
            (Scope::Channels, Access::All) => "All channels".to_owned(),
            (Scope::Chats, Access::None) => "No chat visible".to_owned(),
            (Scope::Channels, Access::None) => "No channel visible".to_owned(),
            (Scope::Chats, Access::Selected(ids)) => {
                format!("{} selected, incl. meeting chats", ids.len())
            }
            (Scope::Channels, Access::Selected(ids)) => format!("{} selected", ids.len()),
        }
    }

    fn scope_block(
        &self,
        scope: Scope,
        policy: &AgentPolicy,
        titles: &HashMap<String, String>,
        cx: &mut Context<Self>,
    ) -> Div {
        let access = scope.access(policy);
        let header = control_row(
            scope.title(),
            Self::scope_detail(scope, access),
            self.segmented(scope, access, cx),
        );
        let Access::Selected(ids) = access else {
            return v_flex().w_full().child(header);
        };
        let chips = ids.iter().map(|conversation_id| {
            let title = titles
                .get(conversation_id)
                .cloned()
                .unwrap_or_else(|| "Unknown".to_owned());
            let conversation_id = conversation_id.clone();
            h_flex()
                .h(px(24.))
                .pl(px(8.))
                .gap(px(4.))
                .items_center()
                .rounded(px(6.))
                .bg(theme::surface_raised())
                .text_size(px(12.))
                .text_color(theme::text_strong())
                .child(div().max_w(px(CHIP_MAX_WIDTH)).truncate().child(title))
                .child(
                    div()
                        .id(SharedString::from(format!("remove-{conversation_id}")))
                        .w(px(16.))
                        .h_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .child(icon(IconName::X, 10., theme::text_faint()))
                        .on_click(cx.listener(move |_, _, _, cx| {
                            let conversation_id = conversation_id.clone();
                            agent_api::update_policy(cx, |policy| {
                                if let Access::Selected(ids) = scope.access_mut(policy) {
                                    ids.remove(&conversation_id);
                                }
                            })
                        })),
                )
        });
        let add = div()
            .id(SharedString::from(format!("add-{}", scope.title())))
            .h(px(24.))
            .px(px(8.))
            .flex()
            .items_center()
            .rounded(px(6.))
            .border_1()
            .border_color(theme::border_strong())
            .cursor_pointer()
            .text_size(px(12.))
            .text_color(theme::accent_text())
            .child(scope.add_label())
            .on_click(cx.listener(move |this, _, window, cx| this.open_picker(scope, window, cx)));
        v_flex()
            .w_full()
            .child(header)
            .child(
                h_flex()
                    .w_full()
                    .flex_wrap()
                    .gap(px(6.))
                    .px(px(20.))
                    .pb(px(8.))
                    .children(chips)
                    .child(add),
            )
            .children(
                self.picker
                    .filter(|open| *open == scope)
                    .map(|_| self.picker_panel(scope, ids, cx)),
            )
    }

    fn picker_panel(
        &self,
        scope: Scope,
        selected: &BTreeSet<String>,
        cx: &mut Context<Self>,
    ) -> Div {
        let query = self.search_input.read(cx).value().to_string();
        let state = self.app.read(cx);
        let candidates: Vec<(String, Candidate)> = candidates_from(state)
            .into_iter()
            .filter(|(_, candidate)| scope.contains(candidate))
            .collect();
        let results = fuzzy::rank(&query, &candidates, PICKER_RESULTS);
        let rows: Vec<AnyElement> = results
            .into_iter()
            .map(|candidate| {
                let conversation_id = candidate.selection.conversation_id().to_owned();
                let added = selected.contains(&conversation_id);
                let kind = match scope {
                    Scope::Chats => chat_kind_label(state, &conversation_id)
                        .unwrap_or("Chat")
                        .to_owned(),
                    Scope::Channels => candidate.subtitle.clone(),
                };
                h_flex()
                    .id(SharedString::from(format!("pick-{conversation_id}")))
                    .h(px(PICKER_ROW_HEIGHT))
                    .flex_none()
                    .px(px(6.))
                    .gap(px(8.))
                    .items_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|row| row.bg(theme::popup_hover()))
                    .child(candidate_avatar(
                        &state.directory,
                        &candidate.avatar,
                        20.,
                        theme::popup(),
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .text_color(theme::text())
                            .child(candidate.title.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(11.))
                            .text_color(if added {
                                theme::accent_text()
                            } else {
                                theme::text_faint()
                            })
                            .child(if added { "Added".to_owned() } else { kind }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let conversation_id = conversation_id.clone();
                        this.picker = None;
                        agent_api::update_policy(cx, |policy| {
                            if let Access::Selected(ids) = scope.access_mut(policy) {
                                ids.insert(conversation_id);
                            }
                        });
                    }))
                    .into_any_element()
            })
            .collect();
        v_flex()
            .mx(px(20.))
            .mb(px(8.))
            .p(px(6.))
            .gap(px(4.))
            .rounded(px(8.))
            .bg(theme::popup())
            .border_1()
            .border_color(theme::border())
            .child(
                h_flex()
                    .h(px(30.))
                    .px(px(6.))
                    .gap(px(6.))
                    .items_center()
                    .child(icon(IconName::Search, 14., theme::text_muted()))
                    .child(
                        div().flex_1().child(
                            Input::new(&self.search_input)
                                .appearance(false)
                                .bordered(false),
                        ),
                    ),
            )
            .child(
                v_flex()
                    .id("mcp-picker-list")
                    .w_full()
                    .max_h(px(PICKER_ROW_HEIGHT * PICKER_MAX_ROWS as f32))
                    .overflow_y_scroll()
                    .children(rows),
            )
    }

    fn access_tab(&self, cx: &mut Context<Self>) -> Div {
        let policy = agent_api::policy(cx);
        let titles: HashMap<String, String> = candidates_from(self.app.read(cx))
            .into_iter()
            .map(|(label, candidate)| (candidate.selection.conversation_id().to_owned(), label))
            .collect();
        let toggles: [(
            &'static str,
            &'static str,
            &'static str,
            Hsla,
            bool,
            fn(&mut AgentPolicy),
        ); 3] = [
            (
                "mcp-propose",
                "Propose messages",
                "Card above the composer, you send",
                theme::text_muted(),
                policy.propose,
                |policy| policy.propose = !policy.propose,
            ),
            (
                "mcp-calendar",
                "Calendar and meetings",
                "Times, titles, organizers, join links",
                theme::text_muted(),
                policy.calendar,
                |policy| policy.calendar = !policy.calendar,
            ),
            (
                "mcp-transcripts",
                "Meeting transcripts",
                "Spoken text goes to the agent's provider",
                theme::amber(),
                policy.transcripts,
                |policy| policy.transcripts = !policy.transcripts,
            ),
        ];
        v_flex()
            .w_full()
            .child(self.scope_block(Scope::Chats, &policy, &titles, cx))
            .child(self.scope_block(Scope::Channels, &policy, &titles, cx))
            .children(toggles.map(|(id, title, detail, color, on, flip)| {
                toggle_row(id, title, detail, color, on)
                    .on_click(cx.listener(move |_, _, _, cx| agent_api::update_policy(cx, flip)))
            }))
            .child(note_box(
                "Agents only see what is allowed here. They never send - you do.",
                4.,
            ))
    }

    fn install_row(&self, index: usize, target: Target, cx: &mut Context<Self>) -> Div {
        let state = self
            .install_states
            .as_ref()
            .and_then(|states| states.get(index));
        let busy = self.install_busy;
        let action = state.and_then(target_action);
        let button = match (busy, action) {
            (Some((busy_target, busy_action)), _) if busy_target == target => {
                inert_button(target.label(), busy_action.busy_label()).into_any_element()
            }
            (None, Some(action)) => small_button(target.label(), action.label())
                .on_click(
                    cx.listener(move |this, _, _, cx| this.run_install_action(target, action, cx)),
                )
                .into_any_element(),
            _ => inert_button(target.label(), InstallAction::Install.label()).into_any_element(),
        };
        v_flex()
            .w_full()
            .child(
                h_flex()
                    .w_full()
                    .px(px(20.))
                    .py(px(8.))
                    .gap(px(12.))
                    .items_center()
                    .child(
                        div()
                            .size(px(28.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(7.))
                            .bg(theme::surface_raised())
                            .text_size(px(11.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme::text_soft())
                            .child(target_logo(target)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::text())
                                    .child(target.label()),
                            )
                            .child(install_detail(target, state)),
                    )
                    .child(button),
            )
            .children(self.install_errors.get(&target).map(|message| {
                div()
                    .px(px(20.))
                    .pb(px(4.))
                    .pl(px(60.))
                    .text_size(px(12.))
                    .text_color(theme::red_soft())
                    .child(message.clone())
            }))
    }

    fn install_tab(&self, cx: &mut Context<Self>) -> Div {
        let url = agent_api::endpoint_url(agent_api::policy(cx).port);
        let running = agent_api::handle(cx)
            .is_some_and(|api| matches!(api.read(cx).status(), ServerStatus::Running(_)));
        let rows: Vec<Div> = Target::ALL
            .into_iter()
            .enumerate()
            .map(|(index, target)| self.install_row(index, target, cx))
            .collect();
        v_flex()
            .w_full()
            .pt(px(8.))
            .children(rows)
            .child(caption("MANUAL"))
            .child(self.field_row(
                "URL",
                url.clone(),
                vec![self.copy_button("mcp-manual-copy", Copied::Manual, url, cx)],
                cx,
            ))
            .when(running && self.wsl_unreachable, |tab| {
                tab.child(wsl_warning())
            })
    }
}
