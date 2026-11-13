mod auth;
mod hub;
pub mod install;
mod meetings;
mod policy;
mod remote;
mod server;
mod settings;
mod source;
mod tools;

pub use hub::{
    AgentHub, CaptionLine, Event, EventKind, FreeWriting, FreeWritingAsk, MeetingOutcome,
    MeetingProposal, NewProposal, NewReactionProposal, Outcome, Proposal, ProposalImage,
    ProposalMention, ReactionOutcome, ReactionProposal, UiEvent, Wakeup,
};
pub use policy::{Access, AgentPolicy, ConversationKind, DEFAULT_PORT};
pub use remote::{ListedTool, ToolSession};
pub use server::{ServerError, ServerHandle, start};
pub use settings::{
    load as load_policy, load_or_create_token, regenerate_token, save as save_policy,
};
pub use source::{
    BusySlot, CreatedMeeting, EncodedImage, FileInfo, ForwardedInfo, ImageInfo, ImageKind,
    LinkInfo, Meeting, MeetingDraft, MentionInfo, MessageExtras, PersonAvailability, QuoteInfo,
    ReactionInfo, SourceError, SourceFuture, TeamsSource, Transcript, TranscriptTurn,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpEndpoint {
    pub url: String,
    pub token: String,
}
