mod channels;
mod chats;
mod folders;
mod state;
mod teams;
mod transport;

use session::Session;

pub use folders::{Folder, FolderKind, Folders};
pub use state::{PinnedChannels, PinnedChats};
pub use teams::{ChannelLayout, TeamLayout};
pub use transport::{CSA_RESOURCE, CSA_SCOPE, CsaTransport, SessionTransport};

pub const DEFAULT_REGION: &str = "emea";

pub struct Pins<T: CsaTransport = SessionTransport> {
    transport: T,
    base_url: String,
}

impl Pins<SessionTransport> {
    pub fn new(session: &Session) -> Self {
        Self::with_region(session, DEFAULT_REGION)
    }

    pub fn with_region(session: &Session, region: &str) -> Self {
        Pins::with_transport(SessionTransport::new(session), region)
    }
}

impl<T: CsaTransport> Pins<T> {
    pub fn with_transport(transport: T, region: &str) -> Self {
        Pins {
            transport,
            base_url: format!(
                "https://teams.cloud.microsoft/api/csa/{region}/api/v1/teams/users/me"
            ),
        }
    }
}
