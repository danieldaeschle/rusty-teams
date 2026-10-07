pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Graph(#[from] graph::Error),
    #[error(transparent)]
    Store(#[from] store::Error),
    #[error("folder service: {0}")]
    Folders(String),
    #[error("unknown conversation {0}")]
    UnknownConversation(String),
    #[error("{0} is not supported yet")]
    Unsupported(&'static str),
}

impl From<chatsvc::Error> for Error {
    fn from(error: chatsvc::Error) -> Self {
        match error {
            chatsvc::Error::Session(inner) => Error::Graph(graph::Error::Session(inner)),
            other => Error::Folders(other.to_string()),
        }
    }
}
