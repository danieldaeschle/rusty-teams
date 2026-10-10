use std::collections::{HashMap, HashSet};

use crate::error::{Error, Result};

const SIGNALED_PORT: &str = "1234";
const SIGNALED_PROTO: &str = "RTP/SAVP";
const SESSION_BANDWIDTH: &str = "CT:4000";
const PLACEHOLDER_CANDIDATE: &str = "1755259772 1 UDP 2122197247 10.10.10.10 1234 typ host";
const PLACEHOLDER_CONNECTION: &str = "IN IP4 10.10.10.10";
const SSRC_RANGE_SIZE: u32 = 100;
const BACKSLASHED_EXTENSIONS: [&str; 2] = [
    "http://www.webrtc.org/experiments/rtp-hdrext/abs-send-time",
    "http://www.ietf.org/id/draft-holmer-rmcat-transport-wide-cc-extensions-01",
];
const DROPPED_EXTENSIONS: [&str; 1] = ["http://www.webrtc.org/experiments/rtp-hdrext/playout-delay"];
const LAYERS_ALLOCATION: &str = "http://www.webrtc.org/experiments/rtp-hdrext/video-layers-allocation00";
const NOT_ADVERTISED_SUFFIX: &str = "-non-advertised";
const DIRECTIONS: [&str; 4] = ["sendrecv", "sendonly", "recvonly", "inactive"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    pub name: String,
    pub value: Option<String>,
}

impl Attribute {
    fn new(name: &str, value: impl Into<String>) -> Self {
        Attribute {
            name: name.to_owned(),
            value: Some(value.into()),
        }
    }

    fn flag(name: &str) -> Self {
        Attribute {
            name: name.to_owned(),
            value: None,
        }
    }

    fn render(&self) -> String {
        match &self.value {
            Some(value) => format!("a={}:{value}", self.name),
            None => format!("a={}", self.name),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Session {
    pub version: String,
    pub origin: String,
    pub name: String,
    pub connection: Option<String>,
    pub bandwidths: Vec<String>,
    pub timing: String,
    pub attributes: Vec<Attribute>,
}

#[derive(Debug, Clone)]
pub struct Media {
    pub kind: String,
    pub port: String,
    pub proto: String,
    pub formats: Vec<String>,
    pub connection: Option<String>,
    pub bandwidths: Vec<String>,
    pub attributes: Vec<Attribute>,
}

#[derive(Debug, Clone)]
pub struct SessionDescription {
    pub session: Session,
    pub media: Vec<Media>,
}

impl Media {
    pub fn values<'a>(&'a self, name: &str) -> impl Iterator<Item = &'a str> + 'a {
        let name = name.to_owned();
        self.attributes
            .iter()
            .filter(move |attribute| attribute.name == name)
            .filter_map(|attribute| attribute.value.as_deref())
    }

    pub fn value(&self, name: &str) -> Option<&str> {
        self.values(name).next()
    }

    pub fn has(&self, name: &str) -> bool {
        self.attributes.iter().any(|attribute| attribute.name == name)
    }

    pub fn mid(&self) -> Option<&str> {
        self.value("mid")
    }

    pub fn direction(&self) -> Option<&str> {
        self.attributes
            .iter()
            .find(|attribute| DIRECTIONS.contains(&attribute.name.as_str()))
            .map(|attribute| attribute.name.as_str())
    }

    fn rtpmap(&self) -> HashMap<String, String> {
        self.values("rtpmap")
            .filter_map(|value| value.split_once(' '))
            .map(|(payload, encoding)| (payload.to_owned(), encoding.to_owned()))
            .collect()
    }
}

pub fn parse(text: &str) -> Result<SessionDescription> {
    let mut session = Session::default();
    let mut media: Vec<Media> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let (kind, value) = line
            .split_once('=')
            .ok_or_else(|| Error::Sdp(format!("line without '=': {line}")))?;
        if kind == "m" {
            let mut parts = value.split(' ');
            let (Some(media_kind), Some(port), Some(proto)) = (parts.next(), parts.next(), parts.next()) else {
                return Err(Error::Sdp(format!("short m-line: {line}")));
            };
            media.push(Media {
                kind: media_kind.to_owned(),
                port: port.to_owned(),
                proto: proto.to_owned(),
                formats: parts.map(str::to_owned).collect(),
                connection: None,
                bandwidths: Vec::new(),
                attributes: Vec::new(),
            });
            continue;
        }
        let attribute = || {
            let (name, value) = match value.split_once(':') {
                Some((name, value)) => (name, Some(value.to_owned())),
                None => (value, None),
            };
            Attribute {
                name: name.to_owned(),
                value,
            }
        };
        match (media.last_mut(), kind) {
            (None, "v") => session.version = value.to_owned(),
            (None, "o") => session.origin = value.to_owned(),
            (None, "s") => session.name = value.to_owned(),
            (None, "c") => session.connection = Some(value.to_owned()),
            (None, "b") => session.bandwidths.push(value.to_owned()),
            (None, "t") => session.timing = value.to_owned(),
            (None, "a") => session.attributes.push(attribute()),
            (Some(current), "c") => current.connection = Some(value.to_owned()),
            (Some(current), "b") => current.bandwidths.push(value.to_owned()),
            (Some(current), "a") => current.attributes.push(attribute()),
            _ => {}
        }
    }
    Ok(SessionDescription { session, media })
}

fn session_rank(name: &str) -> usize {
    match name {
        "x-mediabw" => 0,
        "extmap-allow-mixed" => 1,
        "msid-semantic" => 2,
        "group" => 3,
        _ => 99,
    }
}

fn media_rank(name: &str) -> usize {
    match name {
        "x-multi-stream" => 0,
        "x-data-protocol" => 1,
        "x-signaling-fb" => 2,
        "x-ssrc-range" => 3,
        "x-source-streamid" => 4,
        "rtpmap" => 10,
        "fmtp" => 11,
        "rtcp" => 12,
        "rtcp-fb" => 13,
        "extmap" => 14,
        "extmap-allow-mixed" => 15,
        "setup" => 16,
        "mid" => 17,
        "msid" => 18,
        "ptime" => 19,
        "maxptime" => 20,
        "sendrecv" | "sendonly" | "recvonly" | "inactive" => 21,
        "ice-ufrag" => 22,
        "ice-pwd" => 23,
        "fingerprint" => 24,
        "candidate" => 25,
        "end-of-candidates" => 26,
        "ice-options" => 27,
        "ssrc" => 28,
        "ssrc-group" => 29,
        "rtcp-mux" => 30,
        "rtcp-rsize" => 31,
        "rid" => 32,
        "simulcast" => 33,
        "label" => 34,
        "sctp-port" => 35,
        "max-message-size" => 36,
        _ => 99,
    }
}

pub fn write(description: &SessionDescription) -> String {
    let session = &description.session;
    let mut lines = vec![
        format!("v={}", session.version),
        format!("o={}", session.origin),
        format!("s={}", session.name),
    ];
    lines.extend(session.connection.iter().map(|connection| format!("c={connection}")));
    lines.extend(session.bandwidths.iter().map(|bandwidth| format!("b={bandwidth}")));
    lines.push(format!("t={}", session.timing));
    let mut session_attributes: Vec<&Attribute> = session.attributes.iter().collect();
    session_attributes.sort_by_key(|attribute| session_rank(&attribute.name));
    lines.extend(session_attributes.into_iter().map(Attribute::render));
    for media in &description.media {
        let mut header = vec![media.kind.clone(), media.port.clone(), media.proto.clone()];
        header.extend(media.formats.iter().cloned());
        lines.push(format!("m={}", header.join(" ")));
        lines.extend(media.connection.iter().map(|connection| format!("c={connection}")));
        lines.extend(media.bandwidths.iter().map(|bandwidth| format!("b={bandwidth}")));
        let mut attributes: Vec<&Attribute> = media.attributes.iter().collect();
        attributes.sort_by_key(|attribute| media_rank(&attribute.name));
        lines.extend(attributes.into_iter().map(Attribute::render));
    }
    let mut text = lines.join("\r\n");
    text.push_str("\r\n");
    text
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineRole {
    MainAudio,
    MainVideo,
    ScreenShare,
    Data,
}

impl LineRole {
    fn label(self) -> &'static str {
        match self {
            LineRole::MainAudio => "main-audio",
            LineRole::MainVideo => "main-video",
            LineRole::ScreenShare => "applicationsharing-video",
            LineRole::Data => "data",
        }
    }

    fn stream_prefix(self) -> &'static str {
        match self {
            LineRole::MainAudio => "mainAudio",
            LineRole::MainVideo => "mainVideo",
            LineRole::ScreenShare => "applicationsharingVideo",
            LineRole::Data => "data",
        }
    }
}

