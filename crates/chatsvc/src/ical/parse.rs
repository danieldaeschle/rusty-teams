use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, TimeZone, Utc};

use super::join::teams_join_url;
use super::timezone::resolve_tzid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedEvent {
    pub uid: String,
    pub href_hint: Option<String>,
    pub summary: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub all_day: bool,
    pub cancelled: bool,
    pub transparent: bool,
    pub description: Option<String>,
    pub location: Option<String>,
    pub join_url: Option<String>,
    pub organizer: Option<(String, Option<String>)>,
    pub attendees: Vec<ParsedAttendee>,
    pub reminders_minutes: Vec<u32>,
    pub rrule: Option<String>,
    pub rdates: Vec<DateTime<Utc>>,
    pub exdates: Vec<DateTime<Utc>>,
    pub recurrence_id: Option<DateTime<Utc>>,
    pub sequence: u32,
    pub status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedAttendee {
    pub cn: Option<String>,
    pub mailto: String,
    pub partstat: Option<String>,
    pub role: Option<String>,
}

/// Parse one or more VEVENT components from an iCalendar document.
pub fn parse_calendar(ics: &str) -> Vec<ParsedEvent> {
    let unfolded = unfold(ics);
    let mut events = Vec::new();
    let mut lines = unfolded.lines().peekable();
    while let Some(line) = lines.next() {
        if line.eq_ignore_ascii_case("BEGIN:VEVENT") {
            let mut body = Vec::new();
            for next in lines.by_ref() {
                if next.eq_ignore_ascii_case("END:VEVENT") {
                    break;
                }
                body.push(next.to_owned());
            }
            if let Some(event) = parse_vevent(&body) {
                events.push(event);
            }
        }
    }
    events
}

fn unfold(ics: &str) -> String {
    let mut out = String::with_capacity(ics.len());
    let mut chars = ics.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            continue;
        }
        if ch == '\n' {
            if matches!(chars.peek(), Some(' ') | Some('\t')) {
                chars.next();
                continue;
            }
            out.push('\n');
            continue;
        }
        out.push(ch);
    }
    out
}

fn parse_vevent(lines: &[String]) -> Option<ParsedEvent> {
    let mut uid = String::new();
    let mut summary = String::new();
    let mut description = None;
    let mut location = None;
    let mut url = None;
    let mut skype = None;
    let mut start = None;
    let mut end = None;
    let mut duration = None;
    let mut all_day = false;
    let mut cancelled = false;
    let mut transparent = false;
    let mut organizer = None;
    let mut attendees = Vec::new();
    let mut reminders = Vec::new();
    let mut rrule = None;
    let mut rdates = Vec::new();
    let mut exdates = Vec::new();
    let mut recurrence_id = None;
    let mut sequence = 0u32;
    let mut status = None;
    let mut in_alarm = false;
    let mut alarm_trigger = None;

    for line in lines {
        let (name, params, value) = split_property(line);
        let name_upper = name.to_ascii_uppercase();
        if in_alarm {
            if name_upper == "END" && value.eq_ignore_ascii_case("VALARM") {
                if let Some(minutes) = alarm_trigger {
                    reminders.push(minutes);
                }
                alarm_trigger = None;
                in_alarm = false;
            } else if name_upper == "TRIGGER" {
                alarm_trigger = parse_trigger_minutes(&value);
            }
            continue;
        }
        match name_upper.as_str() {
            "BEGIN" if value.eq_ignore_ascii_case("VALARM") => in_alarm = true,
            "UID" => uid = unescape(&value),
            "SUMMARY" => summary = unescape(&value),
            "DESCRIPTION" => description = Some(unescape(&value)),
            "LOCATION" => location = Some(unescape(&value)),
            "URL" => url = Some(unescape(&value)),
            "X-MICROSOFT-SKYPETEAMSMEETINGURL" => skype = Some(unescape(&value)),
            "DTSTART" => {
                let (dt, day) = parse_date_value(&params, &value)?;
                start = Some(dt);
                all_day = day;
            }
            "DTEND" => {
                let (dt, day) = parse_date_value(&params, &value)?;
                // All-day DTEND is exclusive.
                end = Some(if day { dt } else { dt });
                if day {
                    all_day = true;
                }
            }
            "DURATION" => duration = parse_duration(&value),
            "STATUS" => {
                status = Some(value.clone());
                if value.eq_ignore_ascii_case("CANCELLED") {
                    cancelled = true;
                }
            }
            "TRANSP" => transparent = value.eq_ignore_ascii_case("TRANSPARENT"),
            "RRULE" => rrule = Some(value),
            "RDATE" => rdates.extend(parse_multi_dates(&params, &value)),
            "EXDATE" => exdates.extend(parse_multi_dates(&params, &value)),
            "RECURRENCE-ID" => {
                if let Some((dt, _)) = parse_date_value(&params, &value) {
                    recurrence_id = Some(dt);
                }
            }
            "SEQUENCE" => sequence = value.parse().unwrap_or(0),
            "ORGANIZER" => organizer = Some(parse_cal_address(&params, &value)),
            "ATTENDEE" => attendees.push(parse_attendee(&params, &value)),
            _ => {}
        }
    }

    let start = start?;
    let end = end
        .or_else(|| duration.and_then(|d| start.checked_add_signed(d)))
        .unwrap_or_else(|| {
            if all_day {
                start + Duration::days(1)
            } else {
                start + Duration::hours(1)
            }
        });
    let join = teams_join_url(
        skype.as_deref(),
        url.as_deref(),
        location.as_deref(),
        description.as_deref(),
    );
    Some(ParsedEvent {
        uid,
        href_hint: None,
        summary,
        start,
        end,
        all_day,
        cancelled,
        transparent,
        description,
        location: location.filter(|value| {
            !value.contains("teams.microsoft.com") && !value.contains("teams.live.com")
        }),
        join_url: join,
        organizer,
        attendees,
        reminders_minutes: reminders,
        rrule,
        rdates,
        exdates,
        recurrence_id,
        sequence,
        status,
    })
}

