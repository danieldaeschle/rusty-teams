use thiserror::Error;

#[derive(Debug, Error)]
pub enum BrowserError {
    #[error("no browser on {0}")]
    NoBrowser(String),
    #[error("chrome not found: {0}")]
    ChromeNotFound(String),
    #[error("cannot read Windows environment: {0}")]
    Environment(String),
    #[error("something already listens on port {0}")]
    PortBusy(u16),
    #[error("chrome did not open debugging port {port} within {seconds}s")]
    StartTimeout { port: u16, seconds: u64 },
    #[error("chrome on port {port} did not close within {seconds}s")]
    StopTimeout { port: u16, seconds: u64 },
    #[error("http: {0}")]
    Http(String),
    #[error("devtools: {0}")]
    Cdp(String),
    #[error("unexpected response: {0}")]
    Parse(String),
    #[error("launch failed: {0}")]
    Launch(#[from] std::io::Error),
}
