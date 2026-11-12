//! Google Calendar OAuth + REST client for rusty-teams calendar accounts.

mod loopback;
mod merge;
mod oauth;
mod parse;
mod write;

pub use loopback::{allocate_redirect_uri, wait_for_code};
pub use merge::{merge_calendar_events, normalize_join_url};
pub use oauth::{
    AuthPending, GoogleOAuth, TokenBundle, client_id_from_env, exchange_code, refresh_access_token,
};
pub use parse::{parse_calendar_list, parse_google_events};
pub use write::{
    GoogleEventDraft, google_event_body, google_rsvp_query, google_rsvp_status, rrule_for_repeat,
};

use chrono::{DateTime, Utc};
use reqwest::Client;
use serde_json::Value;

use crate::calendar::CalendarEvent;
use crate::error::{Error, Result};

const CALENDAR_API: &str = "https://www.googleapis.com/calendar/v3";

pub struct GoogleCalendar {
    client: Client,
    access_token: String,
}

impl GoogleCalendar {
    pub fn new(access_token: impl Into<String>) -> Self {
        GoogleCalendar {
            client: Client::new(),
            access_token: access_token.into(),
        }
    }

    pub async fn list_calendars(&self) -> Result<Value> {
        self.get(&format!("{CALENDAR_API}/users/me/calendarList"))
            .await
    }

    pub async fn events(
        &self,
        calendar_id: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<CalendarEvent>> {
        let encoded = percent_encoding::utf8_percent_encode(
            calendar_id,
            percent_encoding::NON_ALPHANUMERIC,
        );
        let url = format!(
            "{CALENDAR_API}/calendars/{encoded}/events?singleEvents=true&orderBy=startTime&timeMin={}&timeMax={}",
            start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            end.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        );
        let body = self.get(&url).await?;
        Ok(parse_google_events(&body, None, Some(calendar_id)))
    }

    pub async fn insert_event(&self, calendar_id: &str, draft: &GoogleEventDraft) -> Result<Value> {
        let encoded = percent_encoding::utf8_percent_encode(
            calendar_id,
            percent_encoding::NON_ALPHANUMERIC,
        );
        let url = format!("{CALENDAR_API}/calendars/{encoded}/events?sendUpdates=all");
        self.post(&url, google_event_body(draft)).await
    }

    pub async fn respond(
        &self,
        calendar_id: &str,
        event_id: &str,
        status: &str,
        send_updates: bool,
    ) -> Result<()> {
        let encoded_cal = percent_encoding::utf8_percent_encode(
            calendar_id,
            percent_encoding::NON_ALPHANUMERIC,
        );
        let encoded_event =
            percent_encoding::utf8_percent_encode(event_id, percent_encoding::NON_ALPHANUMERIC);
        let url = format!(
            "{CALENDAR_API}/calendars/{encoded_cal}/events/{encoded_event}?{}",
            google_rsvp_query(send_updates)
        );
        // Patch attendee self via conferenceData-free body: use attendees[].responseStatus
        // by first getting the event is heavy; Google allows PATCH with only attendees when
        // "self" is present. Simpler path: PATCH with a single attendees entry marked self.
        let body = serde_json::json!({
            "attendees": [{ "self": true, "responseStatus": status }]
        });
        self.patch(&url, body).await?;
        Ok(())
    }

    pub async fn delete_event(
        &self,
        calendar_id: &str,
        event_id: &str,
        send_updates: bool,
    ) -> Result<()> {
        let encoded_cal = percent_encoding::utf8_percent_encode(
            calendar_id,
            percent_encoding::NON_ALPHANUMERIC,
        );
        let encoded_event =
            percent_encoding::utf8_percent_encode(event_id, percent_encoding::NON_ALPHANUMERIC);
        let url = format!(
            "{CALENDAR_API}/calendars/{encoded_cal}/events/{encoded_event}?{}",
            google_rsvp_query(send_updates)
        );
        self.delete(&url).await
    }

    async fn get(&self, url: &str) -> Result<Value> {
        let response = self
            .client
            .get(url)
            .bearer_auth(&self.access_token)
            .send()
            .await
            .map_err(|error| Error::UnexpectedAnswer(error.to_string()))?;
        Self::json_result(url, response).await
    }

    async fn post(&self, url: &str, body: Value) -> Result<Value> {
        let response = self
            .client
            .post(url)
            .bearer_auth(&self.access_token)
            .json(&body)
            .send()
            .await
            .map_err(|error| Error::UnexpectedAnswer(error.to_string()))?;
        Self::json_result(url, response).await
    }

    async fn patch(&self, url: &str, body: Value) -> Result<Value> {
        let response = self
            .client
            .patch(url)
            .bearer_auth(&self.access_token)
            .json(&body)
            .send()
            .await
            .map_err(|error| Error::UnexpectedAnswer(error.to_string()))?;
        Self::json_result(url, response).await
    }

    async fn delete(&self, url: &str) -> Result<()> {
        let response = self
            .client
            .delete(url)
            .bearer_auth(&self.access_token)
            .send()
            .await
            .map_err(|error| Error::UnexpectedAnswer(error.to_string()))?;
        let status = response.status().as_u16();
        if (200..300).contains(&status) {
            return Ok(());
        }
        let body = response
            .json::<Value>()
            .await
            .unwrap_or_else(|_| Value::Null);
        Err(Error::Session(session::Error::api(status, url, body)))
    }

    async fn json_result(url: &str, response: reqwest::Response) -> Result<Value> {
        let status = response.status().as_u16();
        let body = response
            .json::<Value>()
            .await
            .unwrap_or_else(|_| Value::Null);
        if !(200..300).contains(&status) {
            return Err(Error::Session(session::Error::api(status, url, body)));
        }
        Ok(body)
    }
}
