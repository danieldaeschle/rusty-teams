use calling::sdp::{self, OfferPlan, SessionDescription, from_teams_offer, to_browser_answer, to_teams_answer, to_teams_offer};

const BROWSER_OFFER: &str = include_str!("fixtures/browser_offer.sdp");
const TEAMS_OFFER: &str = include_str!("fixtures/teams_offer.sdp");
const TEAMS_ANSWER: &str = include_str!("fixtures/teams_answer.sdp");
const BROWSER_ANSWER: &str = include_str!("fixtures/browser_answer.sdp");

fn captured_plan() -> OfferPlan {
    OfferPlan {
        screen_share_mids: vec!["18".into()],
        gallery_mids: (10..=17).map(|mid| mid.to_string()).collect(),
    }
}

fn counter() -> impl FnMut() -> u32 {
    let mut next = 1_000_000u32;
    move || {
        next += 1;
        next
    }
}

fn is_number(token: &str) -> bool {
    !token.is_empty() && token.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_address(token: &str) -> bool {
    let dotted_quad = token.split('.').count() == 4 && token.split('.').all(|part| part == "x" || is_number(part));
    token == "IP4" || token == "IP6" || dotted_quad || token.matches(':').count() >= 2
}

/// Masks what differs per call or capture run: SSRCs, ports, addresses, candidate foundations, session ids.
fn normalize(line: &str) -> String {
    let line = line.trim_end_matches('\r');
    if line.starts_with("o=") {
        return "o=<origin>".into();
    }
    if let Some(rest) = line.strip_prefix("a=x-ssrc-range:") {
        return format!("a=x-ssrc-range:{}", if rest == "1-1" { "1-1" } else { "<range>" });
    }
    let masked: Vec<String> = line
        .split(' ')
        .map(|token| {
            let (prefix, value) = token.split_once(':').filter(|(prefix, _)| prefix.starts_with("a=")).unwrap_or(("", token));
            let value_masked = if prefix.ends_with("cname") || token.starts_with("cname:") {
                "<C>".to_owned()
            } else if is_address(value) {
                "<addr>".to_owned()
            } else if is_number(value) && value.len() >= 4 && value != "8100" {
                "<n>".to_owned()
            } else if let Some((stream, _)) = value.rsplit_once('-').filter(|(_, number)| is_number(number)) {
                format!("{stream}-<n>")
            } else {
                value.to_owned()
            };
            if prefix.is_empty() { value_masked } else { format!("{prefix}:{value_masked}") }
        })
        .collect();
    masked.join(" ")
}

fn sections(description: &SessionDescription) -> Vec<Vec<String>> {
    let text = sdp::write(description);
    let mut sections = vec![Vec::new()];
    for line in text.split("\r\n").filter(|line| !line.is_empty()) {
        if line.starts_with("m=") {
            sections.push(Vec::new());
        }
        sections.last_mut().unwrap().push(normalize(line));
    }
    sections
}

fn assert_same(actual: Vec<Vec<String>>, expected: Vec<Vec<String>>, skip: impl Fn(usize, &str) -> bool) {
    assert_eq!(actual.len(), expected.len(), "section count");
    for (index, (actual, expected)) in actual.iter().zip(&expected).enumerate() {
        let actual: Vec<&String> = actual.iter().filter(|line| !skip(index, line)).collect();
        let expected: Vec<&String> = expected.iter().filter(|line| !skip(index, line)).collect();
        assert_eq!(actual, expected, "section {index}");
    }
}

/// Teams pins the first H264 payload of each video line to its own High-profile fmtp; that is codec policy, not dialect.
fn is_teams_codec_policy(line: &str) -> bool {
    line.starts_with("a=fmtp:102 ")
}

#[test]
fn captured_browser_offer_translates_to_the_captured_teams_offer() {
    let offer = to_teams_offer(BROWSER_OFFER, &captured_plan(), &mut counter()).unwrap();
    let actual = sections(&sdp::parse(&offer.sdp).unwrap());
    let expected = sections(&sdp::parse(TEAMS_OFFER).unwrap());
    assert_same(actual, expected, |_, line| is_teams_codec_policy(line));
    assert_eq!(offer.lines.len(), 13);
    assert_eq!(offer.lines[10].browser_mids.len(), 8);
}

/// The browser answer was captured in another run than the Teams answer, so per-run values are masked too:
/// bandwidth, video fmtp levels and the per-run `x-mediabw` session lines.
fn differs_per_run(index: usize, line: &str) -> bool {
    line.starts_with("b=AS:")
        || line.starts_with("a=x-mediabw:")
        || (index > 1 && line.starts_with("a=fmtp:") && !line.contains("apt="))
}

#[test]
fn captured_teams_answer_translates_to_the_captured_browser_answer() {
    let offer = to_teams_offer(BROWSER_OFFER, &captured_plan(), &mut counter()).unwrap();
    let answer = to_browser_answer(TEAMS_ANSWER, &offer, BROWSER_OFFER).unwrap();
    let actual = sections(&sdp::parse(&answer).unwrap());
    let expected = sections(&sdp::parse(BROWSER_ANSWER).unwrap());
    assert_same(actual, expected, differs_per_run);
}

fn kinds(description: &SessionDescription) -> Vec<String> {
    description.media.iter().map(|media| media.kind.clone()).collect()
}

#[test]
fn captured_teams_offer_translates_to_a_browser_offer() {
    let remote = from_teams_offer(TEAMS_OFFER).unwrap();
    let browser = sdp::parse(&remote.browser_sdp).unwrap();
    assert_eq!(browser.media.len(), 20);
    let mids: Vec<&str> = browser.media.iter().filter_map(|media| media.mid()).collect();
    assert_eq!(mids, (0..20).map(|mid| mid.to_string()).collect::<Vec<_>>().iter().map(String::as_str).collect::<Vec<_>>());
    let kinds = kinds(&browser);
    assert_eq!(kinds.iter().filter(|kind| *kind == "audio").count(), 1);
    assert_eq!(kinds.iter().filter(|kind| *kind == "video").count(), 18);
    assert_eq!(kinds.last().map(String::as_str), Some("application"));
    assert!(browser.media.iter().all(|media| media.port == "9"));
    assert!(browser.media[..19].iter().all(|media| media.proto == "UDP/TLS/RTP/SAVPF"));
    assert!(!remote.browser_sdp.contains("x-ssrc-range") && !remote.browser_sdp.contains("a=label"));
    assert!(!remote.browser_sdp.contains("10.10.10.10") && !remote.browser_sdp.contains("1234"));
    assert!(remote.browser_sdp.contains("a=group:BUNDLE 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19"));
    assert_eq!(remote.lines.len(), 13);
    assert_eq!(remote.lines[10].browser_mids.len(), 8);
}

#[test]
fn a_teams_offer_survives_the_trip_through_libwebrtc_form_and_back() {
    let remote = from_teams_offer(TEAMS_OFFER).unwrap();
    let back = to_teams_offer(&remote.browser_sdp, &remote.plan(), &mut counter()).unwrap();
    let actual = sections(&sdp::parse(&back.sdp).unwrap());
    let expected = sections(&sdp::parse(TEAMS_OFFER).unwrap());
    let per_run_ssrcs = |_: usize, line: &str| {
        is_teams_codec_policy(line) || line.starts_with("a=ssrc:") || line.starts_with("a=x-ssrc-range:")
    };
    assert_same(actual, expected, per_run_ssrcs);
    assert_eq!(back.lines, remote.lines);
}

#[test]
fn an_answer_to_a_teams_offer_comes_back_in_the_teams_dialect() {
    let remote = from_teams_offer(TEAMS_OFFER).unwrap();
    let answer = to_teams_answer(BROWSER_ANSWER, &remote, &mut counter()).unwrap();
    let described = sdp::parse(&answer.sdp).unwrap();
    let captured = sdp::parse(TEAMS_ANSWER).unwrap();
    assert_eq!(described.media.len(), captured.media.len());
    for (ours, theirs) in described.media.iter().zip(&captured.media) {
        assert_eq!(ours.kind, theirs.kind);
        assert_eq!(ours.value("label"), theirs.value("label"));
        assert_eq!(ours.port, "1234");
        assert_eq!(ours.proto, "RTP/SAVP");
    }
    assert_eq!(answer.lines, remote.lines);
    let bundle = described
        .session
        .attributes
        .iter()
        .find(|attribute| attribute.name == "group")
        .and_then(|attribute| attribute.value.clone())
        .unwrap();
    assert_eq!(bundle, "BUNDLE 0 1 2 3 4 5 6 7 8 9 17 18 19");
}
