use session::{ApiResponse, Request, Scope, Session};

use crate::error::Result;

pub const CSA_RESOURCE: &str = "https://chatsvcagg.teams.microsoft.com";
pub const CSA_SCOPE: &str = "user_impersonation";

pub trait CsaTransport {
    fn send(&self, request: Request) -> impl std::future::Future<Output = Result<ApiResponse>>;
}

pub struct SessionTransport {
    session: Session,
    scope: Scope,
}

impl SessionTransport {
    pub fn new(session: &Session) -> Self {
        SessionTransport {
            session: session.clone(),
            scope: Scope::new(CSA_RESOURCE, CSA_SCOPE),
        }
    }
}

impl CsaTransport for SessionTransport {
    async fn send(&self, request: Request) -> Result<ApiResponse> {
        let mut answers = self.session.batch(&[request], &self.scope).await?;
        Ok(answers.remove(0))
    }
}
