#[cfg(test)]
mod tests {
    use super::super::xml::{parse_calendar_query, parse_multistatus};

    #[test]
    fn parses_propfind_calendar_collection() {
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:response>
    <d:href>/calendars/joe/personal/</d:href>
    <d:propstat>
      <d:prop>
        <d:displayname>Personal</d:displayname>
        <d:resourcetype><d:collection/><c:calendar/></d:resourcetype>
        <c:supported-calendar-component-set>
          <c:comp name="VEVENT"/>
        </c:supported-calendar-component-set>
        <d:current-user-privilege-set>
          <d:privilege><d:write/></d:privilege>
        </d:current-user-privilege-set>
      </d:prop>
    </d:propstat>
  </d:response>
</d:multistatus>"#;
        let responses = parse_multistatus(xml);
        assert_eq!(responses.len(), 1);
        assert!(responses[0].resource_type_calendar);
        assert!(responses[0].supports_vevent);
        assert!(responses[0].writable);
        assert_eq!(responses[0].displayname.as_deref(), Some("Personal"));
    }

    #[test]
    fn parses_calendar_query_data() {
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:response>
    <d:href>/calendars/joe/personal/abc.ics</d:href>
    <d:propstat>
      <d:prop>
        <d:getetag>"etag1"</d:getetag>
        <c:calendar-data>BEGIN:VCALENDAR
BEGIN:VEVENT
UID:1
SUMMARY:Hi
DTSTART:20260101T100000Z
DTEND:20260101T110000Z
END:VEVENT
END:VCALENDAR</c:calendar-data>
      </d:prop>
    </d:propstat>
  </d:response>
</d:multistatus>"#;
        let items = parse_calendar_query(xml);
        assert_eq!(items.len(), 1);
        assert!(items[0].2.contains("UID:1"));
        assert_eq!(items[0].1.as_deref(), Some("etag1"));
    }
}
