use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

const MACROBLOCK: u32 = 16;
const FPS_UNIT: f64 = 100.0;
const BITRATE_UNIT: u64 = 1200;
const SCALE_STEP: f64 = 1.01;
const MAX_SCALE: f64 = 16.0;
pub const KEYFRAME_NUDGE: f64 = 1.1;
const KNOWN_KEYS: [&str; 10] =
    ["max-fs", "max-fps", "max-mbps", "max-br", "KeyFrame", "profile-level-id", "rid", "ssrc", "sequence-number", "packetization-mode"];

/// One layer of a mixer `controlVideoStreaming` request (VDCLC) for what we send.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SendLimit {
    pub max_fs: Option<u32>,
    pub max_mbps: Option<u32>,
    pub max_fps: Option<f64>,
    pub max_bitrate: Option<u64>,
    pub keyframe: bool,
    pub rid: Option<String>,
    pub ssrc: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SendRequest {
    pub source_id: Option<i64>,
    pub sequence: Option<i64>,
    pub limits: Vec<SendLimit>,
    pub shape: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EncoderLimits {
    pub scale_down_by: f64,
    pub max_framerate: Option<f64>,
    pub max_bitrate: Option<u64>,
}

fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(u8::from(*flag).to_string()),
        _ => None,
    }
}

fn pairs(value: &Value) -> BTreeMap<String, String> {
    match value {
        Value::String(text) => text
            .split(';')
            .filter_map(|pair| pair.split_once('='))
            .map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned()))
            .collect(),
        Value::Object(map) => map.iter().filter_map(|(key, value)| Some((key.clone(), text_of(value)?))).collect(),
        _ => BTreeMap::new(),
    }
}

fn limit_from(parameters: &BTreeMap<String, String>) -> SendLimit {
    let number = |key: &str| parameters.get(key).and_then(|value| value.parse::<f64>().ok());
    SendLimit {
        max_fs: number("max-fs").map(|value| value as u32),
        max_mbps: number("max-mbps").map(|value| value as u32),
        max_fps: number("max-fps").map(|value| value / FPS_UNIT),
        max_bitrate: number("max-br").map(|value| value as u64 * BITRATE_UNIT),
        keyframe: number("KeyFrame").is_some_and(|value| value != 0.0),
        rid: parameters.get("rid").cloned(),
        ssrc: parameters.get("ssrc").and_then(|value| value.parse().ok()),
    }
}

fn describe(parameters: &BTreeMap<String, String>) -> String {
    parameters
        .iter()
        .map(|(key, value)| if KNOWN_KEYS.contains(&key.as_str()) { format!("{key}={value}") } else { key.clone() })
        .collect::<Vec<_>>()
        .join(";")
}

pub fn parse_request(body: &Value) -> Option<SendRequest> {
    let control = body.get("controlVideoStreaming").unwrap_or(body);
    let info = match &control["controlInfo"] {
        Value::Array(items) => items.first()?,
        Value::Object(_) => &control["controlInfo"],
        _ => return None,
    };
    let layers: Vec<BTreeMap<String, String>> = match (&info["fmtParamsList"], &info["fmtParams"]) {
        (Value::Array(list), _) => list.iter().map(pairs).collect(),
        (_, Value::Array(list)) => list.iter().map(pairs).collect(),
        (_, single) if !single.is_null() => vec![pairs(single)],
        _ => Vec::new(),
    };
    let mut fields: Vec<&str> = info.as_object().map(|map| map.keys().map(String::as_str).collect()).unwrap_or_default();
    fields.sort_unstable();
    let shape = format!(
        "seq {} source {} control {} fields [{}] layers [{}]",
        control["sequenceNumber"],
        info["sourceId"],
        info["control"].as_str().unwrap_or("-"),
        fields.join(","),
        layers.iter().map(describe).collect::<Vec<_>>().join(" | ")
    );
    let layer_sequence = layers.iter().find_map(|layer| layer.get("sequence-number")?.parse().ok());
    Some(SendRequest {
        source_id: info["sourceId"].as_i64(),
        sequence: layer_sequence.or_else(|| control["sequenceNumber"].as_i64()),
        limits: layers.iter().map(limit_from).collect(),
        shape,
    })
}