/// Which browser m-lines play which Teams role; `gallery_mids` are folded into one `x-multi-stream` line.
#[derive(Debug, Clone, Default)]
pub struct OfferPlan {
    pub screen_share_mids: Vec<String>,
    pub gallery_mids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignaledLine {
    pub browser_mids: Vec<String>,
    pub role: LineRole,
}

#[derive(Debug, Clone)]
pub struct SignaledOffer {
    pub sdp: String,
    pub lines: Vec<SignaledLine>,
}

/// Random numbers for SSRCs; injected so tests stay deterministic.
pub trait NumberSource {
    fn next_u32(&mut self) -> u32;
}

impl<F: FnMut() -> u32> NumberSource for F {
    fn next_u32(&mut self) -> u32 {
        self()
    }
}

pub fn to_teams_offer(browser_sdp: &str, plan: &OfferPlan, numbers: &mut impl NumberSource) -> Result<SignaledOffer> {
    let browser = parse(browser_sdp)?;
    let cname = browser
        .media
        .iter()
        .flat_map(|media| media.values("ssrc"))
        .find_map(|value| value.split_once(" cname:").map(|(_, cname)| cname.to_owned()))
        .unwrap_or_else(|| format!("{:08x}{:08x}", numbers.next_u32(), numbers.next_u32()));
    let gallery_carrier = plan.gallery_mids.last().cloned();
    let mut lines = Vec::new();
    let mut media = Vec::new();
    for source in &browser.media {
        let mid = source.mid().unwrap_or_default().to_owned();
        if plan.gallery_mids.contains(&mid) && Some(&mid) != gallery_carrier.as_ref() {
            continue;
        }
        let role = match source.kind.as_str() {
            "audio" => LineRole::MainAudio,
            "application" => LineRole::Data,
            _ if plan.screen_share_mids.contains(&mid) => LineRole::ScreenShare,
            _ => LineRole::MainVideo,
        };
        let multi_stream = (Some(&mid) == gallery_carrier.as_ref()).then_some(plan.gallery_mids.len());
        let is_first = media.is_empty();
        let signaled = if source.port == "0" {
            signal_rejected_line(source, role)
        } else if role == LineRole::Data {
            signal_data_line(source, numbers)
        } else {
            signal_rtp_line(source, role, multi_stream, &cname, is_first, numbers)
        };
        media.push(signaled);
        lines.push(SignaledLine {
            browser_mids: match multi_stream {
                Some(_) => plan.gallery_mids.clone(),
                None => vec![mid],
            },
            role,
        });
    }
    let mut session = browser.session.clone();
    session.bandwidths = vec![SESSION_BANDWIDTH.to_owned()];
    for attribute in &mut session.attributes {
        match attribute.name.as_str() {
            "msid-semantic" => attribute.value = Some(" WMS *".to_owned()),
            "group" if attribute.value.as_deref().is_some_and(|value| value.starts_with("BUNDLE")) => {
                let mids: Vec<&str> = media.iter().filter(|line| line.port != "0").filter_map(Media::mid).collect();
                attribute.value = Some(format!("BUNDLE {}", mids.join(" ")));
            }
            _ => {}
        }
    }
    let description = SessionDescription { session, media };
    Ok(SignaledOffer {
        sdp: write(&description),
        lines,
    })
}

fn signal_rejected_line(source: &Media, role: LineRole) -> Media {
    let attributes = vec![
        Attribute::new("mid", source.mid().unwrap_or_default()),
        Attribute::flag("inactive"),
        Attribute::new("label", role.label()),
    ];
    Media {
        kind: if role == LineRole::Data { "x-data".to_owned() } else { source.kind.clone() },
        port: "0".to_owned(),
        proto: SIGNALED_PROTO.to_owned(),
        formats: source.formats.clone(),
        connection: Some(PLACEHOLDER_CONNECTION.to_owned()),
        bandwidths: Vec::new(),
        attributes,
    }
}

fn signal_rtp_line(
    source: &Media,
    role: LineRole,
    multi_stream: Option<usize>,
    cname: &str,
    carries_candidate: bool,
    numbers: &mut impl NumberSource,
) -> Media {
    let mut media = source.clone();
    media.port = SIGNALED_PORT.to_owned();
    media.proto = SIGNALED_PROTO.to_owned();
    media.connection = Some(PLACEHOLDER_CONNECTION.to_owned());
    let rtpmap = source.rtpmap();
    let is_rtx = |payload: &str| rtpmap.get(payload).is_some_and(|encoding| encoding.starts_with("rtx/"));
    let mid = source.mid().unwrap_or_default().to_owned();

    let mut attributes = Vec::new();
    if let Some(count) = multi_stream {
        attributes.push(Attribute::new("x-multi-stream", format!("{count} 1 1 1")));
    }
    attributes.push(Attribute::new(
        "x-signaling-fb",
        match role {
            LineRole::MainAudio => "* x-message app recv:dsh",
            _ => "* x-message app send:src recv:src,vc",
        },
    ));
    let browser_ssrcs: Vec<u32> = source
        .values("ssrc")
        .filter_map(|value| value.split(' ').next()?.parse().ok())
        .collect();
    let simulcast = source.has("simulcast");
    let receive_only = source.direction() == Some("recvonly");
    let (range, ssrc_lines) = if multi_stream.is_some() {
        (None, vec![Attribute::new("ssrc", format!("1 cname:{cname}"))])
    } else if let Some(&first) = browser_ssrcs.first() {
        let lines = source
            .values("ssrc")
            .map(|value| Attribute::new("ssrc", rewrite_ssrc_msid(value, &mid)))
            .collect();
        (Some((first, first)), lines)
    } else if simulcast {
        let base = numbers.next_u32();
        let lines = vec![Attribute::new("ssrc", format!("{base} fake_attribute:fake_value"))];
        (Some((base, base + SSRC_RANGE_SIZE - 1)), lines)
    } else if receive_only && role != LineRole::MainAudio {
        (Some((1, 1)), vec![Attribute::new("ssrc", format!("1 cname:{cname}"))])
    } else {
        let base = numbers.next_u32();
        (Some((base, base)), vec![Attribute::new("ssrc", format!("{base} cname:{cname}"))])
    };
    if let Some((first, last)) = range {
        attributes.push(Attribute::new("x-ssrc-range", format!("{first}-{last}")));
    }

    let video = source.kind == "video";
    let feedback_by_payload = feedback_by_payload(source);
    let shared_feedback = shared_feedback(&feedback_by_payload, source, &is_rtx);
    let mut fmtp: Vec<(usize, Attribute)> = Vec::new();
    for attribute in &source.attributes {
        let value = attribute.value.as_deref().unwrap_or_default();
        match attribute.name.as_str() {
            "ssrc" | "ssrc-group" | "rid" | "simulcast" | "rtcp-xr" | "rtcp-fb" => {}
            "rtpmap" => attributes.push(Attribute::new("rtpmap", signal_rtpmap(value))),
            "fmtp" => {
                let payload = value.split(' ').next().unwrap_or_default();
                let position = source.formats.iter().position(|format| format == payload).unwrap_or(usize::MAX);
                let cleaned = if video {
                    value.replace(";sps-pps-idr-in-keyframe=1", "")
                } else {
                    value.to_owned()
                };
                fmtp.push((position, Attribute::new("fmtp", cleaned)));
            }
            "rtcp" => attributes.push(Attribute::new("rtcp", SIGNALED_PORT)),
            "extmap" => {
                if let Some(signaled) = signal_extmap(value) {
                    attributes.push(Attribute::new("extmap", signaled));
                }
            }
            _ => attributes.push(attribute.clone()),
        }
    }
    fmtp.sort_by_key(|(position, _)| *position);
    attributes.extend(fmtp.into_iter().map(|(_, attribute)| attribute));
    match shared_feedback {
        Some(feedback) => attributes.extend(feedback.iter().map(|kind| Attribute::new("rtcp-fb", format!("* {kind}")))),
        None => attributes.extend(source.attributes.iter().filter(|attribute| attribute.name == "rtcp-fb").cloned()),
    }
    if carries_candidate {
        attributes.push(Attribute::new("candidate", PLACEHOLDER_CANDIDATE));
    }
    attributes.extend(ssrc_lines);
    attributes.extend(source.attributes.iter().filter(|attribute| attribute.name == "ssrc-group").cloned());
    attributes.push(Attribute::new("label", role.label()));
    if !source.has("rtcp-mux") {
        attributes.push(Attribute::flag("rtcp-mux"));
    }
    media.attributes = attributes;
    media
}

fn rewrite_ssrc_msid(value: &str, mid: &str) -> String {
    match value.split_once(" msid:") {
        Some((ssrc, msid)) => {
            let stream = msid.split(' ').next().unwrap_or("-");
            format!("{ssrc} msid:{stream} {mid}")
        }
        None => value.to_owned(),
    }
}

fn signal_rtpmap(value: &str) -> String {
    match value.split_once(' ') {
        Some((payload, encoding)) if encoding.eq_ignore_ascii_case("red/48000/2") => format!("{payload} RED/8000"),
        _ => value.to_owned(),
    }
}

fn signal_extmap(value: &str) -> Option<String> {
    let (identifier, uri) = value.split_once(' ')?;
    if DROPPED_EXTENSIONS.contains(&uri) {
        return None;
    }
    if BACKSLASHED_EXTENSIONS.contains(&uri) {
        return Some(format!("{identifier} {}", uri.replace('/', "\\")));
    }
    if uri == LAYERS_ALLOCATION {
        return Some(format!("{identifier} {uri}{NOT_ADVERTISED_SUFFIX}"));
    }
    Some(value.to_owned())
}

fn feedback_by_payload(source: &Media) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for value in source.values("rtcp-fb") {
        if let Some((payload, kind)) = value.split_once(' ') {
            map.entry(payload.to_owned()).or_default().push(kind.to_owned());
        }
    }
    map
}

