#[derive(Debug, Clone)]
pub struct EmbedSpec {
    pub parent: isize,
    pub url: String,
    pub background: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbedEvent {
    Navigated(String),
    NewWindow(String),
    Loaded,
    Closed,
}
