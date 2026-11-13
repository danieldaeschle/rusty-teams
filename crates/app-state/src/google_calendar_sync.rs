use chrono::{DateTime, Utc};
use store::{CalendarAccounts, GoogleTokenSet};

use chatsvc::{
    CalendarEvent, GoogleCalendar, GoogleEventDraft, GoogleOAuth, Reply, client_id_from_env,
    google_rsvp_status, refresh_access_token,
};

/// Refresh tokens when needed and load events from every selected Google calendar.
pub async fn fetch_google_events(
    accounts: &mut CalendarAccounts,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<CalendarEvent>, String> {
    let Some(oauth) = client_id_from_env() else {
        // Connected accounts but no client id compiled in: say so instead of an empty week.
        if accounts.has_google() {
            return Err(missing_client_id());
        }
        return Ok(Vec::new());
    };
    let mut all = Vec::new();
    for index in 0..accounts.google_accounts.len() {
        if !ensure_access_token(accounts, index, &oauth).await? {
            continue;
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
        let calendars = if calendars.is_empty() {
            vec![("primary".to_owned(), "Primary".to_owned())]
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
    Ok(all)
}

pub async fn insert_google_event(
    accounts: &mut CalendarAccounts,
    account_id: &str,
    calendar_id: &str,
    draft: &GoogleEventDraft,
) -> Result<(), String> {
    let oauth = client_id_from_env().ok_or_else(|| missing_client_id())?;
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
    accounts: &mut CalendarAccounts,
    account_id: &str,
    calendar_id: &str,
    event_id: &str,
    reply: Reply,
    send_updates: bool,
) -> Result<(), String> {
    let oauth = client_id_from_env().ok_or_else(|| missing_client_id())?;
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
    accounts: &mut CalendarAccounts,
    account_id: &str,
    calendar_id: &str,
    event_id: &str,
    send_updates: bool,
) -> Result<(), String> {
    let oauth = client_id_from_env().ok_or_else(|| missing_client_id())?;
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
                crate::diagnostics::record(format!("calls: google token refresh failed: {error}"));
                return Ok(false);
            }
        }
    }
    Ok(!account.tokens.access_token.is_empty())
}

fn missing_client_id() -> String {
    app_base::ui_locale::tr("settings.calendar.not_in_build")
}
