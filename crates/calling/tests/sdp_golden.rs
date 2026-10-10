use calling::sdp::{self, OfferPlan, SessionDescription, to_browser_answer, to_teams_offer};

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
