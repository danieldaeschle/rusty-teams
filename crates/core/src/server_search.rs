use chatsvc::substrate_search::{
    QueryRequest, QueryResults, SearchIdentity, SearchSession, Suggestions,
};

use crate::engine::SyncEngine;
use crate::error::Result;
use crate::remote::Remote;

impl<R: Remote> SyncEngine<R> {
    pub async fn server_search_init(&self, search_session: &SearchSession) -> Result<()> {
        let identity = self.search_identity().await?;
        self.remote
            .server_search_init(&identity, search_session)
            .await
    }

    /// An empty text returns the zero-query state (top people, recent searches).
    pub async fn server_suggestions(
        &self,
        search_session: &SearchSession,
        text: &str,
    ) -> Result<Suggestions> {
        let identity = self.search_identity().await?;
        self.remote
            .server_suggestions(&identity, search_session, text)
            .await
    }

    pub async fn server_query(
        &self,
        search_session: &SearchSession,
        request: &QueryRequest,
    ) -> Result<QueryResults> {
        let identity = self.search_identity().await?;
        self.remote
            .server_query(&identity, search_session, request)
            .await
    }

    async fn search_identity(&self) -> Result<SearchIdentity> {
        Ok(SearchIdentity {
            user_id: self.my_user_id().await?,
            tenant_id: self.my_tenant_id_from_any_chat().await?,
        })
    }
}
