mod bridge;
mod source;

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use chrono::{DateTime, Local};
use gpui_kit::*;
use mcp::{Access, AgentHub, AgentPolicy, McpEndpoint, Outcome, Proposal, ServerHandle};
use store::Store;

use crate::backend::Engine;
use crate::runtime;
use source::AppSource;

#[derive(Debug, Clone, PartialEq)]
pub enum ProposalStatus {
    Pending,
    Stale { answered_at: DateTime<Local> },
    Failed { reason: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProposalEntry {
    pub proposal: Proposal,
    pub status: ProposalStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerStatus {
    Off,
    Starting,
    Running(u16),
    PortInUse(u16),
    Failed(String),
}

pub struct AgentApi {
    hub: Arc<AgentHub>,
    source: Arc<AppSource>,
    store: Arc<Store>,
    token: String,
    proposals: HashMap<String, ProposalEntry>,
    server: Option<ServerHandle>,
    status: ServerStatus,
    generation: u64,
    reinstall_note: Option<String>,
}

struct AgentApiHandle(Entity<AgentApi>);

impl Global for AgentApiHandle {}

const DEMO_PORT_ENV: &str = "RUSTY_TEAMS_MCP_PORT";

pub fn endpoint_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/mcp")
}

pub fn agent_label(agent: Option<&str>) -> &'static str {
    match agent.map(str::to_ascii_lowercase).as_deref() {
        Some("claude") => "Claude",
        Some("copilot") => "Copilot",
        Some("codex") => "Codex",
        _ => "Agent",
    }
}

pub fn init(store: Arc<Store>, demo: bool, cx: &mut App) {
    let (ui, receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut policy = mcp::load_policy(&store);
    if demo
        && let Some(port) = std::env::var(DEMO_PORT_ENV)
            .ok()
            .and_then(|value| value.parse().ok())
    {
        policy.port = port;
    }
    let hub = AgentHub::new(store.clone(), policy, ui);
    let token = mcp::load_or_create_token(&store).unwrap_or_default();
    if demo {
        println!("mcp: {} token={token}", endpoint_url(hub.policy().port));
    }
    let source = Arc::new(AppSource::new(store.clone()));
    let api = cx.new(|_| AgentApi {
        hub,
        source,
        store,
        token,
        proposals: HashMap::new(),
        server: None,
        status: ServerStatus::Off,
        generation: 0,
        reinstall_note: None,
    });
    bridge::drain_ui_events(receiver, api.downgrade(), cx);
    api.update(cx, |api, cx| {
        let _ = api.hub.restore_pending();
        api.reconcile(cx);
    });
    cx.set_global(AgentApiHandle(api));
}

pub fn engine_ready(engine: Arc<Engine>, cx: &mut App) {
    let Some(api) = handle(cx) else {
        return;
    };
    let api = api.read(cx);
    if api.source.set_engine(engine.clone()) {
        bridge::spawn_message_bridge(engine, api.hub.clone(), api.source.clone());
    }
}

pub fn handle(cx: &App) -> Option<Entity<AgentApi>> {
    cx.try_global::<AgentApiHandle>()
        .map(|handle| handle.0.clone())
}

pub fn mcp_endpoint(cx: &App) -> Option<McpEndpoint> {
    let api = handle(cx)?.read(cx);
    match api.status {
        ServerStatus::Running(port) => Some(McpEndpoint {
            url: endpoint_url(port),
            token: api.token.clone(),
        }),
        _ => None,
    }
}

pub fn configured_endpoint(cx: &App) -> Option<McpEndpoint> {
    let api = handle(cx)?.read(cx);
    Some(McpEndpoint {
        url: endpoint_url(api.hub.policy().port),
        token: api.token.clone(),
    })
}

pub fn policy(cx: &App) -> AgentPolicy {
    handle(cx)
        .map(|api| api.read(cx).hub.policy())
        .unwrap_or_default()
}

pub fn chat_allowed(conversation_id: &str, cx: &App) -> bool {
    let Some(api) = handle(cx) else {
        return false;
    };
    let api = api.read(cx);
    let policy = api.hub.policy();
    policy.propose && policy.can_read_any_kind(conversation_id)
}

pub fn allow_chat(conversation_id: &str, cx: &mut App) {
    let Some(api) = handle(cx) else {
        return;
    };
    let conversation_id = conversation_id.to_owned();
    api.update(cx, |api, cx| {
        let is_channel = api.store.channel(&conversation_id).ok().flatten().is_some();
        api.update_policy(
            move |policy| {
                policy.enabled = true;
                policy.propose = true;
                let access = if is_channel {
                    &mut policy.channels
                } else {
                    &mut policy.chats
                };
                match access {
                    Access::All => {}
                    Access::Selected(ids) => {
                        ids.insert(conversation_id);
                    }
                    Access::None => *access = Access::Selected(BTreeSet::from([conversation_id])),
                }
            },
            cx,
        );
    });
}

pub fn update_policy(cx: &mut App, change: impl FnOnce(&mut AgentPolicy)) {
    if let Some(api) = handle(cx) {
        api.update(cx, |api, cx| api.update_policy(change, cx));
    }
}

pub fn proposal(conversation_id: &str, cx: &App) -> Option<ProposalEntry> {
    handle(cx)?.read(cx).proposals.get(conversation_id).cloned()
}

pub fn pending_agents(cx: &App) -> HashMap<String, &'static str> {
    let Some(api) = handle(cx) else {
        return HashMap::new();
    };
    api.read(cx)
        .proposals
        .iter()
        .filter(|(_, entry)| !matches!(entry.status, ProposalStatus::Stale { .. }))
        .map(|(conversation_id, entry)| {
            (
                conversation_id.clone(),
                agent_label(entry.proposal.agent.as_deref()),
            )
        })
        .collect()
}

pub fn resolve_proposal(conversation_id: &str, outcome: Outcome, cx: &mut App) {
    if let Some(api) = handle(cx) {
        api.update(cx, |api, cx| api.resolve(conversation_id, outcome, cx));
    }
}

pub fn fail_proposal(conversation_id: &str, reason: String, cx: &mut App) {
    if let Some(api) = handle(cx) {
        api.update(cx, |api, cx| {
            if let Some(entry) = api.proposals.get_mut(conversation_id) {
                entry.status = ProposalStatus::Failed { reason };
                cx.notify();
            }
        });
    }
}

pub fn user_typing(conversation_id: &str, cx: &App) {
    if let Some(api) = handle(cx) {
        api.read(cx).hub.user_typing(conversation_id);
    }
}

pub fn user_sent(conversation_id: &str, cx: &App) {
    if let Some(api) = handle(cx) {
        api.read(cx).hub.user_sent(conversation_id);
    }
}

pub fn note_caption(meeting_chat_id: &str, caption_id: &str, speaker: &str, text: &str, cx: &App) {
    let Some(api) = handle(cx) else {
        return;
    };
    let hub = &api.read(cx).hub;
    let policy = hub.policy();
    if policy.enabled && policy.transcripts {
        hub.push_caption(meeting_chat_id, caption_id, speaker, text);
    }
}

pub fn clear_captions(meeting_chat_id: &str, cx: &App) {
    if let Some(api) = handle(cx) {
        api.read(cx).hub.clear_captions(meeting_chat_id);
    }
}

impl AgentApi {
    pub fn status(&self) -> &ServerStatus {
        &self.status
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn regenerate_token(&mut self, cx: &mut Context<Self>) {
        if let Ok(token) = mcp::regenerate_token(&self.store) {
            self.token = token;
            self.reinstall_agent_configs(cx);
            if self.server.is_some() {
                self.start_server(cx);
            }
            cx.notify();
        }
    }

    pub fn reinstall_note(&self) -> Option<&str> {
        self.reinstall_note.as_deref()
    }

    fn reinstall_agent_configs(&mut self, cx: &mut Context<Self>) {
        let endpoint = McpEndpoint {
            url: endpoint_url(self.hub.policy().port),
            token: self.token.clone(),
        };
        let task = cx.background_spawn(async move {
            mcp::install::reinstall_all(&endpoint, &mcp::install::Homes::detect())
        });
        cx.spawn(async move |this, cx| {
            let results = task.await;
            this.update(cx, |this, cx| {
                this.reinstall_note = results.into_iter().find_map(|(target, result)| {
                    result
                        .err()
                        .map(|error| format!("Could not update {}: {error}", target.label()))
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn update_policy(&mut self, change: impl FnOnce(&mut AgentPolicy), cx: &mut Context<Self>) {
        let mut policy = self.hub.policy();
        let previous_port = policy.port;
        change(&mut policy);
        let port_changed = policy.port != previous_port;
        mcp::save_policy(&self.store, &policy);
        self.hub.set_policy(policy);
        self.reconcile(cx);
        if port_changed {
            self.reinstall_agent_configs(cx);
        }
        cx.notify();
    }

    fn apply_ui_event(&mut self, event: mcp::UiEvent, cx: &mut Context<Self>) {
        bridge::apply(&mut self.proposals, event);
        cx.notify();
    }

    fn resolve(&mut self, conversation_id: &str, outcome: Outcome, cx: &mut Context<Self>) {
        let Some(entry) = self.proposals.remove(conversation_id) else {
            return;
        };
        if !matches!(entry.status, ProposalStatus::Stale { .. }) {
            self.hub.resolve(&entry.proposal.id, outcome);
        }
        cx.notify();
    }

    fn reconcile(&mut self, cx: &mut Context<Self>) {
        let policy = self.hub.policy();
        let wanted = policy.enabled;
        match (wanted, &self.status) {
            (false, ServerStatus::Off) => {}
            (false, _) => self.stop_server(cx),
            (true, ServerStatus::Running(port)) if *port == policy.port => {}
            (true, _) => self.start_server(cx),
        }
    }

    fn stop_server(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        if let Some(server) = self.server.take() {
            drop(runtime::spawn(server.shutdown()));
        }
        self.status = ServerStatus::Off;
        cx.notify();
    }

    fn start_server(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        let previous = self.server.take();
        let (hub, source, token) = (self.hub.clone(), self.source.clone(), self.token.clone());
        let port = self.hub.policy().port;
        self.status = ServerStatus::Starting;
        let receiver = runtime::spawn(async move {
            if let Some(previous) = previous {
                previous.shutdown().await;
            }
            mcp::start(hub, source, port, token).await
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = receiver.await else {
                return;
            };
            this.update(cx, |this, cx| {
                this.finish_start(generation, port, result, cx)
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn finish_start(
        &mut self,
        generation: u64,
        port: u16,
        result: Result<ServerHandle, mcp::ServerError>,
        cx: &mut Context<Self>,
    ) {
        if generation != self.generation {
            if let Ok(server) = result {
                drop(runtime::spawn(server.shutdown()));
            }
            return;
        }
        match result {
            Ok(server) => {
                self.server = Some(server);
                self.status = ServerStatus::Running(port);
            }
            Err(mcp::ServerError::PortInUse { port }) => {
                self.status = ServerStatus::PortInUse(port)
            }
            Err(error) => self.status = ServerStatus::Failed(error.to_string()),
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::agent_label;

    #[test]
    fn agent_labels_know_the_common_agents() {
        assert_eq!(agent_label(Some("claude")), "Claude");
        assert_eq!(agent_label(Some("Copilot")), "Copilot");
        assert_eq!(agent_label(Some("codex")), "Codex");
        assert_eq!(agent_label(Some("other")), "Agent");
        assert_eq!(agent_label(None), "Agent");
    }
}
