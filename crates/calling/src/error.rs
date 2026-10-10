#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("sdp: {0}")]
    Sdp(String),
    #[error("signaling: {0}")]
    Signaling(String),
    #[error("callback: {0}")]
    Callback(String),
    #[error("webrtc: {0}")]
    Webrtc(String),
    #[error("call cancelled")]
    Cancelled,
    #[error(transparent)]
    Session(#[from] session::Error),
    #[error(transparent)]
    Realtime(#[from] chatsvc::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
