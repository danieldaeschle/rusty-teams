use crate::sdp::{self, Attribute};

pub fn teams_candidate(browser: &str) -> Option<String> {
    let line = browser.trim().trim_start_matches("a=").trim_start_matches("candidate:");
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let [foundation, component, transport, priority, address, port, "typ", kind, rest @ ..] = tokens.as_slice() else {
        return None;
    };
    if address.ends_with(".local") {
        return None;
    }
    let mut related = Vec::new();
    let mut tcp_type = None;
    for pair in rest.chunks(2) {
        match pair {
            ["raddr", value] | ["rport", value] => related.push(format!("{} {value}", pair[0])),
            ["tcptype", value] => tcp_type = Some(*value),
            _ => {}
        }
    }
    let transport = match (transport.to_ascii_lowercase().as_str(), tcp_type) {
        ("udp", _) => "UDP",
        ("tcp", Some("active")) => "TCP-ACT",
        ("tcp", Some("passive")) => "TCP-PASS",
        ("tcp", Some("so")) => "TCP-SO",
        _ => return None,
    };
    let mut text = format!("{foundation} {component} {transport} {priority} {address} {port} typ {kind}");
    for part in related {
        text.push(' ');
        text.push_str(&part);
    }
    Some(text)
}

fn candidate_ufrag(browser: &str) -> Option<&str> {
    let tokens: Vec<&str> = browser.split_whitespace().collect();
    tokens.windows(2).find(|pair| pair[0] == "ufrag").map(|pair| pair[1])
}

pub fn is_udp_relay(browser: &str) -> bool {
    let lower = browser.to_ascii_lowercase();
    lower.contains(" udp ") && lower.contains(" typ relay")
}

struct DefaultAddress {
    address: String,
    port: String,
}

fn default_address(candidates: &[String]) -> Option<DefaultAddress> {
    let rank = |kind: &str| match kind {
        "relay" => 0,
        "srflx" => 1,
        _ => 2,
    };
    candidates
        .iter()
        .filter_map(|candidate| {
            let tokens: Vec<&str> = candidate.split_whitespace().collect();
            (tokens.get(1) == Some(&"1")).then_some(())?;
            let transport_rank = if tokens.get(2) == Some(&"UDP") { 0 } else { 1 };
            Some(((transport_rank, rank(tokens.get(7)?)), tokens.get(4)?.to_string(), tokens.get(5)?.to_string()))
        })
        .min_by_key(|(rank, _, _)| *rank)
        .map(|(_, address, port)| DefaultAddress { address, port })
}

fn connection_for(address: &str) -> String {
    if address.contains(':') { format!("IN IP6 {address}") } else { format!("IN IP4 {address}") }
}

/// Replaces the placeholder transport of a signaled SDP with the gathered candidates, for a peer that is not the mixer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdpKind {
    Offer,
    Answer,
}

fn settled_role(sdp: &str) -> Option<&str> {
    sdp.lines()
        .filter_map(|line| line.trim_end_matches('\r').strip_prefix("a=setup:"))
        .find(|role| matches!(*role, "active" | "passive"))
}

/// The DTLS role this side took in the last negotiation; Teams keeps it in re-offers instead of `actpass`.
pub fn negotiated_setup(local_sdp: Option<&str>, remote_sdp: Option<&str>) -> Option<String> {
    if let Some(role) = local_sdp.and_then(settled_role) {
        return Some(role.to_owned());
    }
    match remote_sdp.and_then(settled_role)? {
        "active" => Some("passive".to_owned()),
        _ => Some("active".to_owned()),
    }
}