/// Drops requests older than the last one per source and repeats that change nothing and ask for no keyframe.
#[derive(Debug, Default)]
pub struct RequestFilter {
    last_sequence: HashMap<Option<i64>, i64>,
    last_limits: HashMap<Option<i64>, Vec<SendLimit>>,
}

impl RequestFilter {
    pub fn admit(&mut self, request: &SendRequest) -> Option<Vec<SendLimit>> {
        if request.limits.is_empty() {
            return None;
        }
        if let Some(sequence) = request.sequence {
            let last = self.last_sequence.entry(request.source_id).or_insert(i64::MIN);
            if sequence < *last {
                return None;
            }
            *last = sequence;
        }
        let unchanged = self.last_limits.get(&request.source_id) == Some(&request.limits);
        self.last_limits.insert(request.source_id, request.limits.clone());
        let keyframe = request.limits.iter().any(|limit| limit.keyframe);
        (!unchanged || keyframe).then(|| request.limits.clone())
    }
}

fn macroblocks(width: f64, height: f64) -> u32 {
    ((width / f64::from(MACROBLOCK)).ceil() * (height / f64::from(MACROBLOCK)).ceil()) as u32
}

pub fn scale_for(width: u32, height: u32, max_fs: u32) -> f64 {
    let (width, height) = (f64::from(width), f64::from(height));
    if max_fs == 0 || macroblocks(width, height) <= max_fs {
        return 1.0;
    }
    let mut scale = (f64::from(macroblocks(width, height)) / f64::from(max_fs)).sqrt();
    while macroblocks(width / scale, height / scale) > max_fs && scale < MAX_SCALE {
        scale *= SCALE_STEP;
    }
    scale
}

