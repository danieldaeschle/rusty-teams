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
    /// A complete email that the people index misses is resolved through `search_people`.
    pub async fn server_suggestions(
        &self,
        search_session: &SearchSession,
        text: &str,
    ) -> Result<Suggestions> {
        self.collect_suggestions(search_session, text, true).await
    }

    /// People-index suggestions without the complete-email directory lookup.
    /// The new-chat field calls `search_people` itself.
    pub async fn server_suggestions_without_email_lookup(
        &self,
        search_session: &SearchSession,
        text: &str,
    ) -> Result<Suggestions> {
        self.collect_suggestions(search_session, text, false).await
    }

    async fn collect_suggestions(
        &self,
        search_session: &SearchSession,
        text: &str,
        resolve_email: bool,
    ) -> Result<Suggestions> {
        let identity = self.search_identity().await?;
        let mut suggestions = self
            .remote
            .server_suggestions(&identity, search_session, text)
            .await?;
        if resolve_email
            && crate::native::is_email_address(text)
            && !has_person_with_email(&suggestions, text)
        {
            let external = self.search_people(text).await.unwrap_or_default();
            suggestions.people.extend(
                external
                    .into_iter()
                    .filter_map(|user| search_person(user, text)),
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
    let email = email.trim();
    suggestions.people.iter().any(|person| {
        person
            .email_addresses
            .iter()
            .any(|address| address.eq_ignore_ascii_case(email))
            || person
                .user_principal_name
                .as_deref()
                .is_some_and(|address| address.eq_ignore_ascii_case(email))
    })
}

fn search_person(user: graph::User, query: &str) -> Option<SearchPerson> {
    if !crate::native::user_has_address(
        user.mail.as_deref(),
        user.user_principal_name.as_deref(),
        query,
    ) {
        return None;
    }
    let email = user
        .mail
        .clone()
        .or_else(|| user.user_principal_name.clone());
    Some(SearchPerson {
        company_name: crate::native::known_profile(&user.id)
            .and_then(|profile| profile.tenant_name),
        mri: Some(format!("8:orgid:{}", user.id)),
        email_addresses: email.into_iter().collect(),
        display_name: user.display_name.unwrap_or_default(),
        user_principal_name: user.user_principal_name,
        user_id: Some(user.id),
        ..SearchPerson::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_principal_name_still_becomes_a_person_row() {
        let user = graph::User {
            id: "ada".into(),
            display_name: Some("Ada".into()),
            mail: None,
            user_principal_name: Some("ada@other.example".into()),
            job_title: None,
            department: None,
        };
        let person = search_person(user, "Ada@Other.Example").unwrap();
        assert_eq!(person.user_id.as_deref(), Some("ada"));
        assert_eq!(person.email_addresses, ["ada@other.example"]);
    }
}
