mod auth;
mod hub;
pub mod install;
mod policy;
mod server;
mod settings;
mod source;
mod tools;

pub use hub::{AgentHub, CaptionLine, Event, EventKind, Outcome, Proposal, UiEvent};
pub use policy::{Access, AgentPolicy, ConversationKind, DEFAULT_PORT};
pub use server::{ServerError, ServerHandle, start};
pub use settings::{
    load as load_policy, load_or_create_token, regenerate_token, save as save_policy,
};
pub use source::{Meeting, SourceError, SourceFuture, TeamsSource, Transcript, TranscriptTurn};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpEndpoint {
    pub url: String,
    pub token: String,
}