pub fn encoder_limits(limit: &SendLimit, frame: Option<(u32, u32)>) -> EncoderLimits {
    let scale_down_by = match (limit.max_fs, frame) {
        (Some(max_fs), Some((width, height))) => scale_for(width, height, max_fs),
        _ => 1.0,
    };
    let rate_by_mbps = match (limit.max_mbps, frame) {
        (Some(max_mbps), Some((width, height))) if max_mbps > 0 => {
            let blocks = macroblocks(f64::from(width) / scale_down_by, f64::from(height) / scale_down_by).max(1);
            Some(f64::from(max_mbps) / f64::from(blocks))
        }
        _ => None,
    };
    let max_framerate = match (limit.max_fps, rate_by_mbps) {
        (Some(fps), Some(by_mbps)) => Some(fps.min(by_mbps)),
        (fps, by_mbps) => fps.or(by_mbps),
    }
    .filter(|fps| *fps > 0.0)
    .map(|fps| fps.max(1.0));
    EncoderLimits { scale_down_by, max_framerate, max_bitrate: limit.max_bitrate.filter(|bitrate| *bitrate > 0) }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayerPlan {
    pub rid: String,
    pub active: bool,
    pub limits: Option<EncoderLimits>,
    pub keyframe: bool,
}

pub fn plan_layers(limits: &[SendLimit], frame: Option<(u32, u32)>, rids: &[String]) -> Vec<LayerPlan> {
    let encoding_of = |limit: &SendLimit| {
        limit.rid.as_ref().and_then(|rid| rids.iter().position(|candidate| candidate == rid)).unwrap_or(0)
    };
    rids.iter()
        .enumerate()
        .map(|(index, rid)| {
            let requested = limits.iter().find(|limit| encoding_of(limit) == index);
            LayerPlan {
                rid: rid.clone(),
                active: requested.is_some(),
                limits: requested.map(|limit| encoder_limits(limit, frame)),
                keyframe: requested.is_some_and(|limit| limit.keyframe),
            }
        })
        .collect()
}

pub fn capture_macroblocks(width: u32, height: u32) -> u32 {
    macroblocks(f64::from(width), f64::from(height))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_signaling_request_with_a_string_fmtp_is_read() {
        let body = json!({"controlVideoStreaming": {"sequenceNumber": 12, "controlInfo": [
            {"control": "start", "sourceId": 405, "streamMsid": 3, "fmtParams": "max-fs=920;max-mbps=27600;max-fps=3000;KeyFrame=1;x-secret=abc"}
        ]}});
        let request = parse_request(&body).unwrap();
        assert_eq!(request.source_id, Some(405));
        assert_eq!(request.sequence, Some(12));
        assert_eq!(request.limits, vec![SendLimit { max_fs: Some(920), max_mbps: Some(27600), max_fps: Some(30.0), max_bitrate: None, keyframe: true, rid: None, ssrc: None }]);
        assert_eq!(
            request.shape,
            "seq 12 source 405 control start fields [control,fmtParams,sourceId,streamMsid] layers [KeyFrame=1;max-fps=3000;max-fs=920;max-mbps=27600;x-secret]"
        );
    }

    #[test]
    fn lists_objects_and_a_bare_control_info_are_read_too() {
        let listed = json!({"controlVideoStreaming": {"sequenceNumber": 1, "controlInfo": [
            {"sourceId": 405, "fmtParamsList": ["max-fs=3600;max-br=1500", "max-fs=920"]}
        ]}});
        let request = parse_request(&listed).unwrap();
        assert_eq!(request.limits.len(), 2);
        assert_eq!(request.limits[0].max_bitrate, Some(1_800_000));
        let objects = json!({"sequenceNumber": 2, "controlInfo": {"sourceId": 415, "fmtParams": [{"max-fs": 8160, "max-fps": 1500}]}});
        let request = parse_request(&objects).unwrap();
        assert_eq!(request.source_id, Some(415));
        assert_eq!(request.limits[0].max_fs, Some(8160));
        assert_eq!(request.limits[0].max_fps, Some(15.0));
        assert!(parse_request(&json!({"other": 1})).is_none());
    }

    #[test]
    fn stale_and_repeated_requests_are_dropped_unless_a_keyframe_is_asked() {
        let request = |sequence: i64, fmtp: &str| {
            parse_request(&json!({"sequenceNumber": sequence, "controlInfo": [{"sourceId": 405, "fmtParams": fmtp}]})).unwrap()
        };
        let mut filter = RequestFilter::default();
        assert!(filter.admit(&request(5, "max-fs=920")).is_some());
        assert!(filter.admit(&request(6, "max-fs=920")).is_none());
        assert!(filter.admit(&request(4, "max-fs=3600")).is_none());
        assert!(filter.admit(&request(7, "max-fs=920;KeyFrame=1")).is_some());
        assert_eq!(filter.admit(&request(8, "max-fs=3600")).unwrap()[0].max_fs, Some(3600));
    }

    #[test]
    fn the_scale_fits_the_frame_into_max_fs() {
        assert_eq!(scale_for(1280, 720, 3600), 1.0);
        let scale = scale_for(1280, 720, 920);
        assert!(macroblocks(1280.0 / scale, 720.0 / scale) <= 920);
        assert!(scale < 2.2, "{scale}");
        let scale = scale_for(960, 720, 920);
        assert!(macroblocks(960.0 / scale, 720.0 / scale) <= 920);
        assert_eq!(scale_for(1280, 720, 0), 1.0);
    }

    #[test]
    fn frame_rate_follows_max_fps_and_max_mbps() {
        let limit = SendLimit { max_fs: Some(920), max_mbps: Some(13_800), max_fps: Some(30.0), max_bitrate: Some(600_000), keyframe: false, ..SendLimit::default() };
        let limits = encoder_limits(&limit, Some((1280, 720)));
        assert!(limits.scale_down_by > 1.0);
        assert!(limits.max_framerate.unwrap() <= 30.0 && limits.max_framerate.unwrap() >= 14.0);
        assert_eq!(limits.max_bitrate, Some(600_000));
        let unknown_frame = encoder_limits(&limit, None);
        assert_eq!(unknown_frame.scale_down_by, 1.0);
        assert_eq!(unknown_frame.max_framerate, Some(30.0));
    }

    fn simulcast_request(sequence: i64, fmtp: [&str; 3]) -> SendRequest {
        parse_request(&json!({"sequenceNumber": sequence, "controlInfo": [{"sourceId": 405, "fmtParamsList": fmtp}]})).unwrap()
    }

    const THREE_LAYERS: [&str; 3] = [
        "max-fs=405;max-fps=3000;max-br=250;rid=1;ssrc=1111",
        "max-fs=920;max-fps=3000;max-br=500;rid=2;ssrc=1112",
        "max-fs=3600;max-fps=3000;max-br=1500;rid=3;ssrc=1113;KeyFrame=0",
    ];

    fn rids() -> Vec<String> {
        ["1", "2", "3"].map(String::from).to_vec()
    }

    #[test]
    fn three_layers_keep_their_rid_ssrc_and_limits() {
        let request = simulcast_request(1, THREE_LAYERS);
        let seen: Vec<(Option<&str>, Option<u32>, Option<u32>)> =
            request.limits.iter().map(|limit| (limit.rid.as_deref(), limit.ssrc, limit.max_fs)).collect();
        assert_eq!(seen, vec![(Some("1"), Some(1111), Some(405)), (Some("2"), Some(1112), Some(920)), (Some("3"), Some(1113), Some(3600))]);
        assert_eq!(request.limits[2].max_bitrate, Some(1_800_000));
    }

    #[test]
    fn a_missing_rid_switches_that_layer_off() {
        let request = simulcast_request(1, THREE_LAYERS);
        let limits: Vec<SendLimit> = request.limits.into_iter().filter(|limit| limit.rid.as_deref() != Some("2")).collect();
        let plans = plan_layers(&limits, Some((1280, 720)), &rids());
        assert_eq!(plans.iter().map(|plan| plan.active).collect::<Vec<_>>(), vec![true, false, true]);
        assert!(plans[1].limits.is_none());
        assert!(plans[0].limits.unwrap().scale_down_by > plans[2].limits.unwrap().scale_down_by);
        assert_eq!(plans[2].limits.unwrap().scale_down_by, 1.0);
    }

    #[test]
    fn an_entry_without_rid_drives_the_first_encoding() {
        let limits = [SendLimit { max_fs: Some(920), ..SendLimit::default() }];
        let plans = plan_layers(&limits, Some((1280, 720)), &rids());
        assert_eq!(plans.iter().map(|plan| plan.active).collect::<Vec<_>>(), vec![true, false, false]);
        assert!(plans[0].limits.unwrap().scale_down_by > 1.0);
    }

    #[test]
    fn the_keyframe_flag_stays_on_its_rid() {
        let mut limits = simulcast_request(1, THREE_LAYERS).limits;
        limits[1].keyframe = true;
        let plans = plan_layers(&limits, Some((1280, 720)), &rids());
        assert_eq!(plans.iter().map(|plan| plan.keyframe).collect::<Vec<_>>(), vec![false, true, false]);
    }

    #[test]
    fn the_filter_compares_the_whole_layer_set() {
        let mut filter = RequestFilter::default();
        assert_eq!(filter.admit(&simulcast_request(1, THREE_LAYERS)).unwrap().len(), 3);
        assert!(filter.admit(&simulcast_request(2, THREE_LAYERS)).is_none());
        let mut changed = THREE_LAYERS;
        changed[1] = "max-fs=405;max-fps=3000;max-br=250;rid=2;ssrc=1112";
        assert!(filter.admit(&simulcast_request(3, changed)).is_some());
        assert!(filter.admit(&simulcast_request(4, changed)).is_none());
        let mut keyframe = changed;
        keyframe[2] = "max-fs=3600;max-fps=3000;max-br=1500;rid=3;ssrc=1113;KeyFrame=1";
        assert!(filter.admit(&simulcast_request(5, keyframe)).is_some());
    }
}
