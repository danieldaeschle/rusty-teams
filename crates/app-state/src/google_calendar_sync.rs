use chrono::{DateTime, Utc};
use store::{CalendarAccounts, GoogleTokenSet, Store};

use chatsvc::{
    CalendarEvent, GoogleCalendar, GoogleEventDraft, GoogleOAuth, Reply, google_rsvp_status,
    refresh_access_token,
};

/// Refresh tokens when needed and load events from every selected Google calendar.
pub async fn fetch_google_events(
    store: &Store,
    accounts: &mut CalendarAccounts,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<(Vec<CalendarEvent>, Vec<String>), String> {
    let Some(oauth) = crate::google_oauth_client::resolve(store).await else {
        // Connected accounts but no client id compiled in: say so instead of an empty week.
        if accounts.has_google() {
            return Err(missing_client_id());
        }
        return Ok((Vec::new(), Vec::new()));
    };
    let mut all = Vec::new();
    // One rejected token must not hide calendars that still refresh.
    let mut account_errors = Vec::new();
    for index in 0..accounts.google_accounts.len() {
        match ensure_access_token(accounts, index, &oauth).await {
            Ok(true) => {}
            Ok(false) => continue,
            Err(error) => {
                remember_account_error(&mut account_errors, error);
                continue;
            }
        }
        let account = &accounts.google_accounts[index];
        let access = account.tokens.access_token.clone();
        let account_id = account.id.clone();
        let calendars: Vec<(String, String)> = account
            .calendars
            .iter()
            .filter(|calendar| calendar.selected)
            .map(|calendar| (calendar.id.clone(), calendar.summary.clone()))
            .collect();
        // Every calendar turned off (group check box): the account shows nothing.
        if calendars.is_empty() && !account.calendars.is_empty() {
            continue;
        }
        let calendars = if calendars.is_empty() {
            vec![(
                "primary".to_owned(),
                app_base::ui_locale::tr("settings.calendar.primary"),
            )]
        } else {
            calendars
        };
        let client = GoogleCalendar::new(access);
        for (calendar_id, calendar_name) in calendars {
            match client.events(&calendar_id, start, end).await {
                Ok(mut events) => {
                    for event in &mut events {
                        event.account_id = Some(account_id.clone());
                        event.calendar_id = Some(calendar_id.clone());
                        event.calendar_name = Some(calendar_name.clone());
                    }
                    all.extend(events);
                }
                Err(error) => crate::diagnostics::record(format!(
                    "calls: google calendar {calendar_id} not loaded: {error}"
                )),
            }
        }
    }
    Ok((all, account_errors))
}

/// Keeps a sign-in failure ahead of any other account error. One notice is enough.
fn remember_account_error(errors: &mut Vec<String>, error: String) {
    if errors.iter().any(|existing| needs_sign_in(existing)) && !needs_sign_in(&error) {
        return;
    }
    if needs_sign_in(&error) {
        errors.clear();
    }
    if errors.is_empty() {
        errors.push(error);
    }
}

pub async fn insert_google_event(
    store: &Store,
    accounts: &mut CalendarAccounts,
    account_id: &str,
    calendar_id: &str,
    draft: &GoogleEventDraft,
) -> Result<(), String> {
    let oauth = crate::google_oauth_client::resolve(store)
        .await
        .ok_or_else(|| missing_client_id())?;
    let index = accounts
        .google_accounts
        .iter()
        .position(|account| account.id == account_id)
        .ok_or_else(|| format!("Google account {account_id} missing"))?;
    if !ensure_access_token(accounts, index, &oauth).await? {
        return Err("Google access token missing".into());
    }
    let access = accounts.google_accounts[index].tokens.access_token.clone();
    GoogleCalendar::new(access)
        .insert_event(calendar_id, draft)
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub async fn respond_google_event(
    store: &Store,
    accounts: &mut CalendarAccounts,
    account_id: &str,
    calendar_id: &str,
    event_id: &str,
    reply: Reply,
    send_updates: bool,
) -> Result<(), String> {
    let oauth = crate::google_oauth_client::resolve(store)
        .await
        .ok_or_else(|| missing_client_id())?;
    let index = accounts
        .google_accounts
        .iter()
        .position(|account| account.id == account_id)
        .ok_or_else(|| format!("Google account {account_id} missing"))?;
    if !ensure_access_token(accounts, index, &oauth).await? {
        return Err("Google access token missing".into());
    }
    let access = accounts.google_accounts[index].tokens.access_token.clone();
    GoogleCalendar::new(access)
        .respond(
            calendar_id,
            event_id,
            google_rsvp_status(reply),
            send_updates,
        )
        .await
        .map_err(|error| error.to_string())
}

pub async fn delete_google_event(
    store: &Store,
    accounts: &mut CalendarAccounts,
    account_id: &str,
    calendar_id: &str,
    event_id: &str,
    send_updates: bool,
) -> Result<(), String> {
    let oauth = crate::google_oauth_client::resolve(store)
        .await
        .ok_or_else(|| missing_client_id())?;
    let index = accounts
        .google_accounts
        .iter()
        .position(|account| account.id == account_id)
        .ok_or_else(|| format!("Google account {account_id} missing"))?;
    if !ensure_access_token(accounts, index, &oauth).await? {
        return Err("Google access token missing".into());
    }
    let access = accounts.google_accounts[index].tokens.access_token.clone();
    GoogleCalendar::new(access)
        .delete_event(calendar_id, event_id, send_updates)
        .await
        .map_err(|error| error.to_string())
}

async fn ensure_access_token(
    accounts: &mut CalendarAccounts,
    index: usize,
    oauth: &GoogleOAuth,
) -> Result<bool, String> {
    let now = Utc::now().timestamp();
    let account = &mut accounts.google_accounts[index];
    if account.tokens.refresh_token.is_empty() && account.tokens.access_token.is_empty() {
        return Ok(false);
    }
    if account.tokens.expires_at <= now + 60 && !account.tokens.refresh_token.is_empty() {
        match refresh_access_token(oauth, &account.tokens.refresh_token).await {
            Ok(bundle) => {
                account.tokens = GoogleTokenSet {
                    access_token: bundle.access_token,
                    refresh_token: if bundle.refresh_token.is_empty() {
                        account.tokens.refresh_token.clone()
                    } else {
                        bundle.refresh_token
                    },
                    expires_at: now + bundle.expires_in.max(60),
                };
            }
            Err(error) => {
                let message = format!("google token refresh failed: {error}");
                crate::diagnostics::record(format!("calls: {message}"));
                // Surface to week load as CalendarLoadFailure — never Ok([]) for auth fail.
                return Err(message);
            }
        }
    }
    Ok(!account.tokens.access_token.is_empty())
}

fn missing_client_id() -> String {
    app_base::ui_locale::tr("settings.calendar.not_in_build")
}

/// True when another fetch cannot succeed until the user signs in to Google again.
pub fn needs_sign_in(message: &str) -> bool {
    message.starts_with("google token refresh failed:") || message == missing_client_id()
}

#[cfg(test)]
mod tests {
    use super::{needs_sign_in, remember_account_error};

    #[test]
    fn a_rejected_token_does_not_hide_another_accounts_error_notice() {
        let mut errors = Vec::new();
        remember_account_error(&mut errors, "HTTP 503".into());
        remember_account_error(
            &mut errors,
            "google token refresh failed: HTTP 400: invalid_grant".into(),
        );
        remember_account_error(&mut errors, "HTTP 500".into());
        assert_eq!(errors.len(), 1);
        assert!(needs_sign_in(&errors[0]));
    }

    #[test]
    fn google_sign_in_is_required_only_for_a_rejected_token() {
        assert!(needs_sign_in(
            "google token refresh failed: HTTP 400: invalid_grant"
        ));
        assert!(!needs_sign_in("HTTP 503"));
        assert!(!needs_sign_in("client_secret is missing"));
    }
}
