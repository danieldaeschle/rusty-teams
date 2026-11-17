use chrono::{DateTime, Utc};
use store::{CalDavAccount, CalDavCalendarInfo, CalendarAccounts};
use uuid::Uuid;

use chatsvc::{
    CalDavClient, CalDavEventDraft, CalendarEvent, Reply, discover_caldav, href_from_object_id,
    new_event_uid, partstat_for_reply,
};

use crate::caldav_credentials::{delete_password, load_password_async, store_password_async};

pub async fn connect_caldav(
    accounts: &mut CalendarAccounts,
    url: &str,
    username: &str,
    password: &str,
) -> Result<String, String> {
    let url = normalize_url(url)?;
    let client = CalDavClient::new(username, password).map_err(|e| e.to_string())?;
    let discovered = discover_caldav(&client, &url)
        .await
        .map_err(|e| e.to_string())?;
    let listed = client
        .list_calendars(&discovered.home_set)
        .await
        .map_err(|e| e.to_string())?;
    if listed.is_empty() {
        return Err("No event calendars found on this CalDAV account".into());
    }
    let id = Uuid::new_v4().to_string();
    store_password_async(id.clone(), password.to_owned()).await?;
    accounts.caldav_accounts.push(CalDavAccount {
        id: id.clone(),
        url,
        username: username.to_owned(),
        display_name: username.to_owned(),
        calendars: listed
            .into_iter()
            .map(|calendar| CalDavCalendarInfo {
                id: calendar.href,
                summary: calendar.summary,
                selected: true,
                writable: calendar.writable,
                color: calendar.color,
            })
            .collect(),
        user_addresses: discovered.user_addresses,
        schedule_outbox: discovered.schedule_outbox,
    });
    Ok(id)
}

pub fn disconnect_caldav(accounts: &mut CalendarAccounts, account_id: &str) -> Result<(), String> {
    delete_password(account_id)?;
    accounts.disconnect_caldav(account_id);
    Ok(())
}

/// Events plus per-calendar failure messages. One bad calendar does not drop the rest.
pub async fn fetch_caldav_events(
    accounts: &mut CalendarAccounts,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<(Vec<CalendarEvent>, Vec<String>), String> {
    let mut all = Vec::new();
    let mut failures = Vec::new();
    for index in 0..accounts.caldav_accounts.len() {
        let account = &accounts.caldav_accounts[index];
        let password = match load_password_async(account.id.clone()).await {
            Ok(password) => password,
            Err(error) => {
                failures.push(format!(
                    "CalDAV password missing for {}: {error}",
                    account.username
                ));
                continue;
            }
        };
        let client = match CalDavClient::new(&account.username, &password) {
            Ok(client) => client,
            Err(error) => {
                failures.push(format!("CalDAV {}: {error}", account.username));
                continue;
            }
        };
        let account_id = account.id.clone();
        let calendars: Vec<(String, String)> = account
            .calendars
            .iter()
            .filter(|calendar| calendar.selected)
            .map(|calendar| (calendar.id.clone(), calendar.summary.clone()))
            .collect();
        for (calendar_id, calendar_name) in calendars {
            match client
                .fetch_events(&calendar_id, &account_id, Some(&calendar_name), start, end)
                .await
            {
                Ok(events) => all.extend(events.into_iter().map(|item| item.event)),
                Err(error) => failures.push(format!("CalDAV {calendar_name}: {error}")),
            }
        }
    }
    Ok((all, failures))
}

pub async fn insert_caldav_event(
    accounts: &mut CalendarAccounts,
    account_id: &str,
    calendar_id: &str,
    draft: &CalDavEventDraft,
) -> Result<(), String> {
    let account = accounts
        .caldav_account(account_id)
        .ok_or_else(|| format!("CalDAV account {account_id} missing"))?;
    let password = load_password_async(account_id.to_owned()).await?;
    let client = CalDavClient::new(&account.username, password).map_err(|e| e.to_string())?;
    client
        .put_event(calendar_id, draft, Utc::now())
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub async fn respond_caldav_event(
    accounts: &mut CalendarAccounts,
    account_id: &str,
    href: &str,
    reply: Reply,
) -> Result<bool, String> {
    let account = accounts
        .caldav_account(account_id)
        .ok_or_else(|| format!("CalDAV account {account_id} missing"))?;
    let password = load_password_async(account_id.to_owned()).await?;
    let mailto = account
        .user_addresses
        .iter()
        .find(|addr| addr.to_ascii_lowercase().contains('@'))
        .cloned()
        .or_else(|| {
            account
                .username
                .contains('@')
                .then(|| format!("mailto:{}", account.username))
        })
        .ok_or_else(|| "CalDAV calendar-user-address missing".to_owned())?;
    let notified = account.schedule_outbox.is_some();
    let client = CalDavClient::new(&account.username, password).map_err(|e| e.to_string())?;
    let href = href_from_object_id(href).to_owned();
    client
        .update_partstat(&href, None, &mailto, partstat_for_reply(reply))
        .await
        .map_err(|e| e.to_string())?;
    Ok(notified)
}

pub async fn delete_caldav_event(
    accounts: &mut CalendarAccounts,
    account_id: &str,
    object_id: &str,
    series: bool,
    occurrence_start: DateTime<Utc>,
) -> Result<(), String> {
    let account = accounts
        .caldav_account(account_id)
        .ok_or_else(|| format!("CalDAV account {account_id} missing"))?;
    let password = load_password_async(account_id.to_owned()).await?;
    let client = CalDavClient::new(&account.username, password).map_err(|e| e.to_string())?;
    let href = href_from_object_id(object_id);
    if series {
        client
            .exclude_occurrence(href, occurrence_start, None)
            .await
            .map_err(|e| e.to_string())
    } else {
        client.delete(href, None).await.map_err(|e| e.to_string())
    }
}

pub fn draft_from_meeting(
    subject: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    join_url: Option<&str>,
    attendees: &[String],
    include_attendees: bool,
    repeat: chatsvc::Repeat,
) -> CalDavEventDraft {
    CalDavEventDraft {
        uid: new_event_uid(),
        summary: subject.to_owned(),
        start,
        end,
        all_day: false,
        description: None,
        location: None,
        join_url: join_url.map(str::to_owned),
        attendees: if include_attendees {
            attendees.to_vec()
        } else {
            Vec::new()
        },
        repeat,
        sequence: 0,
    }
}

fn normalize_url(url: &str) -> Result<String, String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("CalDAV URL is empty".into());
    }
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_owned()
    } else {
        format!("https://{trimmed}")
    };
    let parsed = url::Url::parse(&with_scheme).map_err(|e| e.to_string())?;
    if parsed.username() != "" || parsed.password().is_some() {
        return Err("Put the username and app password in the form fields, not in the URL".into());
    }
    Ok(parsed.to_string())
}
