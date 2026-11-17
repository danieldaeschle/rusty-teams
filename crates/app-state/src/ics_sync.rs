//! Read-only ICS subscriptions (holidays, Proton share links).

use std::collections::HashSet;
use std::time::Duration;

use chrono::{DateTime, Utc};
use store::{CalendarAccounts, IcsSubscription};
use uuid::Uuid;

use chatsvc::{CalendarEvent, CalendarSource, expand_events, object_id_for, parse_calendar};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

pub async fn add_ics_subscription(
    accounts: &mut CalendarAccounts,
    url: &str,
    summary: &str,
) -> Result<String, String> {
    let url = normalize_ics_url(url)?;
    // Smoke-fetch once so a bad URL fails at connect time.
    let _ = fetch_one(
        &url,
        "tmp",
        summary,
        Utc::now(),
        Utc::now() + chrono::Duration::days(1),
    )
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

/// Events plus per-subscription failure messages.
pub async fn fetch_ics_events(
    accounts: &CalendarAccounts,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<(Vec<CalendarEvent>, Vec<String>), String> {
    let mut all = Vec::new();
    let mut failures = Vec::new();
    for subscription in accounts
        .ics_subscriptions
        .iter()
        .filter(|subscription| subscription.selected)
    {
        let url = match normalize_ics_url(&subscription.url) {
            Ok(url) => url,
            Err(error) => {
                failures.push(format!("ICS {}: {error}", subscription.summary));
                continue;
            }
        };
        match fetch_one(&url, &subscription.id, &subscription.summary, start, end).await {
            Ok(events) => all.extend(events),
            Err(error) => failures.push(format!("ICS {}: {error}", subscription.summary)),
        }
    }
    Ok((all, failures))
}

pub fn normalize_ics_url(url: &str) -> Result<String, String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("ICS URL is empty".into());
    }
    let lowered = trimmed.to_ascii_lowercase();
    let with_scheme = if let Some(rest) = lowered.strip_prefix("webcals://") {
        format!("https://{}", &trimmed[trimmed.len() - rest.len()..])
    } else if let Some(rest) = lowered.strip_prefix("webcal://") {
        format!("https://{}", &trimmed[trimmed.len() - rest.len()..])
    } else if trimmed.contains("://") {
        trimmed.to_owned()
    } else {
        format!("https://{trimmed}")
    };
    let parsed = url::Url::parse(&with_scheme).map_err(|e| e.to_string())?;
    if parsed.scheme() != "https" && parsed.scheme() != "http" {
        return Err(format!("Unsupported ICS URL scheme: {}", parsed.scheme()));
    }
    Ok(parsed.to_string())
}

async fn fetch_one(
    url: &str,
    account_id: &str,
    calendar_name: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<CalendarEvent>, String> {
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let response = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    let body = response.text().await.map_err(|e| e.to_string())?;
    let parsed = parse_calendar(&body);
    let series_uids: HashSet<String> = parsed
        .iter()
        .filter(|event| {
            event.rrule.is_some() || !event.rdates.is_empty() || event.recurrence_id.is_some()
        })
        .map(|event| event.uid.clone())
        .collect();
    let expanded = expand_events(parsed, start, end);
    Ok(expanded
        .into_iter()
        .map(|item| {
            let series = series_uids.contains(&item.uid) || item.recurrence_id.is_some();
            let object_id =
                object_id_for(&format!("ics:{account_id}"), &item.uid, series, item.start);
            CalendarEvent {
                object_id,
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
                series,
                ..Default::default()
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webcal_becomes_https() {
        assert_eq!(
            normalize_ics_url("webcal://calendar.example/feed.ics").unwrap(),
            "https://calendar.example/feed.ics"
        );
        assert_eq!(
            normalize_ics_url("webcals://calendar.example/feed.ics").unwrap(),
            "https://calendar.example/feed.ics"
        );
    }
}
