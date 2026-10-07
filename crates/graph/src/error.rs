pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Session(#[from] session::Error),
    #[error("unexpected Graph answer: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("refusing to follow a next link outside Graph")]
    ForeignNextLink,
    #[error("cannot decode a binary answer: {0}")]
    Decode(String),
}
