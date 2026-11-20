use chatsvc::substrate_search::{
    QueryRequest, QueryResults, SearchIdentity, SearchPerson, SearchSession, Suggestions,
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
        let mut suggestions = self
            .remote
            .server_suggestions(&identity, search_session, text)
            .await?;
        if crate::native::is_email_address(text) && !has_person_with_email(&suggestions, text) {
            let external = self.search_people(text).await.unwrap_or_default();
            suggestions.people.extend(
                external
                    .into_iter()
                    .filter(|user| {
                        user.mail
                            .as_deref()
                            .is_some_and(|mail| mail.eq_ignore_ascii_case(text.trim()))
                    })
                    .map(|user| SearchPerson {
                        company_name: crate::native::known_profile(&user.id)
                            .and_then(|profile| profile.tenant_name),
                        mri: Some(format!("8:orgid:{}", user.id)),
                        email_addresses: user.mail.into_iter().collect(),
                        display_name: user.display_name.unwrap_or_default(),
                        user_principal_name: user.user_principal_name,
                        user_id: Some(user.id),
                        ..SearchPerson::default()
                    }),
            );
        }
        Ok(suggestions)
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

fn has_person_with_email(suggestions: &Suggestions, email: &str) -> bool {
    suggestions.people.iter().any(|person| {
        person
            .email_addresses
            .iter()
            .any(|address| address.eq_ignore_ascii_case(email.trim()))
    })
}
