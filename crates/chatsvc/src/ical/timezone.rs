use chrono_tz::Tz;

/// Resolve a TZID to chrono-tz, including common Windows zone names from Outlook iMIP.
pub fn resolve_tzid(tzid: &str) -> Option<Tz> {
    let trimmed = tzid.trim().trim_matches('"');
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(tz) = trimmed.parse::<Tz>() {
        return Some(tz);
    }
    windows_to_iana(trimmed).and_then(|iana| iana.parse().ok())
}

fn windows_to_iana(name: &str) -> Option<&'static str> {
    Some(match name {
        "W. Europe Standard Time" => "Europe/Berlin",
        "Central European Standard Time" => "Europe/Warsaw",
        "Romance Standard Time" => "Europe/Paris",
        "GMT Standard Time" => "Europe/London",
        "UTC" | "Coordinated Universal Time" => "UTC",
        "Pacific Standard Time" => "America/Los_Angeles",
        "Mountain Standard Time" => "America/Denver",
        "Central Standard Time" => "America/Chicago",
        "Eastern Standard Time" => "America/New_York",
        "Tokyo Standard Time" => "Asia/Tokyo",
        "China Standard Time" => "Asia/Shanghai",
        "AUS Eastern Standard Time" => "Australia/Sydney",
        "Russian Standard Time" => "Europe/Moscow",
        "India Standard Time" => "Asia/Kolkata",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_windows_w_europe() {
        assert_eq!(
            resolve_tzid("W. Europe Standard Time"),
            Some(chrono_tz::Europe::Berlin)
        );
    }

    #[test]
    fn parses_iana() {
        assert_eq!(
            resolve_tzid("Europe/Zurich"),
            Some(chrono_tz::Europe::Zurich)
        );
    }
}
