use std::collections::BTreeMap;

use calling::{CallHealth, Limitation, NetworkLevel, StatsSample};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Series {
    pub min: f64,
    pub max: f64,
    pub last: f64,
    pub samples: u32,
}

impl Series {
    fn new(value: f64) -> Self {
        Series { min: value, max: value, last: value, samples: 1 }
    }

    fn observe(&mut self, value: f64) {
        self.min = self.min.min(value);
        self.max = self.max.max(value);
        self.last = value;
        self.samples += 1;
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct StatsSeries {
    values: BTreeMap<&'static str, Series>,
    pub stalled_audio_in: u32,
    pub stalled_audio_out: u32,
    pub level_samples: [u32; 3],
    pub limitation_samples: [u32; 3],
    pub cpu_limited_samples: u32,
    pub level_changes: Vec<(u128, &'static str)>,
    pub last_level: Option<NetworkLevel>,
}

pub fn level_name(level: NetworkLevel) -> &'static str {
    match level {
        NetworkLevel::Good => "good",
        NetworkLevel::Poor => "poor",
        NetworkLevel::Bad => "bad",
    }
}

pub fn limitation_name(limitation: Limitation) -> &'static str {
    match limitation {
        Limitation::None => "none",
        Limitation::Cpu => "cpu",
        Limitation::Bandwidth => "bandwidth",
    }
}

impl StatsSeries {
    pub fn values(&self) -> &BTreeMap<&'static str, Series> {
        &self.values
    }

    fn observe(&mut self, name: &'static str, value: f64) {
        match self.values.get_mut(name) {
            Some(series) => series.observe(value),
            None => {
                self.values.insert(name, Series::new(value));
            }
        }
    }

    pub fn observe_sample(&mut self, sample: &StatsSample) {
        let video = &sample.video;
        if let Some(before) = self.values.get("audio_in_packets")
            && sample.audio_in_packets <= before.last as u64
        {
            self.stalled_audio_in += 1;
        }
        if let Some(before) = self.values.get("audio_out_packets")
            && sample.audio_out_packets <= before.last as u64
        {
            self.stalled_audio_out += 1;
        }
        let readings: [(&'static str, f64); 16] = [
            ("audio_in_packets", sample.audio_in_packets as f64),
            ("audio_out_packets", sample.audio_out_packets as f64),
            ("audio_in_level", sample.audio_in_level),
            ("audio_out_level", sample.audio_out_level),
            ("video_in_packets", video.inbound_packets as f64),
            ("video_in_frames_decoded", video.inbound_frames_decoded as f64),
            ("video_in_width", f64::from(video.inbound_width)),
            ("video_in_height", f64::from(video.inbound_height)),
            ("video_out_packets", video.outbound_packets as f64),
            ("video_out_frames_encoded", video.outbound_frames_encoded as f64),
            ("video_out_width", f64::from(video.outbound_width)),
            ("video_out_height", f64::from(video.outbound_height)),
            ("video_remote_round_trip_ms", video.remote_round_trip_ms),
            ("share_in_packets", sample.share_in_packets as f64),
            ("share_out_packets", sample.share_out_packets as f64),
            ("video_frames_published", video.frames_published as f64),
        ];
        for (name, value) in readings {
            self.observe(name, value);
        }
    }

    pub fn observe_health(&mut self, health: &CallHealth, at_ms: u128) {
        let sample = &health.sample;
        for (name, value) in [
            ("loss_percent", sample.loss_percent),
            ("round_trip_ms", sample.round_trip_ms),
            ("jitter_ms", sample.jitter_ms),
            ("sent_kbps", sample.sent_kbps),
            ("received_kbps", sample.received_kbps),
        ] {
            self.observe(name, value);
        }
        self.level_samples[health.level as usize] += 1;
        self.limitation_samples[sample.limitation as usize] += 1;
        if health.cpu_limited {
            self.cpu_limited_samples += 1;
        }
        if self.last_level != Some(health.level) {
            self.level_changes.push((at_ms, level_name(health.level)));
            self.last_level = Some(health.level);
        }
    }
}

#[cfg(test)]
mod tests {
    use calling::{HealthSample, MediaPath, TransportDetails, call::VideoReport};

    use super::*;

    fn sample(audio_in: u64) -> StatsSample {
        StatsSample {
            audio_in_packets: audio_in,
            audio_out_packets: audio_in * 2,
            video: VideoReport { inbound_width: 1280, inbound_height: 720, ..VideoReport::default() },
            ..StatsSample::default()
        }
    }

    fn health(level: NetworkLevel, loss_percent: f64) -> CallHealth {
        CallHealth {
            level,
            cpu_limited: false,
            sample: HealthSample { loss_percent, ..HealthSample::default() },
            transport: TransportDetails::default(),
            path: MediaPath::Direct,
        }
    }

    #[test]
    fn samples_keep_min_max_last_and_count_stalls() {
        let mut series = StatsSeries::default();
        for packets in [10, 20, 20, 35] {
            series.observe_sample(&sample(packets));
        }
        let audio_in = series.values()["audio_in_packets"];
        assert_eq!((audio_in.min, audio_in.max, audio_in.last, audio_in.samples), (10., 35., 35., 4));
        assert_eq!(series.stalled_audio_in, 1);
        assert_eq!(series.values()["video_in_width"].last, 1280.);
    }

    #[test]
    fn health_keeps_ranges_and_level_changes() {
        let mut series = StatsSeries::default();
        series.observe_health(&health(NetworkLevel::Good, 0.), 2000);
        series.observe_health(&health(NetworkLevel::Poor, 7.), 4000);
        series.observe_health(&health(NetworkLevel::Poor, 6.), 6000);
        assert_eq!(series.values()["loss_percent"].max, 7.);
        assert_eq!(series.level_samples, [1, 2, 0]);
        assert_eq!(series.level_changes, [(2000, "good"), (4000, "poor")]);
    }
}