pub fn with_candidates(teams_sdp: &str, browser_candidates: &[String], kind: SdpKind) -> crate::error::Result<String> {
    let mut description = sdp::parse(teams_sdp)?;
    let ufrag = description.media.first().and_then(|media| media.value("ice-ufrag")).map(str::to_owned);
    let candidates: Vec<String> = browser_candidates
        .iter()
        .filter(|candidate| ufrag.as_deref().is_none_or(|ufrag| candidate_ufrag(candidate).is_none_or(|own| own == ufrag)))
        .filter_map(|candidate| teams_candidate(candidate))
        .collect();
    let Some(default) = default_address(&candidates) else {
        return Ok(teams_sdp.to_owned());
    };
    let connection = connection_for(&default.address);
    if description.session.connection.is_some() {
        description.session.connection = Some(connection.clone());
    }
    for media in &mut description.media {
        if media.port == "0" {
            continue;
        }
        media.port = default.port.clone();
        media.connection = Some(connection.clone());
        let carried_candidates = media.attributes.iter().any(|attribute| attribute.name == "candidate");
        let mut attributes = Vec::with_capacity(media.attributes.len() + candidates.len());
        for attribute in media.attributes.drain(..) {
            match attribute.name.as_str() {
                "candidate" => {}
                "rtcp" if kind == SdpKind::Offer => attributes.push(Attribute::with_value("rtcp", &default.port)),
                "rtcp" => {}
                _ => attributes.push(attribute),
            }
        }
        if carried_candidates {
            let position = attributes.iter().position(|attribute| attribute.name == "ssrc" || attribute.name == "label").unwrap_or(attributes.len());
            for (offset, candidate) in candidates.iter().enumerate() {
                attributes.insert(position + offset, Attribute::with_value("candidate", candidate));
            }
        }
        media.attributes = attributes;
    }
    Ok(sdp::write(&description))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_candidates_take_the_teams_form() {
        assert_eq!(
            teams_candidate("candidate:842163049 1 udp 1677729535 203.0.113.7 61001 typ srflx raddr 10.0.0.2 rport 61001 generation 0 ufrag abc network-id 1 network-cost 10").as_deref(),
            Some("842163049 1 UDP 1677729535 203.0.113.7 61001 typ srflx raddr 10.0.0.2 rport 61001")
        );
        assert_eq!(
            teams_candidate("a=candidate:1 1 tcp 1518280447 10.0.0.2 9 typ host tcptype active generation 0").as_deref(),
            Some("1 1 TCP-ACT 1518280447 10.0.0.2 9 typ host")
        );
        assert_eq!(teams_candidate("candidate:2 1 udp 2122260223 abcd-1234.local 50000 typ host generation 0"), None);
    }

    #[test]
    fn the_relay_candidate_becomes_the_default_transport() {
        let teams = "v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\ns=-\r\nc=IN IP4 10.10.10.10\r\nt=0 0\r\nm=audio 1234 RTP/SAVP 111\r\nc=IN IP4 10.10.10.10\r\na=rtcp:1234\r\na=candidate:1755259772 1 UDP 2122197247 10.10.10.10 1234 typ host\r\na=rtpmap:111 opus/48000/2\r\na=label:main-audio\r\na=mid:0\r\nm=video 1234 RTP/SAVP 102\r\nc=IN IP4 10.10.10.10\r\na=rtcp:1234\r\na=rtpmap:102 H264/90000\r\na=label:main-video\r\na=mid:1\r\nm=video 0 RTP/SAVP 102\r\na=mid:2\r\n";
        let gathered = vec![
            "candidate:1 1 udp 2122260223 10.0.0.2 50000 typ host generation 0".to_owned(),
            "candidate:2 1 udp 1677729535 203.0.113.7 61001 typ srflx raddr 10.0.0.2 rport 50000".to_owned(),
            "candidate:3 1 udp 41885439 52.114.0.9 3480 typ relay raddr 203.0.113.7 rport 61001".to_owned(),
        ];
        let text = with_candidates(teams, &gathered, SdpKind::Offer).unwrap();
        assert!(text.contains("m=audio 3480 RTP/SAVP 111"));
        assert!(text.contains("m=video 3480 RTP/SAVP 102"));
        assert!(text.contains("m=video 0 RTP/SAVP 102"));
        assert!(text.contains("c=IN IP4 52.114.0.9"));
        assert!(text.contains("a=rtcp:3480"));
        assert!(!text.contains("10.10.10.10"));
        assert_eq!(text.matches("a=candidate:").count(), 3);
        assert!(text.contains("a=candidate:3 1 UDP 41885439 52.114.0.9 3480 typ relay raddr 203.0.113.7 rport 61001"));
    }

    #[test]
    fn old_ice_generations_and_tcp_only_gathering_are_handled() {
        let teams = "v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\nm=audio 1234 RTP/SAVP 111\r\nc=IN IP4 10.10.10.10\r\na=ice-ufrag:new\r\na=candidate:1755259772 1 UDP 2122197247 10.10.10.10 1234 typ host\r\na=mid:0\r\n";
        let gathered = vec![
            "candidate:1 1 udp 2122260223 10.0.0.2 50000 typ host generation 0 ufrag old".to_owned(),
            "candidate:2 1 tcp 1518280447 10.0.0.2 9 typ host tcptype active generation 0 ufrag new".to_owned(),
        ];
        let text = with_candidates(teams, &gathered, SdpKind::Offer).unwrap();
        assert!(!text.contains("50000"));
        assert!(text.contains("m=audio 9 RTP/SAVP 111"));
        assert!(text.contains("a=candidate:2 1 TCP-ACT 1518280447 10.0.0.2 9 typ host"));
        assert!(is_udp_relay("candidate:3 1 udp 41885439 52.114.0.9 3480 typ relay raddr 1.2.3.4 rport 5"));
        assert!(!is_udp_relay("candidate:3 1 tcp 41885439 52.114.0.9 443 typ relay tcptype passive"));
    }

    #[test]
    fn without_candidates_the_sdp_stays_as_it_is() {
        assert_eq!(with_candidates("v=0\r\n", &[], SdpKind::Offer).unwrap(), "v=0\r\n");
        assert_eq!(negotiated_setup(Some("v=0\r\na=setup:actpass\r\na=setup:active\r\n"), None).as_deref(), Some("active"));
        assert_eq!(negotiated_setup(Some("a=setup:actpass\r\n"), Some("a=setup:active\r\n")).as_deref(), Some("passive"));
        assert_eq!(negotiated_setup(Some("a=setup:actpass\r\n"), None), None);
    }
}
