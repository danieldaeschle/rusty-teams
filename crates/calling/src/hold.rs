use crate::error::Result;
use crate::sdp::parse;

const DIRECTIONS: [&str; 4] = ["sendrecv", "sendonly", "recvonly", "inactive"];
pub const INACTIVE: &str = "inactive";
const REJECTED_PORT: &str = "0";
const DATA_KINDS: [&str; 2] = ["x-data", "application"];
const MAIN_VIDEO_LABEL: &str = "main-video";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HoldState {
    #[default]
    Active,
    Local,
    Remote,
}

impl HoldState {
    pub fn from_flags(local: bool, remote: bool) -> HoldState {
        if local {
            HoldState::Local
        } else if remote {
            HoldState::Remote
        } else {
            HoldState::Active
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MediaLine<'a> {
    pub kind: &'a str,
    pub mid: &'a str,
}

struct Section {
    kind: String,
    mid: String,
    direction_line: Option<usize>,
}

fn sections(lines: &[String]) -> Vec<Section> {
    let mut found: Vec<Section> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if let Some(rest) = line.strip_prefix("m=") {
            let kind = rest.split(' ').next().unwrap_or_default().to_owned();
            found.push(Section { kind, mid: String::new(), direction_line: None });
        } else if let Some(section) = found.last_mut() {
            if let Some(mid) = line.strip_prefix("a=mid:") {
                section.mid = mid.to_owned();
            } else if line.strip_prefix("a=").is_some_and(|name| DIRECTIONS.contains(&name)) {
                section.direction_line = Some(index);
            }
        }
    }
    found
}

pub fn rewrite_directions(sdp: &str, pick: impl Fn(MediaLine<'_>) -> Option<&'static str>) -> String {
    let newline = if sdp.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = sdp.split('\n').map(|line| line.trim_end_matches('\r').to_owned()).collect();
    for section in sections(&lines) {
        let Some(direction_line) = section.direction_line else { continue };
        if let Some(direction) = pick(MediaLine { kind: &section.kind, mid: &section.mid }) {
            lines[direction_line] = format!("a={direction}");
        }
    }
    lines.join(newline)
}

pub fn all_inactive(sdp: &str) -> String {
    rewrite_directions(sdp, |_| Some(INACTIVE))
}

pub fn is_hold(teams_sdp: &str) -> Result<bool> {
    let description = parse(teams_sdp)?;
    let mut media = description
        .media
        .iter()
        .filter(|line| line.port != REJECTED_PORT && !DATA_KINDS.contains(&line.kind.as_str()))
        .peekable();
    if media.peek().is_none() {
        return Ok(false);
    }
    Ok(media.all(|line| line.direction() == Some(INACTIVE)))
}

pub fn offer_sends_video(teams_sdp: &str) -> bool {
    parse(teams_sdp).is_ok_and(|description| {
        description.media.iter().any(|line| {
            line.kind == "video"
                && line.port != REJECTED_PORT
                && line.value("label").is_none_or(|label| label == MAIN_VIDEO_LABEL)
                && matches!(line.direction(), None | Some("sendrecv" | "sendonly"))
        })
    })
}

pub fn accepted_modalities(video: bool) -> Vec<String> {
    let mut modalities = vec!["Audio".to_owned()];
    if video {
        modalities.push("Video".to_owned());
    }
    modalities
}

#[cfg(test)]
mod tests {
    use super::*;

    const OFFER: &str = "v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\na=group:BUNDLE 0 1 2\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\nc=IN IP4 0.0.0.0\r\na=mid:0\r\na=sendrecv\r\na=rtpmap:111 opus/48000/2\r\nm=video 9 UDP/TLS/RTP/SAVPF 102\r\nc=IN IP4 0.0.0.0\r\na=mid:1\r\na=sendrecv\r\na=rtpmap:102 H264/90000\r\nm=video 9 UDP/TLS/RTP/SAVPF 102\r\nc=IN IP4 0.0.0.0\r\na=mid:2\r\na=recvonly\r\na=rtpmap:102 H264/90000\r\n";

    fn directions(sdp: &str) -> Vec<String> {
        parse(sdp).unwrap().media.iter().map(|line| line.direction().unwrap_or("none").to_owned()).collect()
    }

    #[test]
    fn holding_puts_every_line_on_inactive_and_nothing_else_changes() {
        let held = all_inactive(OFFER);
        assert_eq!(directions(&held), vec!["inactive", "inactive", "inactive"]);
        assert_eq!(held.lines().count(), OFFER.lines().count());
        assert!(held.contains("a=mid:2\r\na=inactive\r\n"));
        assert!(held.ends_with("\r\n"));
    }

    #[test]
    fn resuming_keeps_the_offer_as_libwebrtc_wrote_it() {
        let resumed = rewrite_directions(OFFER, |_| None);
        assert_eq!(resumed, OFFER);
        assert_eq!(directions(&resumed), vec!["sendrecv", "sendrecv", "recvonly"]);
    }

    #[test]
    fn single_lines_can_be_picked_by_kind_and_mid() {
        let picked = rewrite_directions(OFFER, |line| match (line.kind, line.mid) {
            ("video", "1") => Some("inactive"),
            ("video", "2") => Some("sendonly"),
            _ => None,
        });
        assert_eq!(directions(&picked), vec!["sendrecv", "inactive", "sendonly"]);
    }

    #[test]
    fn plain_line_feeds_survive() {
        let unix = OFFER.replace("\r\n", "\n");
        let held = all_inactive(&unix);
        assert!(!held.contains('\r'));
        assert_eq!(directions(&held), vec!["inactive", "inactive", "inactive"]);
    }

    #[test]
    fn a_hold_is_every_audio_and_video_line_inactive() {
        assert!(is_hold(&all_inactive(OFFER)).unwrap());
        assert!(!is_hold(OFFER).unwrap());
        let sendonly_audio = rewrite_directions(&all_inactive(OFFER), |line| (line.kind == "audio").then_some("sendonly"));
        assert!(!is_hold(&sendonly_audio).unwrap());
    }

    #[test]
    fn rejected_and_data_lines_do_not_stop_a_hold() {
        let teams = "v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\nm=audio 1234 RTP/SAVP 111\r\na=mid:0\r\na=inactive\r\nm=video 0 RTP/SAVP 102\r\na=mid:1\r\na=inactive\r\nm=x-data 1234 RTP/SAVP 127\r\na=mid:2\r\na=sendrecv\r\n";
        assert!(is_hold(teams).unwrap());
        assert!(!is_hold("v=0\r\ns=-\r\nt=0 0\r\n").unwrap());
    }

    #[test]
    fn an_offer_with_a_sending_main_video_line_is_a_video_call() {
        let teams = |direction: &str, label: &str| format!("v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\nm=audio 1234 RTP/SAVP 111\r\na=mid:0\r\na=sendrecv\r\nm=video 1234 RTP/SAVP 102\r\na=mid:1\r\na={direction}\r\na=label:{label}\r\n");
        assert!(offer_sends_video(&teams("sendrecv", "main-video")));
        assert!(offer_sends_video(&teams("sendonly", "main-video")));
        assert!(!offer_sends_video(&teams("recvonly", "main-video")));
        assert!(!offer_sends_video(&teams("inactive", "main-video")));
        assert!(!offer_sends_video(&teams("sendrecv", "applicationsharing-video")));
    }

    #[test]
    fn the_hold_state_prefers_your_own_hold() {
        assert_eq!(HoldState::from_flags(false, false), HoldState::Active);
        assert_eq!(HoldState::from_flags(true, true), HoldState::Local);
        assert_eq!(HoldState::from_flags(false, true), HoldState::Remote);
    }

    #[test]
    fn accepting_with_video_adds_the_modality() {
        assert_eq!(accepted_modalities(false), vec!["Audio"]);
        assert_eq!(accepted_modalities(true), vec!["Audio", "Video"]);
    }
}