fn shared_feedback(
    by_payload: &HashMap<String, Vec<String>>,
    source: &Media,
    is_rtx: &impl Fn(&str) -> bool,
) -> Option<Vec<String>> {
    let mut codecs = source.formats.iter().filter(|payload| !is_rtx(payload));
    let first = by_payload.get(codecs.next()?)?;
    codecs.all(|payload| by_payload.get(payload) == Some(first)).then(|| first.clone())
}

fn signal_data_line(source: &Media, numbers: &mut impl NumberSource) -> Media {
    let ssrc = numbers.next_u32();
    let mut attributes = vec![
        Attribute::new("x-data-protocol", "sctp"),
        Attribute::new("x-ssrc-range", format!("{ssrc}-{ssrc}")),
        Attribute::new("rtpmap", "127 x-data/90000"),
        Attribute::new("rtpmap", "126 rtx/90000"),
        Attribute::new("fmtp", "126 apt=127"),
        Attribute::flag("sendrecv"),
        Attribute::flag("rtcp-mux"),
        Attribute::new("label", LineRole::Data.label()),
    ];
    attributes.extend(source.attributes.iter().cloned());
    Media {
        kind: "x-data".to_owned(),
        port: SIGNALED_PORT.to_owned(),
        proto: SIGNALED_PROTO.to_owned(),
        formats: vec!["127".to_owned(), "126".to_owned()],
        connection: Some(PLACEHOLDER_CONNECTION.to_owned()),
        bandwidths: Vec::new(),
        attributes,
    }
}

