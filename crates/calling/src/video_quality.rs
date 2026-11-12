/// User-facing camera / stage video quality ceiling. No 4K mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VideoQuality {
    #[default]
    Auto,
    P720,
    P1080,
}

impl VideoQuality {
    /// Preferred capture height for this choice (Auto prefers 1080).
    pub fn capture_height(self) -> u32 {
        match self {
            Self::P720 => 720,
            Self::Auto | Self::P1080 => 1080,
        }
    }

    pub fn capture_width(self) -> u32 {
        self.capture_height() * 16 / 9
    }

    /// Highest sensor height we may open before downscaling to the capture ceiling.
    pub fn largest_sensor_height(self) -> u32 {
        match self {
            Self::P720 => 1080,
            Self::Auto | Self::P1080 => 1440,
        }
    }

    pub fn from_meta(value: &str) -> Self {
        match value {
            "720" => Self::P720,
            "1080" => Self::P1080,
            _ => Self::Auto,
        }
    }

    pub fn to_meta(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::P720 => "720",
            Self::P1080 => "1080",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_and_1080_share_a_1080_capture_ceiling() {
        assert_eq!(VideoQuality::Auto.capture_height(), 1080);
        assert_eq!(VideoQuality::P1080.capture_height(), 1080);
        assert_eq!(VideoQuality::P720.capture_height(), 720);
        assert_eq!(VideoQuality::Auto.capture_width(), 1920);
        assert_eq!(VideoQuality::P720.capture_width(), 1280);
    }

    #[test]
    fn meta_round_trips() {
        for quality in [VideoQuality::Auto, VideoQuality::P720, VideoQuality::P1080] {
            assert_eq!(VideoQuality::from_meta(quality.to_meta()), quality);
        }
        assert_eq!(VideoQuality::from_meta(""), VideoQuality::Auto);
        assert_eq!(VideoQuality::from_meta("garbage"), VideoQuality::Auto);
    }
}
