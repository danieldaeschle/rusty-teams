//! OS keyring storage for CalDAV app passwords (ADR 0019).

const SERVICE: &str = "rusty-teams.caldav";

pub fn store_password(account_id: &str, password: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, account_id).map_err(|e| e.to_string())?;
    entry.set_password(password).map_err(|e| e.to_string())
}

pub fn load_password(account_id: &str) -> Result<String, String> {
    let entry = keyring::Entry::new(SERVICE, account_id).map_err(|e| e.to_string())?;
    entry.get_password().map_err(|e| e.to_string())
}

pub fn delete_password(account_id: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, account_id).map_err(|e| e.to_string())?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

pub async fn store_password_async(account_id: String, password: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || store_password(&account_id, &password))
        .await
        .map_err(|error| error.to_string())?
}

pub async fn load_password_async(account_id: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || load_password(&account_id))
        .await
        .map_err(|error| error.to_string())?
}

pub async fn delete_password_async(account_id: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || delete_password(&account_id))
        .await
        .map_err(|error| error.to_string())?
}

/// Redact credentials that must never appear in diagnostics or support bundles.
pub fn redact_secret_text(text: &str) -> String {
    let mut out = text.to_owned();
    // user:pass@host
    if let Some(scheme_end) = out.find("://") {
        let after = scheme_end + 3;
        if let Some(at) = out[after..].find('@') {
            let creds = &out[after..after + at];
            if creds.contains(':') {
                out.replace_range(after..after + at, "***:***");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_userinfo_in_urls() {
        let scrubbed = redact_secret_text("GET https://joe:s3cret@cloud.example/dav/ ok");
        assert!(!scrubbed.contains("s3cret"));
        assert!(scrubbed.contains("***:***@cloud.example"));
    }
}
