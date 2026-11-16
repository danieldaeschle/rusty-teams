use quick_xml::Reader;
use quick_xml::events::Event;

#[derive(Debug, Clone, Default)]
pub struct PropfindResponse {
    pub href: String,
    pub displayname: Option<String>,
    pub calendar_color: Option<String>,
    pub resource_type_calendar: bool,
    pub supports_vevent: bool,
    pub writable: bool,
    pub current_user_principal: Option<String>,
    pub calendar_home_set: Option<String>,
    pub calendar_user_addresses: Vec<String>,
    pub schedule_outbox: Option<String>,
    pub getctag: Option<String>,
}

pub fn parse_multistatus(xml: &str) -> Vec<PropfindResponse> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut responses = Vec::new();
    let mut current = PropfindResponse::default();
    let mut in_response = false;
    let mut buf = Vec::new();
    let mut path: Vec<String> = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let local = local_name(&e.name().as_ref());
                path.push(local.clone());
                if local == "response" {
                    in_response = true;
                    current = PropfindResponse::default();
                }
                if local == "calendar" && path.iter().any(|p| p == "resourcetype") {
                    current.resource_type_calendar = true;
                }
                if local == "comp" {
                    for attr in e.attributes().flatten() {
                        if local_name(attr.key.as_ref()) == "name"
                            && attr.unescape_value().map(|v| v.eq_ignore_ascii_case("VEVENT")).unwrap_or(false)
                        {
                            current.supports_vevent = true;
                        }
                    }
                }
            }
            Ok(Event::Empty(e)) => {
                let local = local_name(&e.name().as_ref());
                if local == "calendar" && path.iter().any(|p| p == "resourcetype") {
                    current.resource_type_calendar = true;
                }
                if local == "comp" {
                    for attr in e.attributes().flatten() {
                        if local_name(attr.key.as_ref()) == "name"
                            && attr
                                .unescape_value()
                                .map(|v| v.eq_ignore_ascii_case("VEVENT"))
                                .unwrap_or(false)
                        {
                            current.supports_vevent = true;
                        }
                    }
                }
                if local == "write" || local == "write-content" || local == "all" {
                    if path.iter().any(|p| p == "current-user-privilege-set") {
                        current.writable = true;
                    }
                }
            }
            Ok(Event::Text(t)) => {
                let text = t.unescape().unwrap_or_default().to_string();
                let leaf = path.last().cloned().unwrap_or_default();
                if !in_response {
                    continue;
                }
                match leaf.as_str() {
                    "href" => {
                        if path.iter().any(|p| p == "current-user-principal") {
                            current.current_user_principal = Some(text);
                        } else if path.iter().any(|p| p == "calendar-home-set") {
                            current.calendar_home_set = Some(text);
                        } else if path.iter().any(|p| p == "calendar-user-address-set") {
                            current.calendar_user_addresses.push(text);
                        } else if path.iter().any(|p| p == "schedule-outbox-URL") {
                            current.schedule_outbox = Some(text);
                        } else if current.href.is_empty() {
                            current.href = text;
                        }
                    }
                    "displayname" => current.displayname = Some(text),
                    "calendar-color" | "color" => current.calendar_color = Some(text),
                    "getctag" | "getetag" => current.getctag = Some(text.trim_matches('"').to_owned()),
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let local = local_name(&e.name().as_ref());
                if local == "response" && in_response {
                    responses.push(std::mem::take(&mut current));
                    in_response = false;
                }
                path.pop();
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    responses
}

pub fn parse_calendar_query(xml: &str) -> Vec<(String, Option<String>, String)> {
    // href, etag, calendar-data
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut out = Vec::new();
    let mut href = String::new();
    let mut etag = None;
    let mut data = String::new();
    let mut in_response = false;
    let mut path: Vec<String> = Vec::new();
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let local = local_name(&e.name().as_ref());
                path.push(local.clone());
                if local == "response" {
                    in_response = true;
                    href.clear();
                    etag = None;
                    data.clear();
                }
            }
            Ok(Event::Text(t)) => {
                if !in_response {
                    continue;
                }
                let text = t.unescape().unwrap_or_default().to_string();
                let leaf = path.last().cloned().unwrap_or_default();
                match leaf.as_str() {
                    "href" if href.is_empty() => href = text,
                    "getetag" => etag = Some(text.trim_matches('"').to_owned()),
                    "calendar-data" => data.push_str(&text),
                    _ => {}
                }
            }
            Ok(Event::CData(t)) => {
                if path.last().map(|s| s.as_str()) == Some("calendar-data") {
                    data.push_str(&String::from_utf8_lossy(&t));
                }
            }
            Ok(Event::End(e)) => {
                let local = local_name(&e.name().as_ref());
                if local == "response" && in_response {
                    if !data.is_empty() {
                        out.push((href.clone(), etag.clone(), data.clone()));
                    }
                    in_response = false;
                }
                path.pop();
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

fn local_name(name: &[u8]) -> String {
    let full = String::from_utf8_lossy(name);
    let after_ns = full.rsplit('}').next().unwrap_or(&full);
    after_ns
        .rsplit(':')
        .next()
        .unwrap_or(after_ns)
        .to_ascii_lowercase()
}

pub fn propfind_body(props: &[&str]) -> String {
    let mut body = String::from(
        r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:cs="http://calendarserver.org/ns/" xmlns:x="http://apple.com/ns/ical/">
  <d:prop>
"#,
    );
    for prop in props {
        body.push_str("    ");
        body.push_str(prop);
        body.push('\n');
    }
    body.push_str("  </d:prop>\n</d:propfind>");
    body
}

pub fn calendar_query_body(start: &str, end: &str, expand: bool) -> String {
    let expand_xml = if expand {
        format!(
            r#"      <c:expand start="{start}" end="{end}"/>
"#
        )
    } else {
        String::new()
    };
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop>
    <d:getetag/>
    <c:calendar-data>
{expand_xml}    </c:calendar-data>
  </d:prop>
  <c:filter>
    <c:comp-filter name="VCALENDAR">
      <c:comp-filter name="VEVENT">
        <c:time-range start="{start}" end="{end}"/>
      </c:comp-filter>
    </c:comp-filter>
  </c:filter>
</c:calendar-query>"#
    )
}
