mod channels;
mod chats;
mod folders;
mod pinbar;
mod posts;
mod state;
mod teams;
mod transport;

use session::Session;

pub use folders::{Folder, FolderKind, Folders};
pub use pinbar::PinnedMessage;
pub use posts::{PINNED_POSTS_PAGE_SIZE, PinnedPostsPage, parse_pinned_posts};
pub use state::{PinnedChannels, PinnedChats};
pub use teams::{ChannelLayout, ChannelTab, TeamLayout, parse_team_layout};
pub use transport::{CSA_RESOURCE, CSA_SCOPE, CsaTransport, SessionTransport, SkypeTokenTransport};

pub const DEFAULT_REGION: &str = "emea";

pub struct Pins<T: CsaTransport = SessionTransport> {
    transport: T,
    base_url: String,
    api_url: String,
}

impl Pins<SessionTransport> {
    pub fn new(session: &Session) -> Self {
        Self::with_region(session, DEFAULT_REGION)
    }

    pub fn with_region(session: &Session, region: &str) -> Self {
        Pins::with_transport(SessionTransport::new(session), region)
    }
}

impl Pins<SkypeTokenTransport> {
    pub fn with_skype_token(session: &Session) -> Self {
        Pins::with_transport(SkypeTokenTransport::new(session), DEFAULT_REGION)
    }
}

impl<T: CsaTransport> Pins<T> {
    pub fn with_transport(transport: T, region: &str) -> Self {
        Pins {
            transport,
            base_url: format!(
                "https://teams.cloud.microsoft/api/csa/{region}/api/v1/teams/users/me"
            ),
            api_url: format!("https://teams.cloud.microsoft/api/csa/{region}/api/v1"),
        }
    }
}