fn split_property(line: &str) -> (String, Vec<(String, String)>, String) {
    let (left, value) = match line.split_once(':') {
        Some((l, v)) => (l, v.to_owned()),
        None => (line, String::new()),
    };
    let mut parts = left.split(';');
    let name = parts.next().unwrap_or("").to_owned();
    let mut params = Vec::new();
    for part in parts {
        if let Some((k, v)) = part.split_once('=') {
            params.push((k.to_ascii_uppercase(), v.trim_matches('"').to_owned()));
        }
    }
    (name, params, value)
}

fn param<'a>(params: &'a [(String, String)], key: &str) -> Option<&'a str> {
    params
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.as_str())
}

fn parse_date_value(params: &[(String, String)], value: &str) -> Option<(DateTime<Utc>, bool)> {
    let value_param = param(params, "VALUE").unwrap_or("");
    let tzid = param(params, "TZID");
    if value_param.eq_ignore_ascii_case("DATE") || (value.len() == 8 && !value.contains('T')) {
        let date = NaiveDate::parse_from_str(value, "%Y%m%d").ok()?;
        let start = date.and_hms_opt(0, 0, 0)?;
        return Some((Utc.from_utc_datetime(&start), true));
    }
    let floating = !value.ends_with('Z') && tzid.is_none();
    let naive = if value.ends_with('Z') {
        NaiveDateTime::parse_from_str(value.trim_end_matches('Z'), "%Y%m%dT%H%M%S").ok()?
    } else {
        NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S")
            .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S%.f"))
            .ok()?
    };
    if value.ends_with('Z') {
        return Some((Utc.from_utc_datetime(&naive), false));
    }
    if let Some(tzid) = tzid
        && let Some(tz) = resolve_tzid(tzid)
    {
        return Some((
            tz.from_local_datetime(&naive)
                .single()
                .or_else(|| tz.from_local_datetime(&naive).earliest())?
                .with_timezone(&Utc),
            false,
        ));
    }
    if floating {
        // Treat floating as local wall time interpreted via host offset later; store as UTC naive.
        return Some((Utc.from_utc_datetime(&naive), false));
    }
    Some((Utc.from_utc_datetime(&naive), false))
}

fn parse_multi_dates(params: &[(String, String)], value: &str) -> Vec<DateTime<Utc>> {
    value
        .split(',')
        .filter_map(|part| parse_date_value(params, part.trim()).map(|(dt, _)| dt))
        .collect()
}

fn parse_duration(value: &str) -> Option<Duration> {
    // PTnHnMnS / PnD subset
    let mut rest = value.trim().trim_start_matches('+');
    let negative = rest.starts_with('-');
    if negative {
        rest = &rest[1..];
    }
    if !rest.starts_with('P') {
        return None;
    }
    rest = &rest[1..];
    let total = if let Some((date, time)) = rest.split_once('T') {
        parse_duration_date(date)?.checked_add(&parse_duration_time(time)?)?
    } else if rest.contains('T') {
        parse_duration_time(rest.trim_start_matches('T'))?
    } else {
        parse_duration_date(rest)?
    };
    Some(if negative { -total } else { total })
}

