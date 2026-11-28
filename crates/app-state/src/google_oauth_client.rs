//! Saved Google OAuth Desktop client (ADR 0024).
//!
//! The client id lives in store meta. The secret lives in the OS keyring and is
//! never written to the calendar account document or to logs.

use store::Store;

use chatsvc::GoogleOAuth;

pub const CLIENT_ID_KEY: &str = "calendar.google_client_id";
const SERVICE: &str = "rusty-teams.google-oauth";
const ACCOUNT: &str = "desktop";

pub enum GoogleClientSave {
    Saved,
    Cleared,
}

pub enum GoogleClientSaveError {
    /// An id was given without a secret, and no secret is stored for that id.
    SecretRequired,
    Failed,
}

pub fn load_client_id(store: &Store) -> String {
    store
        .meta(CLIENT_ID_KEY)
        .ok()
        .flatten()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
}

pub async fn secret_is_stored() -> bool {
    load_secret()
        .await
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

/// Saved pair, then the environment, then the client compiled into the build.
pub async fn resolve(store: &Store) -> Option<GoogleOAuth> {
    let stored_id = load_client_id(store);
    // Without a saved id the keyring is never asked, so Secret Service stays out of every fetch.
    let stored_secret = if stored_id.is_empty() {
        String::new()
    } else {
        match load_secret().await {
            Ok(secret) => secret,
            Err(_) => {
                log::warn!("saved Google client secret not read from the keyring");
                String::new()
            }
        }
    };
    chatsvc::client_from_stored(Some(stored_id.as_str()), Some(stored_secret.as_str()))
}

pub async fn save(
    store: &Store,
    client_id: &str,
    secret: &str,
) -> Result<GoogleClientSave, GoogleClientSaveError> {
    let id = client_id.trim();
    let secret = secret.trim();
    if id.is_empty() {
        store
            .set_meta(CLIENT_ID_KEY, "")
            .map_err(|_| GoogleClientSaveError::Failed)?;
        delete_secret().await?;
        return Ok(GoogleClientSave::Cleared);
    }
    if secret.is_empty() {
        // The kept secret belongs to the saved id; a new id needs its own secret.
        if load_client_id(store) != id {
            return Err(GoogleClientSaveError::SecretRequired);
        }
        return match load_secret().await {
            Ok(existing) if !existing.trim().is_empty() => Ok(GoogleClientSave::Saved),
            _ => Err(GoogleClientSaveError::SecretRequired),
        };
    }
    // Secret before id: a failed keyring write must not leave a new id beside the old secret.
    store_secret(secret).await?;
    store
        .set_meta(CLIENT_ID_KEY, id)
        .map_err(|_| GoogleClientSaveError::Failed)?;
    Ok(GoogleClientSave::Saved)
}

async fn load_secret() -> Result<String, GoogleClientSaveError> {
    spawn_blocking(move || {
        let entry =
            keyring::Entry::new(SERVICE, ACCOUNT).map_err(|_| GoogleClientSaveError::Failed)?;
        entry
            .get_password()
            .map_err(|_| GoogleClientSaveError::Failed)
    })
    .await
}

async fn store_secret(secret: &str) -> Result<(), GoogleClientSaveError> {
    let secret = secret.to_owned();
    spawn_blocking(move || {
        let entry =
            keyring::Entry::new(SERVICE, ACCOUNT).map_err(|_| GoogleClientSaveError::Failed)?;
        entry
            .set_password(&secret)
            .map_err(|_| GoogleClientSaveError::Failed)
    })
    .await
}

async fn delete_secret() -> Result<(), GoogleClientSaveError> {
    spawn_blocking(move || {
        let entry =
            keyring::Entry::new(SERVICE, ACCOUNT).map_err(|_| GoogleClientSaveError::Failed)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(GoogleClientSaveError::Failed),
        }
    })
    .await
}

async fn spawn_blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, GoogleClientSaveError> + Send + 'static,
) -> Result<T, GoogleClientSaveError> {
    match tokio::task::spawn_blocking(work).await {
        Ok(result) => result,
        Err(_) => Err(GoogleClientSaveError::Failed),
    }
}