pub fn to_browser_answer(teams_sdp: &str, offer: &SignaledOffer, browser_offer_sdp: &str) -> Result<String> {
    let answer = parse(teams_sdp)?;
    let browser_offer = parse(browser_offer_sdp)?;
    if answer.media.len() != offer.lines.len() {
        return Err(Error::Sdp(format!(
            "answer has {} m-lines, offer signaled {}",
            answer.media.len(),
            offer.lines.len()
        )));
    }
    let offered: HashMap<&str, &Media> = browser_offer
        .media
        .iter()
        .filter_map(|media| Some((media.mid()?, media)))
        .collect();
    let mut media = Vec::new();
    for (index, (teams_line, signaled)) in answer.media.iter().zip(&offer.lines).enumerate() {
        for (stream_index, browser_mid) in signaled.browser_mids.iter().enumerate() {
            let offered_line = offered
                .get(browser_mid.as_str())
                .ok_or_else(|| Error::Sdp(format!("offer has no mid {browser_mid}")))?;
            let section = if signaled.role == LineRole::Data {
                browser_data_line(teams_line, offered_line, &answer.session)
            } else {
                browser_rtp_line(teams_line, offered_line, signaled.role, stream_index as u32, index == 0, &answer.session)
            };
            media.push(section);
        }
    }
    let mut session = answer.session.clone();
    session.attributes.retain(|attribute| attribute.name.starts_with("x-"));
    session.attributes.push(Attribute::flag("extmap-allow-mixed"));
    session.attributes.push(Attribute::new("msid-semantic", " WMS *"));
    let mids: Vec<&str> = media.iter().filter_map(Media::mid).collect();
    session.attributes.push(Attribute::new("group", format!("BUNDLE {}", mids.join(" "))));
    Ok(write(&SessionDescription { session, media }))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamLine {
    pub mid: String,
    pub browser_mid: String,
    pub role: LineRole,
    pub source_stream_id: Option<u32>,
}

pub fn stream_lines(teams_sdp: &str, lines: &[SignaledLine]) -> Result<Vec<StreamLine>> {
    let teams = parse(teams_sdp)?;
    if teams.media.len() != lines.len() {
        return Err(Error::Sdp(format!("sdp has {} m-lines, {} were signaled", teams.media.len(), lines.len())));
    }
    let mut streams = Vec::new();
    for (media, line) in teams.media.iter().zip(lines) {
        let source_stream_id = media.value("x-source-streamid").and_then(|value| value.trim().parse().ok());
        let mid = media.mid().unwrap_or_default().to_owned();
        match line.browser_mids.as_slice() {
            [browser_mid] => streams.push(StreamLine {
                mid,
                browser_mid: browser_mid.clone(),
                role: line.role,
                source_stream_id,
            }),
            folded => streams.extend(folded.iter().map(|browser_mid| StreamLine {
                mid: mid.clone(),
                browser_mid: browser_mid.clone(),
                role: line.role,
                source_stream_id: None,
            })),
        }
    }
    Ok(streams)
}

fn transport_attributes(teams_line: &Media, session: &Session) -> Vec<Attribute> {
    ["setup", "ice-ufrag", "ice-pwd", "fingerprint"]
        .into_iter()
        .filter_map(|name| {
            teams_line
                .attributes
                .iter()
                .chain(&session.attributes)
                .find(|attribute| attribute.name == name)
                .cloned()
        })
        .collect()
}

fn browser_rtp_line(
    teams_line: &Media,
    offered: &Media,
    role: LineRole,
    stream_index: u32,
    carries_candidates: bool,
    session: &Session,
) -> Media {
    let offered_rtpmap = offered.rtpmap();
    let offered_codecs: HashMap<String, String> = offered_rtpmap
        .values()
        .filter_map(|encoding| {
            let (name, rest) = encoding.split_once('/')?;
            Some((name.to_ascii_lowercase(), rest.to_owned()))
        })
        .collect();
    let answer_rtpmap = teams_line.rtpmap();
    let kept: Vec<String> = teams_line
        .formats
        .iter()
        .filter(|payload| {
            answer_rtpmap
                .get(*payload)
                .and_then(|encoding| encoding.split('/').next())
                .is_some_and(|name| offered_codecs.contains_key(&name.to_ascii_lowercase()))
        })
        .cloned()
        .collect();
    let kept_set: HashSet<&str> = kept.iter().map(String::as_str).collect();
    let payload_of = |value: &str| value.split(' ').next().unwrap_or_default().to_owned();
    let codec_payload = |codec: &str| {
        kept.iter().find(|payload| {
            answer_rtpmap
                .get(*payload)
                .is_some_and(|encoding| encoding.to_ascii_lowercase().starts_with(&format!("{codec}/")))
        })
    };

    let mut attributes = Vec::new();
    attributes.extend(teams_line.values("x-source-streamid").map(|value| Attribute::new("x-source-streamid", value)));
    for value in teams_line.values("rtpmap") {
        let payload = payload_of(value);
        if !kept_set.contains(payload.as_str()) {
            continue;
        }
        let encoding = answer_rtpmap.get(&payload).cloned().unwrap_or_default();
        let name = encoding.split('/').next().unwrap_or_default();
        let offered_rest = offered_codecs.get(&name.to_ascii_lowercase());
        let rewritten = match offered_rest {
            Some(rest) if name.eq_ignore_ascii_case("red") => format!("{payload} {name}/{rest}"),
            _ => value.to_owned(),
        };
        attributes.push(Attribute::new("rtpmap", rewritten));
    }
    let mut fmtp_payloads = HashSet::new();
    for value in teams_line.values("fmtp") {
        let payload = payload_of(value);
        if !kept_set.contains(payload.as_str()) {
            continue;
        }
        fmtp_payloads.insert(payload);
        let cleaned = value
            .split(';')
            .filter(|parameter| !parameter.starts_with("rtx-time="))
            .collect::<Vec<_>>()
            .join(";");
        attributes.push(Attribute::new("fmtp", cleaned));
    }
    if let Some(opus) = codec_payload("opus").filter(|payload| !fmtp_payloads.contains(*payload)) {
        attributes.push(Attribute::new("fmtp", format!("{opus} usedtx=1")));
    }
    if let (Some(red), Some(opus)) = (codec_payload("red"), codec_payload("opus"))
        && !fmtp_payloads.contains(red)
    {
        attributes.push(Attribute::new("fmtp", format!("{red} {opus}/{opus}")));
    }
    attributes.extend(teams_line.values("rtcp").map(|value| Attribute::new("rtcp", value)));
    let answer_feedback: Vec<&str> = teams_line
        .values("rtcp-fb")
        .filter(|value| !value.contains("x-message"))
        .collect();
    if answer_feedback.is_empty() && !teams_line.has("rtcp-fb") {
        attributes.extend(
            offered
                .values("rtcp-fb")
                .filter(|value| kept_set.contains(payload_of(value).as_str()))
                .map(|value| Attribute::new("rtcp-fb", value)),
        );
    } else {
        attributes.extend(answer_feedback.into_iter().map(|value| Attribute::new("rtcp-fb", value)));
    }
    for value in teams_line.values("extmap") {
        let restored = value.replace('\\', "/").replace(NOT_ADVERTISED_SUFFIX, "");
        attributes.push(Attribute::new("extmap", restored));
    }
    attributes.extend(transport_attributes(teams_line, session));
    attributes.push(Attribute::new("mid", offered.mid().unwrap_or_default()));
    attributes.extend(teams_line.values("ptime").map(|value| Attribute::new("ptime", value)));
    if let Some(direction) = teams_line.direction() {
        attributes.push(Attribute::flag(direction));
    }
    if carries_candidates {
        attributes.extend(teams_line.values("candidate").map(|value| Attribute::new("candidate", browser_candidate(value))));
    }
    let (primary, retransmission) = stream_ssrcs(teams_line, stream_index);
    if let Some(primary) = primary {
        let stream = format!("{}-{primary}", role.stream_prefix());
        for ssrc in [Some(primary), retransmission].into_iter().flatten() {
            attributes.push(Attribute::new("ssrc", format!("{ssrc} cname:{}", stream_cname(primary))));
            attributes.push(Attribute::new("ssrc", format!("{ssrc} msid:{stream} {stream}")));
        }
        if let Some(retransmission) = retransmission {
            attributes.push(Attribute::new("ssrc-group", format!("FID {primary} {retransmission}")));
        }
    }
    attributes.push(Attribute::flag("rtcp-mux"));
    if teams_line.has("rtcp-rsize") {
        attributes.push(Attribute::flag("rtcp-rsize"));
    }
    if let Some(simulcast) = offered.value("simulcast").and_then(|value| value.strip_prefix("send ")) {
        let rids: Vec<String> = simulcast.split(';').map(|rid| rid.trim_start_matches('~').to_owned()).collect();
        attributes.extend(rids.iter().map(|rid| Attribute::new("rid", format!("{rid} recv"))));
        attributes.push(Attribute::new("simulcast", format!("recv {}", rids.join(";"))));
    }
    Media {
        kind: offered.kind.clone(),
        port: teams_line.port.clone(),
        proto: offered.proto.clone(),
        formats: kept,
        connection: teams_line.connection.clone().or_else(|| session.connection.clone()),
        bandwidths: teams_line.bandwidths.clone(),
        attributes,
    }
}

fn stream_cname(primary: u32) -> String {
    format!("teams{primary}")
}

fn stream_ssrcs(teams_line: &Media, stream_index: u32) -> (Option<u32>, Option<u32>) {
    let offset = stream_index * SSRC_RANGE_SIZE;
    if let Some(group) = teams_line.value("ssrc-group").and_then(|value| value.strip_prefix("FID ")) {
        let mut ssrcs = group.split(' ').filter_map(|value| value.parse::<u32>().ok());
        let primary = ssrcs.next().map(|ssrc| ssrc + offset);
        return (primary, ssrcs.next().map(|ssrc| ssrc + offset));
    }
    let first = teams_line
        .value("x-ssrc-range")
        .and_then(|range| range.split('-').next()?.parse::<u32>().ok())
        .map(|ssrc| ssrc + offset);
    (first, None)
}

fn browser_candidate(value: &str) -> String {
    let mut parts: Vec<&str> = value.split(' ').collect();
    if let Some(position) = parts.iter().position(|part| *part == "MTURNID") {
        parts.truncate(position);
    }
    let passive = parts.get(2) == Some(&"tcp-pass");
    if passive {
        parts[2] = "tcp";
    }
    let mut candidate = parts.join(" ");
    if passive {
        candidate.push_str(" tcptype passive");
    }
    candidate
}

fn browser_data_line(teams_line: &Media, offered: &Media, session: &Session) -> Media {
    let mut attributes = Vec::new();
    attributes.extend(teams_line.values("x-source-streamid").map(|value| Attribute::new("x-source-streamid", value)));
    attributes.extend(transport_attributes(teams_line, session));
    attributes.push(Attribute::new("mid", offered.mid().unwrap_or_default()));
    attributes.extend(offered.values("sctp-port").map(|value| Attribute::new("sctp-port", value)));
    Media {
        kind: offered.kind.clone(),
        port: teams_line.port.clone(),
        proto: offered.proto.clone(),
        formats: offered.formats.clone(),
        connection: teams_line.connection.clone().or_else(|| session.connection.clone()),
        bandwidths: Vec::new(),
        attributes,
    }
}

const BROWSER_PORT: &str = "9";
const BROWSER_PROTO: &str = "UDP/TLS/RTP/SAVPF";
const BROWSER_CONNECTION: &str = "IN IP4 0.0.0.0";
const BROWSER_RTCP: &str = "9 IN IP4 0.0.0.0";
const BROWSER_DATA_PROTO: &str = "UDP/DTLS/SCTP";
const BROWSER_DATA_FORMAT: &str = "webrtc-datachannel";
const SCTP_PORT: &str = "5000";
const SCTP_MAX_MESSAGE_SIZE: &str = "262144";
const PLACEHOLDER_CANDIDATE_TAIL: &str = " 1234 typ host";
const TEAMS_ONLY_ATTRIBUTES: [&str; 6] = [
    "x-multi-stream",
    "x-signaling-fb",
    "x-ssrc-range",
    "x-source-streamid",
    "x-data-protocol",
    "label",
];
const TRANSPORT_ATTRIBUTES: [&str; 5] = ["setup", "ice-ufrag", "ice-pwd", "fingerprint", "ice-options"];

/// A Teams offer translated for libwebrtc; `lines` remembers which browser mids each signaled line stands for.
#[derive(Debug, Clone)]
pub struct RemoteOffer {
    pub browser_sdp: String,
    pub lines: Vec<SignaledLine>,
}

impl RemoteOffer {
    pub fn plan(&self) -> OfferPlan {
        let mut plan = OfferPlan::default();
        for line in &self.lines {
            match line.role {
                LineRole::ScreenShare => plan.screen_share_mids.extend(line.browser_mids.iter().cloned()),
                _ if line.browser_mids.len() > 1 => plan.gallery_mids.extend(line.browser_mids.iter().cloned()),
                _ => {}
            }
        }
        plan
    }
}

fn role_of_teams_line(line: &Media) -> LineRole {
    match line.value("label") {
        Some("main-audio") => LineRole::MainAudio,
        Some("main-video") => LineRole::MainVideo,
        Some("applicationsharing-video") => LineRole::ScreenShare,
        Some("data") => LineRole::Data,
        _ => match line.kind.as_str() {
            "audio" => LineRole::MainAudio,
            "x-data" | "application" => LineRole::Data,
            _ => LineRole::MainVideo,
        },
    }
}

/// Folded gallery lines get the mids just below the carrier line's own mid.
fn folded_mids(carrier: &str, count: usize) -> Vec<String> {
    match carrier.parse::<usize>() {
        Ok(last) if last + 1 >= count => (last + 1 - count..=last).map(|mid| mid.to_string()).collect(),
        _ => (0..count)
            .map(|index| if index + 1 == count { carrier.to_owned() } else { format!("{carrier}-{index}") })
            .collect(),
    }
}

pub fn from_teams_offer(teams_sdp: &str) -> Result<RemoteOffer> {
    let teams = parse(teams_sdp)?;
    let mut media = Vec::new();
    let mut lines = Vec::new();
    for source in &teams.media {
        let role = role_of_teams_line(source);
        let carrier = source.mid().unwrap_or_default().to_owned();
        let folded = source
            .value("x-multi-stream")
            .and_then(|value| value.split(' ').next()?.parse::<usize>().ok())
            .filter(|count| *count > 1);
        let mids = match folded {
            Some(count) => folded_mids(&carrier, count),
            None => vec![carrier.clone()],
        };
        for (index, mid) in mids.iter().enumerate() {
            let converted = if role == LineRole::Data {
                browser_data_from_teams(source, mid, &teams.session)
            } else {
                browser_rtp_from_teams(source, mid, folded.is_some(), index == 0 && lines.is_empty(), &teams.session)
            };
            media.push(converted);
        }
        lines.push(SignaledLine {
            browser_mids: mids,
            role,
        });
    }
    let mut session = teams.session.clone();
    session.bandwidths.clear();
    session.connection = None;
    session.attributes.retain(|attribute| matches!(attribute.name.as_str(), "extmap-allow-mixed" | "msid-semantic"));
    let mids: Vec<&str> = media.iter().filter_map(Media::mid).collect();
    session.attributes.push(Attribute::new("group", format!("BUNDLE {}", mids.join(" "))));
    Ok(RemoteOffer {
        browser_sdp: write(&SessionDescription { session, media }),
        lines,
    })
}

fn transport_from_teams(source: &Media, session: &Session) -> Vec<Attribute> {
    TRANSPORT_ATTRIBUTES
        .iter()
        .filter_map(|name| {
            source
                .attributes
                .iter()
                .chain(&session.attributes)
                .find(|attribute| attribute.name == *name)
                .cloned()
        })
        .collect()
}

fn browser_data_from_teams(source: &Media, mid: &str, session: &Session) -> Media {
    let mut attributes = transport_from_teams(source, session);
    attributes.push(Attribute::new("mid", mid));
    attributes.push(Attribute::new("sctp-port", SCTP_PORT));
    attributes.push(Attribute::new("max-message-size", SCTP_MAX_MESSAGE_SIZE));
    Media {
        kind: "application".to_owned(),
        port: BROWSER_PORT.to_owned(),
        proto: BROWSER_DATA_PROTO.to_owned(),
        formats: vec![BROWSER_DATA_FORMAT.to_owned()],
        connection: Some(BROWSER_CONNECTION.to_owned()),
        bandwidths: Vec::new(),
        attributes,
    }
}

fn browser_rtp_from_teams(source: &Media, mid: &str, folded: bool, carries_candidate: bool, session: &Session) -> Media {
    let rtpmap = source.rtpmap();
    let is_rtx = |payload: &str| rtpmap.get(payload).is_some_and(|encoding| encoding.starts_with("rtx/"));
    let mut attributes = Vec::new();
    let mut expanded_feedback: Vec<Attribute> = Vec::new();
    for attribute in &source.attributes {
        let value = attribute.value.as_deref().unwrap_or_default();
        match attribute.name.as_str() {
            name if TEAMS_ONLY_ATTRIBUTES.contains(&name) => {}
            name if name.starts_with("x-") => {}
            "mid" | "rtcp-mux" => {}
            name if TRANSPORT_ATTRIBUTES.contains(&name) => {}
            "candidate" => {
                if carries_candidate && !value.contains(PLACEHOLDER_CANDIDATE_TAIL) {
                    attributes.push(attribute.clone());
                }
            }
            "ssrc" => {
                let placeholder = value.contains("fake_attribute") || value.starts_with("1 ");
                if !folded && !placeholder {
                    attributes.push(attribute.clone());
                }
            }
            "ssrc-group" => {
                if !folded {
                    attributes.push(attribute.clone());
                }
            }
            "rtcp" => attributes.push(Attribute::new("rtcp", BROWSER_RTCP)),
            "rtpmap" => attributes.push(Attribute::new("rtpmap", browser_rtpmap(value))),
            "extmap" => attributes.push(Attribute::new("extmap", browser_extmap(value))),
            "rtcp-fb" => match value.strip_prefix("* ") {
                Some(kind) => expanded_feedback.extend(
                    source
                        .formats
                        .iter()
                        .filter(|payload| !is_rtx(payload))
                        .map(|payload| Attribute::new("rtcp-fb", format!("{payload} {kind}"))),
                ),
                None => attributes.push(attribute.clone()),
            },
            _ => attributes.push(attribute.clone()),
        }
    }
    attributes.extend(expanded_feedback);
    attributes.extend(transport_from_teams(source, session));
    attributes.push(Attribute::new("mid", mid));
    attributes.push(Attribute::flag("rtcp-mux"));
    Media {
        kind: source.kind.clone(),
        port: BROWSER_PORT.to_owned(),
        proto: BROWSER_PROTO.to_owned(),
        formats: source.formats.clone(),
        connection: Some(BROWSER_CONNECTION.to_owned()),
        bandwidths: Vec::new(),
        attributes,
    }
}

fn browser_rtpmap(value: &str) -> String {
    match value.split_once(' ') {
        Some((payload, encoding)) if encoding.eq_ignore_ascii_case("red/8000") => format!("{payload} red/48000/2"),
        _ => value.to_owned(),
    }
}

fn browser_extmap(value: &str) -> String {
    value.replace('\\', "/").replace(NOT_ADVERTISED_SUFFIX, "")
}

/// libwebrtc answers every offered line in the offer's order and with its mids, so the offer's plan applies unchanged.
pub fn to_teams_answer(browser_answer_sdp: &str, remote: &RemoteOffer, numbers: &mut impl NumberSource) -> Result<SignaledOffer> {
    to_teams_offer(browser_answer_sdp, &remote.plan(), numbers)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counter() -> impl FnMut() -> u32 {
        let mut next = 1000u32;
        move || {
            next += 1;
            next
        }
    }

    const AUDIO_ONLY_OFFER: &str = "v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\na=group:BUNDLE 0\r\na=extmap-allow-mixed\r\na=msid-semantic: WMS stream\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111 63 9 0 8 13 110 126\r\nc=IN IP4 0.0.0.0\r\na=rtcp:9 IN IP4 0.0.0.0\r\na=ice-ufrag:u\r\na=ice-pwd:p\r\na=ice-options:trickle\r\na=fingerprint:sha-256 AA\r\na=setup:actpass\r\na=mid:0\r\na=extmap:1 urn:ietf:params:rtp-hdrext:ssrc-audio-level\r\na=extmap:2 http://www.webrtc.org/experiments/rtp-hdrext/abs-send-time\r\na=sendrecv\r\na=msid:stream track\r\na=rtcp-mux\r\na=rtpmap:111 opus/48000/2\r\na=rtcp-fb:111 transport-cc\r\na=fmtp:111 minptime=10;useinbandfec=1\r\na=rtpmap:63 red/48000/2\r\na=fmtp:63 111/111\r\na=rtpmap:9 G722/8000\r\na=rtpmap:0 PCMU/8000\r\na=rtpmap:8 PCMA/8000\r\na=rtpmap:13 CN/8000\r\na=rtpmap:110 telephone-event/48000\r\na=rtpmap:126 telephone-event/8000\r\na=ssrc:42 cname:abc\r\na=ssrc:42 msid:stream track\r\n";

    #[test]
    fn audio_only_offer_gets_the_teams_shape() {
        let offer = to_teams_offer(AUDIO_ONLY_OFFER, &OfferPlan::default(), &mut counter()).unwrap();
        let text = offer.sdp.replace("\r\n", "\n");
        assert!(text.contains("\nb=CT:4000\n"));
        assert!(text.contains("m=audio 1234 RTP/SAVP 111 63 9 0 8 13 110 126\n"));
        assert!(text.contains("a=x-signaling-fb:* x-message app recv:dsh\n"));
        assert!(text.contains("a=x-ssrc-range:42-42\n"));
        assert!(text.contains("a=rtpmap:63 RED/8000\n"));
        assert!(text.contains("a=rtcp:1234\n"));
        assert!(text.contains("a=extmap:2 http:\\\\www.webrtc.org\\experiments\\rtp-hdrext\\abs-send-time\n"));
        assert!(text.contains("a=ssrc:42 msid:stream 0\n"));
        assert!(text.contains("a=candidate:1755259772 1 UDP 2122197247 10.10.10.10 1234 typ host\n"));
        assert!(text.contains("m=audio 1234 RTP/SAVP 111 63 9 0 8 13 110 126\nc=IN IP4 10.10.10.10\n"));
        assert!(text.ends_with("a=label:main-audio\n"));
        assert_eq!(offer.lines, vec![SignaledLine { browser_mids: vec!["0".into()], role: LineRole::MainAudio }]);
    }

    #[test]
    fn answer_candidates_lose_teams_extensions() {
        assert_eq!(
            browser_candidate("1 1 UDP 54001663 52.112.0.1 3480 typ relay raddr 10.0.0.1 rport 3480 MTURNID 158"),
            "1 1 UDP 54001663 52.112.0.1 3480 typ relay raddr 10.0.0.1 rport 3480"
        );
        assert_eq!(
            browser_candidate("3 1 tcp-pass 18087935 52.112.0.1 3478 typ relay raddr 10.0.0.1 rport 3478"),
            "3 1 tcp 18087935 52.112.0.1 3478 typ relay raddr 10.0.0.1 rport 3478 tcptype passive"
        );
    }

    #[test]
    fn audio_answer_maps_back_to_the_browser_mid_and_codecs() {
        let offer = to_teams_offer(AUDIO_ONLY_OFFER, &OfferPlan::default(), &mut counter()).unwrap();
        let answer = "v=0\r\no=- 7 0 IN IP4 52.112.0.1\r\ns=session\r\nc=IN IP4 52.112.0.1\r\nb=CT:10000000\r\nt=0 0\r\na=group:BUNDLE 0\r\nm=audio 3480 RTP/SAVP 109 111 97 13 101\r\nc=IN IP4 52.112.0.1\r\na=x-source-streamid:201\r\na=rtcp:3480\r\na=setup:passive\r\na=ice-ufrag:U\r\na=ice-pwd:P\r\na=rtcp-mux\r\na=candidate:1 1 UDP 54001663 52.112.0.1 3480 typ relay raddr 10.0.0.1 rport 3480 MTURNID 9\r\na=label:main-audio\r\na=mid:0\r\na=rtpmap:109 SATINFB/48000/2\r\na=rtpmap:111 OPUS/48000/2\r\na=rtpmap:97 RED/8000\r\na=rtpmap:13 CN/8000\r\na=rtpmap:101 telephone-event/8000\r\na=fmtp:101 0-16\r\na=fingerprint:sha-256 BB\r\na=x-ssrc-range:1000-1000\r\n";
        let browser = to_browser_answer(answer, &offer, AUDIO_ONLY_OFFER).unwrap().replace("\r\n", "\n");
        assert!(browser.contains("m=audio 3480 UDP/TLS/RTP/SAVPF 111 97 13 101\n"), "{browser}");
        assert!(browser.contains("a=rtpmap:97 RED/48000/2\n"));
        assert!(browser.contains("a=fmtp:111 usedtx=1\n"));
        assert!(browser.contains("a=fmtp:97 111/111\n"));
        assert!(browser.contains("a=rtcp-fb:111 transport-cc\n"));
        assert!(browser.contains("a=ssrc:1000 msid:mainAudio-1000 mainAudio-1000\n"));
        assert!(browser.contains("a=group:BUNDLE 0\n"));
        assert!(!browser.contains("SATINFB") && !browser.contains("MTURNID") && !browser.contains("label"));
    }
    #[test]
    fn an_audio_offer_from_teams_gets_the_browser_shape_back() {
        let offer = to_teams_offer(AUDIO_ONLY_OFFER, &OfferPlan::default(), &mut counter()).unwrap();
        let remote = from_teams_offer(&offer.sdp).unwrap();
        let text = remote.browser_sdp.replace("\r\n", "\n");
        assert!(text.contains("m=audio 9 UDP/TLS/RTP/SAVPF 111 63 9 0 8 13 110 126\n"), "{text}");
        assert!(text.contains("c=IN IP4 0.0.0.0\n"));
        assert!(text.contains("a=rtcp:9 IN IP4 0.0.0.0\n"));
        assert!(text.contains("a=rtpmap:63 red/48000/2\n"));
        assert!(text.contains("a=extmap:2 http://www.webrtc.org/experiments/rtp-hdrext/abs-send-time\n"));
        assert!(text.contains("a=ice-ufrag:u\n") && text.contains("a=fingerprint:sha-256 AA\n") && text.contains("a=setup:actpass\n"));
        assert!(text.contains("a=mid:0\n") && text.contains("a=group:BUNDLE 0\n"));
        assert!(!text.contains("10.10.10.10") && !text.contains("x-ssrc-range") && !text.contains("label") && !text.contains("1234"));
        assert_eq!(remote.lines, offer.lines);
    }

    #[test]
    fn shared_video_feedback_is_spelled_out_per_codec_again() {
        let teams = "v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\na=group:BUNDLE 0\r\nm=video 1234 RTP/SAVP 102 103\r\nc=IN IP4 10.10.10.10\r\na=rtpmap:102 H264/90000\r\na=rtpmap:103 rtx/90000\r\na=fmtp:103 apt=102\r\na=rtcp-fb:* nack\r\na=rtcp-fb:* nack pli\r\na=mid:0\r\na=sendrecv\r\na=ice-ufrag:u\r\na=ice-pwd:p\r\na=fingerprint:sha-256 AA\r\na=setup:actpass\r\na=label:main-video\r\n";
        let remote = from_teams_offer(teams).unwrap();
        let text = remote.browser_sdp.replace("\r\n", "\n");
        assert!(text.contains("a=rtcp-fb:102 nack\n") && text.contains("a=rtcp-fb:102 nack pli\n"));
        assert!(!text.contains("a=rtcp-fb:103") && !text.contains("a=rtcp-fb:*"));
    }

    #[test]
    fn a_rejected_video_line_stays_rejected_and_out_of_the_bundle() {
        let audio_and_video = "v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\na=group:BUNDLE 0 1\r\nm=audio 1234 RTP/SAVP 111\r\nc=IN IP4 10.10.10.10\r\na=rtpmap:111 opus/48000/2\r\na=mid:0\r\na=sendrecv\r\na=ice-ufrag:u\r\na=ice-pwd:p\r\na=fingerprint:sha-256 AA\r\na=setup:actpass\r\na=label:main-audio\r\nm=video 1234 RTP/SAVP 102\r\nc=IN IP4 10.10.10.10\r\na=rtpmap:102 H264/90000\r\na=mid:1\r\na=sendrecv\r\na=label:main-video\r\n";
        let remote = from_teams_offer(audio_and_video).unwrap();
        let answer = "v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\na=group:BUNDLE 0\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\nc=IN IP4 0.0.0.0\r\na=rtpmap:111 opus/48000/2\r\na=mid:0\r\na=sendrecv\r\na=ice-ufrag:u\r\na=ice-pwd:p\r\na=fingerprint:sha-256 BB\r\na=setup:active\r\na=ssrc:7 cname:x\r\nm=video 0 UDP/TLS/RTP/SAVPF 102\r\nc=IN IP4 0.0.0.0\r\na=rtpmap:102 H264/90000\r\na=mid:1\r\na=inactive\r\n";
        let signaled = to_teams_answer(answer, &remote, &mut counter()).unwrap();
        let text = signaled.sdp.replace("\r\n", "\n");
        assert!(text.contains("m=video 0 RTP/SAVP 102\n"), "{text}");
        assert!(text.contains("a=group:BUNDLE 0\n"));
        let parsed = parse(&signaled.sdp).unwrap();
        assert_eq!(parsed.media.len(), 2);
        assert_eq!(parsed.media[1].value("label"), Some("main-video"));
        assert_eq!(parsed.media[1].direction(), Some("inactive"));
    }

    #[test]
    fn folded_gallery_lines_expand_below_their_carrier_mid() {
        assert_eq!(folded_mids("17", 8), (10..=17).map(|mid| mid.to_string()).collect::<Vec<_>>());
        assert_eq!(folded_mids("x", 2), vec!["x-0".to_owned(), "x".to_owned()]);
    }
}
