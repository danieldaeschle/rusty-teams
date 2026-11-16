//! Read-only ICS subscriptions (holidays, Proton share links).

use chrono::{DateTime, Utc};
use store::{CalendarAccounts, IcsSubscription};
use uuid::Uuid;

use chatsvc::{CalendarEvent, CalendarSource, expand_events, parse_calendar};

pub async fn add_ics_subscription(
    accounts: &mut CalendarAccounts,
    url: &str,
    summary: &str,
) -> Result<String, String> {
    let url = url.trim().to_owned();
    if url.is_empty() {
        return Err("ICS URL is empty".into());
    }
    // Smoke-fetch once so a bad URL fails at connect time.
    let _ = fetch_one(&url, "tmp", summary, Utc::now(), Utc::now() + chrono::Duration::days(1))
        .await?;
    let id = Uuid::new_v4().to_string();
    accounts.ics_subscriptions.push(IcsSubscription {
        id: id.clone(),
        url,
        summary: if summary.trim().is_empty() {
            "ICS".into()
        } else {
            summary.to_owned()
        },
        selected: true,
    });
    Ok(id)
}

pub async fn fetch_ics_events(
    accounts: &CalendarAccounts,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<CalendarEvent>, String> {
    let mut all = Vec::new();
    for subscription in accounts
        .ics_subscriptions
        .iter()
        .filter(|subscription| subscription.selected)
    {
        match fetch_one(
            &subscription.url,
            &subscription.id,
            &subscription.summary,
            start,
            end,
        )
        .await
        {
            Ok(events) => all.extend(events),
            Err(error) => return Err(format!("ICS {}: {error}", subscription.summary)),
        }
    }
    Ok(all)
}

async fn fetch_one(
    url: &str,
    account_id: &str,
    calendar_name: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<CalendarEvent>, String> {
    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    let body = response.text().await.map_err(|e| e.to_string())?;
    let parsed = parse_calendar(&body);
    let expanded = expand_events(parsed, start, end);
    Ok(expanded
        .into_iter()
        .map(|item| CalendarEvent {
            object_id: format!("ics:{}:{}", account_id, item.uid),
            subject: item.summary,
            start: item.start,
            end: item.end,
            all_day: item.all_day,
            cancelled: item.cancelled,
            online: item.join_url.is_some(),
            join_url: item.join_url,
            ical_uid: Some(item.uid),
            source: CalendarSource::Ics,
            account_id: Some(account_id.to_owned()),
            calendar_id: Some(url.to_owned()),
            sources: vec![CalendarSource::Ics],
            location: item.location,
            body_preview: item.description.clone(),
            body_html: item.description,
            reminders: item.reminders_minutes,
            calendar_name: Some(calendar_name.to_owned()),
            ..Default::default()
        })
        .collect())
}
