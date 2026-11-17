use std::collections::HashSet;
use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::{Client, Method, StatusCode, header};
use uuid::Uuid;

use crate::calendar::{
    AttendeeResponseCounts, CalendarEvent, CalendarSource, EventAttendee, ResponseType, ShowAs,
};
use crate::error::{Error, Result};
use crate::ical::{CalDavEventDraft, exclude_occurrence_from_ics, expand_events, parse_calendar};

use super::discovery::{DiscoverResult, discover, reject_https_downgrade, resolve_url};
use super::xml::{calendar_query_body, parse_calendar_query, parse_multistatus, propfind_body};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone)]
pub struct ListedCalendar {
    pub href: String,
    pub summary: String,
    pub color: Option<String>,
    pub writable: bool,
}

#[derive(Debug, Clone)]
pub struct CalDavEvent {
    pub event: CalendarEvent,
    pub href: String,
    pub etag: Option<String>,
}

pub struct CalDavClient {
    http: Client,
    username: String,
    password: String,
}

impl CalDavClient {
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Result<Self> {
        let http = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                let previous = attempt.previous().last().map(|u| u.as_str()).unwrap_or("");
                let next = attempt.url().as_str();
                if previous.starts_with("https://") && next.starts_with("http://") {
                    attempt.error(std::io::Error::other("https downgrade"))
                } else if attempt.previous().len() > 10 {
                    attempt.stop()
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(map_reqwest)?;
        Ok(CalDavClient {
            http,
            username: username.into(),
            password: password.into(),
        })
    }

    pub async fn discover(&self, url: &str) -> Result<DiscoverResult> {
        discover(self, url).await
    }

    pub async fn list_calendars(&self, home_set: &str) -> Result<Vec<ListedCalendar>> {
        let body = propfind_body(&[
            "<d:displayname/>",
            "<d:resourcetype/>",
            "<d:current-user-privilege-set/>",
            "<c:supported-calendar-component-set/>",
            "<x:calendar-color/>",
        ]);
        let xml = self.propfind(home_set, "1", &body).await?;
        let mut calendars = Vec::new();
        for response in parse_multistatus(&xml) {
            if !response.resource_type_calendar {
                continue;
            }
            // Prefer VEVENT; if component set absent, still include calendar collections.
            if !response.supports_vevent
                && xml.contains("supported-calendar-component-set")
                && response.href.contains("todo")
            {
                continue;
            }
            let href = resolve_url(home_set, &response.href)?;
            calendars.push(ListedCalendar {
                href,
                summary: response
                    .displayname
                    .unwrap_or_else(|| "Calendar".to_owned()),
                color: response.calendar_color,
                writable: response.writable,
            });
        }
        Ok(calendars)
    }

    pub async fn fetch_events(
        &self,
        calendar_href: &str,
        account_id: &str,
        calendar_name: Option<&str>,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<CalDavEvent>> {
        let start = from.format("%Y%m%dT%H%M%SZ").to_string();
        let end = to.format("%Y%m%dT%H%M%SZ").to_string();
        let mut resources = self.calendar_query(calendar_href, &start, &end, true).await;
        if resources.is_err() {
            resources = self
                .calendar_query(calendar_href, &start, &end, false)
                .await;
        }
        let resources = resources?;
        let mut out = Vec::new();
        for (href, etag, data) in resources {
            let absolute = resolve_url(calendar_href, &href).unwrap_or(href);
            let parsed = parse_calendar(&data);
            let series_uids: HashSet<String> = parsed
                .iter()
                .filter(|event| {
                    event.rrule.is_some()
                        || !event.rdates.is_empty()
                        || event.recurrence_id.is_some()
                })
                .map(|event| event.uid.clone())
                .collect();
            let expanded = expand_events(parsed, from, to);
            for item in expanded {
                let mut event = to_calendar_event(&item, account_id, &absolute, calendar_name);
                let uid = event.ical_uid.clone().unwrap_or_default();
                let series = series_uids.contains(&uid) || item.recurrence_id.is_some();
                event.series = series;
                event.object_id = object_id_for(&absolute, &uid, series, item.start);
                out.push(CalDavEvent {
                    event,
                    href: absolute.clone(),
                    etag: etag.clone(),
                });
            }
        }
        Ok(out)
    }

    pub async fn put_event(
        &self,
        calendar_href: &str,
        draft: &CalDavEventDraft,
        stamp: DateTime<Utc>,
    ) -> Result<String> {
        let href = format!("{}{}.ics", ensure_trailing_slash(calendar_href), draft.uid);
        let body = crate::ical::build_vevent(draft, stamp);
        let response = self
            .http
            .put(&href)
            .basic_auth(&self.username, Some(&self.password))
            .header(header::CONTENT_TYPE, "text/calendar; charset=utf-8")
            .header(header::IF_NONE_MATCH, "*")
            .body(body)
            .send()
            .await
            .map_err(map_reqwest)?;
        ensure_auth(response.status())?;
        if !response.status().is_success() {
            return Err(Error::UnexpectedAnswer(format!(
                "CalDAV PUT failed: {}",
                response.status()
            )));
        }
        Ok(href)
    }

    pub async fn delete(&self, href: &str, etag: Option<&str>) -> Result<()> {
        let mut request = self
            .http
            .delete(href)
            .basic_auth(&self.username, Some(&self.password));
        if let Some(etag) = etag {
            request = request.header(header::IF_MATCH, format!("\"{etag}\""));
        }
        let response = request.send().await.map_err(map_reqwest)?;
        ensure_auth(response.status())?;
        if response.status() == StatusCode::NOT_FOUND || response.status().is_success() {
            return Ok(());
        }
        Err(Error::UnexpectedAnswer(format!(
            "CalDAV DELETE failed: {}",
            response.status()
        )))
    }

    /// Exclude one occurrence by EXDATE on the master; does not DELETE the series href.
    pub async fn exclude_occurrence(
        &self,
        href: &str,
        occurrence: DateTime<Utc>,
        etag: Option<&str>,
    ) -> Result<()> {
        let response = self
            .http
            .get(href)
            .basic_auth(&self.username, Some(&self.password))
            .send()
            .await
            .map_err(map_reqwest)?;
        ensure_auth(response.status())?;
        let fresh_etag = response
            .headers()
            .get(header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.trim_matches('"').to_owned());
        let body = response.text().await.map_err(map_reqwest)?;
        let updated =
            exclude_occurrence_from_ics(&body, occurrence).map_err(Error::UnexpectedAnswer)?;
        let match_etag = etag.or(fresh_etag.as_deref());
        let mut request = self
            .http
            .put(href)
            .basic_auth(&self.username, Some(&self.password))
            .header(header::CONTENT_TYPE, "text/calendar; charset=utf-8")
            .body(updated);
        if let Some(etag) = match_etag {
            request = request.header(header::IF_MATCH, format!("\"{etag}\""));
        }
        let response = request.send().await.map_err(map_reqwest)?;
        if response.status() == StatusCode::PRECONDITION_FAILED {
            return Err(Error::UnexpectedAnswer(
                "CalDAV event changed on the server; reload and try again".into(),
            ));
        }
        ensure_auth(response.status())?;
        if !response.status().is_success() {
            return Err(Error::UnexpectedAnswer(format!(
                "CalDAV EXDATE failed: {}",
                response.status()
            )));
        }
        Ok(())
    }

    pub async fn update_partstat(
        &self,
        href: &str,
        etag: Option<&str>,
        mailto: &str,
        partstat: &str,
    ) -> Result<()> {
        let response = self
            .http
            .get(href)
            .basic_auth(&self.username, Some(&self.password))
            .send()
            .await
            .map_err(map_reqwest)?;
        ensure_auth(response.status())?;
        let fresh_etag = response
            .headers()
            .get(header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.trim_matches('"').to_owned());
        let body = response.text().await.map_err(map_reqwest)?;
        let updated = patch_partstat(&body, mailto, partstat);
        let match_etag = etag.or(fresh_etag.as_deref());
        let mut request = self
            .http
            .put(href)
            .basic_auth(&self.username, Some(&self.password))
            .header(header::CONTENT_TYPE, "text/calendar; charset=utf-8")
            .body(updated);
        if let Some(etag) = match_etag {
            request = request.header(header::IF_MATCH, format!("\"{etag}\""));
        }
        let response = request.send().await.map_err(map_reqwest)?;
        if response.status() == StatusCode::PRECONDITION_FAILED {
            return Err(Error::UnexpectedAnswer(
                "CalDAV event changed on the server; reload and try again".into(),
            ));
        }
        ensure_auth(response.status())?;
        if !response.status().is_success() {
            return Err(Error::UnexpectedAnswer(format!(
                "CalDAV RSVP failed: {}",
                response.status()
            )));
        }
        Ok(())
    }

    pub async fn propfind(&self, url: &str, depth: &str, body: &str) -> Result<String> {
        let method = Method::from_bytes(b"PROPFIND").unwrap();
        let response = self
            .http
            .request(method, url)
            .basic_auth(&self.username, Some(&self.password))
            .header("Depth", depth)
            .header(header::CONTENT_TYPE, "application/xml; charset=utf-8")
            .body(body.to_owned())
            .send()
            .await
            .map_err(map_reqwest)?;
        reject_https_downgrade(url, response.url().as_str())?;
        if response.status() == StatusCode::UNAUTHORIZED {
            let www = response
                .headers()
                .get(header::WWW_AUTHENTICATE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            if www.to_ascii_lowercase().contains("digest") {
                return Err(Error::UnexpectedAnswer(
                    "CalDAV Digest authentication is not supported; enable Basic auth or use an app password with Basic".into(),
                ));
            }
            return Err(Error::UnexpectedAnswer("CalDAV unauthorized".into()));
        }
        if !response.status().is_success() && response.status().as_u16() != 207 {
            return Err(Error::UnexpectedAnswer(format!(
                "CalDAV PROPFIND failed: {}",
                response.status()
            )));
        }
        response.text().await.map_err(map_reqwest)
    }

    async fn calendar_query(
        &self,
        url: &str,
        start: &str,
        end: &str,
        expand: bool,
    ) -> Result<Vec<(String, Option<String>, String)>> {
        let method = Method::from_bytes(b"REPORT").unwrap();
        let body = calendar_query_body(start, end, expand);
        let response = self
            .http
            .request(method, url)
            .basic_auth(&self.username, Some(&self.password))
            .header("Depth", "1")
            .header(header::CONTENT_TYPE, "application/xml; charset=utf-8")
            .body(body)
            .send()
            .await
            .map_err(map_reqwest)?;
        ensure_auth(response.status())?;
        if !response.status().is_success() && response.status().as_u16() != 207 {
            return Err(Error::UnexpectedAnswer(format!(
                "CalDAV REPORT failed: {}",
                response.status()
            )));
        }
        let text = response.text().await.map_err(map_reqwest)?;
        Ok(parse_calendar_query(&text))
    }
}

fn to_calendar_event(
    parsed: &crate::ical::ParsedEvent,
    account_id: &str,
    calendar_id: &str,
    calendar_name: Option<&str>,
) -> CalendarEvent {
    let response = own_response(parsed);
    CalendarEvent {
        object_id: parsed.uid.clone(),
        subject: parsed.summary.clone(),
        start: parsed.start,
        end: parsed.end,
        all_day: parsed.all_day,
        cancelled: parsed.cancelled,
        online: parsed.join_url.is_some(),
        join_url: parsed.join_url.clone(),
        thread_id: None,
        organizer_name: parsed.organizer.as_ref().map(|(n, _)| n.clone()),
        organizer_address: parsed.organizer.as_ref().and_then(|(_, a)| a.clone()),
        ical_uid: Some(parsed.uid.clone()),
        response,
        show_as: if parsed.transparent {
            ShowAs::Free
        } else {
            ShowAs::Busy
        },
        dial_in: None,
        source: CalendarSource::CalDav,
        account_id: Some(account_id.to_owned()),
        calendar_id: Some(calendar_id.to_owned()),
        sources: vec![CalendarSource::CalDav],
        location: parsed.location.clone(),
        body_preview: parsed.description.clone().map(|d| {
            if d.chars().count() > 280 {
                d.chars().take(280).collect()
            } else {
                d
            }
        }),
        body_html: parsed.description.clone(),
        // Overridden by fetch_events after expansion (RRULE is cleared on occurrences).
        series: parsed.rrule.is_some()
            || !parsed.rdates.is_empty()
            || parsed.recurrence_id.is_some(),
        series_master_id: None,
        response_counts: None::<AttendeeResponseCounts>,
        attendees: parsed
            .attendees
            .iter()
            .map(|a| EventAttendee {
                name: a.cn.clone().unwrap_or_else(|| a.mailto.clone()),
                address: Some(a.mailto.clone()),
                response: partstat_to_response(a.partstat.as_deref().unwrap_or("NEEDS-ACTION")),
                required: a
                    .role
                    .as_deref()
                    .map(|r| !r.eq_ignore_ascii_case("OPT-PARTICIPANT"))
                    .unwrap_or(true),
            })
            .collect(),
        reminders: parsed.reminders_minutes.clone(),
        source_link: None,
        web_link: None,
        calendar_name: calendar_name.map(str::to_owned),
    }
}

fn own_response(parsed: &crate::ical::ParsedEvent) -> ResponseType {
    parsed
        .attendees
        .iter()
        .find_map(|a| a.partstat.as_deref().map(partstat_to_response))
        .unwrap_or(ResponseType::NotResponded)
}

fn partstat_to_response(partstat: &str) -> ResponseType {
    match partstat.to_ascii_uppercase().as_str() {
        "ACCEPTED" => ResponseType::Accepted,
        "TENTATIVE" => ResponseType::Tentative,
        "DECLINED" => ResponseType::Declined,
        "DELEGATED" => ResponseType::NotResponded,
        _ => ResponseType::NotResponded,
    }
}

pub fn partstat_for_reply(reply: crate::calendar::Reply) -> &'static str {
    match reply {
        crate::calendar::Reply::Accept => "ACCEPTED",
        crate::calendar::Reply::Tentative => "TENTATIVE",
        crate::calendar::Reply::Decline => "DECLINED",
    }
}

fn patch_partstat(ics: &str, mailto: &str, partstat: &str) -> String {
    let target = mailto.trim_start_matches("mailto:").to_ascii_lowercase();
    let mut out = String::with_capacity(ics.len() + 32);
    for line in ics.lines() {
        if line.to_ascii_uppercase().starts_with("ATTENDEE")
            && line.to_ascii_lowercase().contains(&target)
        {
            let mut rewritten = line.to_owned();
            if let Some(start) = rewritten.to_ascii_uppercase().find("PARTSTAT=") {
                let rest = &rewritten[start + 9..];
                let end = rest.find([';', ':']).unwrap_or(rest.len());
                rewritten.replace_range(start + 9..start + 9 + end, partstat);
            } else if let Some(idx) = rewritten.find(':') {
                rewritten.insert_str(idx, &format!(";PARTSTAT={partstat}"));
            }
            out.push_str(&rewritten);
            out.push_str("\r\n");
        } else {
            out.push_str(line);
            out.push_str("\r\n");
        }
    }
    out
}

fn ensure_trailing_slash(url: &str) -> String {
    if url.ends_with('/') {
        url.to_owned()
    } else {
        format!("{url}/")
    }
}

fn ensure_auth(status: StatusCode) -> Result<()> {
    if status == StatusCode::UNAUTHORIZED {
        return Err(Error::UnexpectedAnswer("CalDAV unauthorized".into()));
    }
    Ok(())
}

fn map_reqwest(error: reqwest::Error) -> Error {
    Error::UnexpectedAnswer(error.to_string())
}

pub fn new_event_uid() -> String {
    format!("{}@rusty-teams", Uuid::new_v4())
}

pub fn object_id_for(href: &str, uid: &str, series: bool, start: DateTime<Utc>) -> String {
    if series {
        format!("{href}#{uid}#{}", start.format("%Y%m%dT%H%M%SZ"))
    } else {
        format!("{href}#{uid}")
    }
}

pub fn href_from_object_id(object_id: &str) -> &str {
    object_id.split('#').next().unwrap_or(object_id)
}

#[cfg(test)]
mod object_id_tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn series_occurrences_get_distinct_ids() {
        let start_a = Utc.with_ymd_and_hms(2026, 1, 5, 9, 0, 0).unwrap();
        let start_b = Utc.with_ymd_and_hms(2026, 1, 12, 9, 0, 0).unwrap();
        let a = object_id_for("https://dav.example/cal/evt.ics", "series", true, start_a);
        let b = object_id_for("https://dav.example/cal/evt.ics", "series", true, start_b);
        assert_ne!(a, b);
        assert!(a.ends_with("#20260105T090000Z"));
        assert_eq!(href_from_object_id(&a), "https://dav.example/cal/evt.ics");
    }
}
