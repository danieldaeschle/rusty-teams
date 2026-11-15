//! The organizations the signed-in account belongs to, as the Teams org switcher lists them.
use serde_json::Value;
use session::{Method, Session};

use crate::cards::{DEFAULT_MT_REGION, spaces_scope};
use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tenant {
    pub id: String,
    pub name: String,
    /// The account is a guest here; its home tenant is the one with `guest == false`.
    pub guest: bool,
    /// The account's user id in this tenant; a guest has its own id in every tenant.
    pub user_id: Option<String>,
}

/// Every tenant of the account, home tenant first.
pub async fn list_tenants(session: &Session) -> Result<Vec<Tenant>> {
    let url =
        format!("https://teams.cloud.microsoft/api/mt/{DEFAULT_MT_REGION}/beta/users/tenants");
    let answer = session
        .request(Method::Get, &url, &spaces_scope(), None)
        .await?;
    parse_tenants(&answer.body)
        .ok_or_else(|| Error::UnexpectedAnswer(format!("{url}: no tenant list")))
}

fn parse_tenants(body: &Value) -> Option<Vec<Tenant>> {
    let mut tenants: Vec<Tenant> = body
        .as_array()?
        .iter()
        .filter_map(|entry| {
            Some(Tenant {
                id: entry["tenantId"].as_str()?.to_ascii_lowercase(),
                name: entry["tenantName"].as_str()?.to_owned(),
                guest: entry["userType"]
                    .as_str()
                    .is_some_and(|kind| kind.eq_ignore_ascii_case("guest")),
                user_id: entry["userId"].as_str().map(str::to_ascii_lowercase),
            })
        })
        .collect();
    tenants.sort_by_key(|tenant| tenant.guest);
    Some(tenants)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lists_the_home_tenant_first_and_marks_guest_tenants() {
        let body = json!([
            {"tenantId": "BBBB-GUEST", "tenantName": "Guest Org", "userType": "guest", "userId": "AB-12"},
            {"tenantId": "aaaa-home", "tenantName": "Home Org", "userType": "member"},
            {"tenantName": "no id"},
        ]);
        assert_eq!(
            parse_tenants(&body).unwrap(),
            vec![
                Tenant {
                    id: "aaaa-home".into(),
                    name: "Home Org".into(),
                    guest: false,
                    user_id: None,
                },
                Tenant {
                    id: "bbbb-guest".into(),
                    name: "Guest Org".into(),
                    guest: true,
                    user_id: Some("ab-12".into()),
                },
            ]
        );
        assert_eq!(parse_tenants(&json!({"error": 1})), None);
    }
}
