pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Session(#[from] session::Error),
    #[error("unexpected service answer: {0}")]
    UnexpectedAnswer(String),
    #[error("no Favorites folder in the conversation folders")]
    NoFavoritesFolder,
    #[error("unknown folder {0}")]
    UnknownFolder(String),
    #[error("pin list changed twice while writing, giving up")]
    VersionConflict,
    #[error("invalid event payload: {0}")]
    Decode(String),
}
