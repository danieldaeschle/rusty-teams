// Copyright 2025 LiveKit, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::fmt::Debug;

use crate::{
    imp::rtp_receiver as imp_rr, media_stream::MediaStream, media_stream_track::MediaStreamTrack,
    rtp_parameters::RtpParameters, stats::RtcStats, RtcError,
};

#[derive(Clone)]
pub struct RtpReceiver {
    pub(crate) handle: imp_rr::RtpReceiver,
}

impl RtpReceiver {
    pub fn track(&self) -> Option<MediaStreamTrack> {
        self.handle.track()
    }

    pub fn stream_ids(&self) -> Vec<String> {
        self.handle.stream_ids()
    }

    pub fn streams(&self) -> Vec<MediaStream> {
        self.handle.streams()
    }

    pub async fn get_stats(&self) -> Result<Vec<RtcStats>, RtcError> {
        self.handle.get_stats().await
    }

    pub fn parameters(&self) -> RtpParameters {
        self.handle.parameters()
    }

    /// The SSRC and CSRC sources of the last few seconds, like the browser's getSynchronizationSources and getContributingSources.
    pub fn sources(&self) -> Vec<RtpSource> {
        self.handle.sources()
    }
}

/// One RTP source; `audio_level` is in -dBov (0 loudest, 127 silent) when the packet carried it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RtpSource {
    pub source_id: u32,
    pub contributing: bool,
    pub audio_level: Option<u8>,
    pub timestamp_ms: i64,
}

impl Debug for RtpReceiver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RtpReceiver")
            .field("track", &self.track())
            .field("cname", &self.parameters().rtcp.cname)
            .finish()
    }
}