fn parse_duration_date(value: &str) -> Option<Duration> {
    if value.is_empty() {
        return Some(Duration::zero());
    }
    let mut num = String::new();
    let mut total = Duration::zero();
    for ch in value.chars() {
        if ch.is_ascii_digit() {
            num.push(ch);
            continue;
        }
        let n: i64 = num.parse().ok()?;
        num.clear();
        // A feed value too large for a duration drops the field instead of panicking.
        let part = match ch {
            'D' => Duration::try_days(n)?,
            'W' => Duration::try_weeks(n)?,
            _ => return None,
        };
        total = total.checked_add(&part)?;
    }
    Some(total)
}

fn parse_duration_time(value: &str) -> Option<Duration> {
    let mut num = String::new();
    let mut total = Duration::zero();
    for ch in value.chars() {
        if ch.is_ascii_digit() {
            num.push(ch);
            continue;
        }
        let n: i64 = num.parse().ok()?;
        num.clear();
        let part = match ch {
            'H' => Duration::try_hours(n)?,
            'M' => Duration::try_minutes(n)?,
            'S' => Duration::try_seconds(n)?,
            _ => return None,
        };
        total = total.checked_add(&part)?;
    }
    Some(total)
}

fn parse_trigger_minutes(value: &str) -> Option<u32> {
    let duration = parse_duration(value)?;
    let minutes = duration.num_minutes().unsigned_abs() as u32;
    Some(minutes)
}

fn parse_cal_address(params: &[(String, String)], value: &str) -> (String, Option<String>) {
    let mailto = value
        .trim()
        .trim_start_matches("mailto:")
        .trim_start_matches("MAILTO:")
        .to_owned();
    let cn = param(params, "CN").map(str::to_owned);
    (cn.unwrap_or_else(|| mailto.clone()), Some(mailto))
}

fn parse_attendee(params: &[(String, String)], value: &str) -> ParsedAttendee {
    let mailto = value
        .trim()
        .trim_start_matches("mailto:")
        .trim_start_matches("MAILTO:")
        .to_owned();
    ParsedAttendee {
        cn: param(params, "CN").map(str::to_owned),
        mailto,
        partstat: param(params, "PARTSTAT").map(str::to_owned),
        role: param(params, "ROLE").map(str::to_owned),
    }
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(',') => out.push(','),
                Some(';') => out.push(';'),
                Some('\\') => out.push('\\'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_too_large_for_a_date_are_dropped_instead_of_panicking() {
        assert_eq!(parse_duration("PT99999999999999999S"), None);
        assert_eq!(parse_duration("P9999999999999D"), None);
        assert_eq!(parse_duration("-PT15M"), Some(-Duration::minutes(15)));
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:1\r\nSUMMARY:Long\r\nDTSTART:20260101T100000Z\r\nDURATION:P99999999D\r\nBEGIN:VALARM\r\nTRIGGER:-PT99999999999999999M\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let events = parse_calendar(ics);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].end - events[0].start, Duration::hours(1));
    }

    #[test]
    fn unfolds_and_parses_all_day_exclusive_end() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:1\r\nSUMMARY:Holiday\r\nDTSTART;VALUE=DATE:20260101\r\nDTEND;VALUE=DATE:20260102\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let events = parse_calendar(ics);
        assert_eq!(events.len(), 1);
        assert!(events[0].all_day);
        assert_eq!(
            events[0].start,
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap()
        );
        assert_eq!(
            events[0].end,
            Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap()
        );
    }

    #[test]
    fn folds_long_summary() {
        let ics = "BEGIN:VEVENT\nUID:1\nSUMMARY:Hello\n  world\nDTSTART:20260101T100000Z\nDTEND:20260101T110000Z\nEND:VEVENT\n";
        let events = parse_calendar(ics);
        assert_eq!(events[0].summary, "Hello world");
    }

    #[test]
    fn groups_override_separately() {
        let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:series\nSUMMARY:Master\nDTSTART:20260105T100000Z\nDTEND:20260105T110000Z\nRRULE:FREQ=WEEKLY\nEND:VEVENT\nBEGIN:VEVENT\nUID:series\nSUMMARY:Changed\nDTSTART:20260112T100000Z\nDTEND:20260112T110000Z\nRECURRENCE-ID:20260112T100000Z\nEND:VEVENT\nEND:VCALENDAR\n";
        let events = parse_calendar(ics);
        assert_eq!(events.len(), 2);
        assert!(events[0].rrule.is_some());
        assert!(events[1].recurrence_id.is_some());
    }
}
