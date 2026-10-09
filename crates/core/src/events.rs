#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreEvent {
    SidebarChanged,
    MessagesChanged { conversation_id: String },
    ReceiptsChanged { conversation_id: String },
    PinsChanged { conversation_id: String },
    AvatarsChanged { user_ids: Vec<String> },
    ImagesChanged { keys: Vec<String> },
    FoldersChanged,
    PresenceChanged,
    Error { message: String },
}
