pub const SPEAKING_THRESHOLD: f32 = 0.02;
pub const SPEAKING_HOLD_TICKS: u32 = 2;

#[derive(Debug, Clone)]
pub struct SpeakingDetector {
    threshold: f32,
    hold_ticks: u32,
    remaining: u32,
}

impl Default for SpeakingDetector {
    fn default() -> Self {
        SpeakingDetector::new(SPEAKING_THRESHOLD, SPEAKING_HOLD_TICKS)
    }
}

impl SpeakingDetector {
    pub fn new(threshold: f32, hold_ticks: u32) -> Self {
        SpeakingDetector {
            threshold,
            hold_ticks,
            remaining: 0,
        }
    }

    pub fn update(&mut self, level: f32) -> bool {
        if level > self.threshold {
            self.remaining = self.hold_ticks;
            return true;
        }
        if self.remaining > 0 {
            self.remaining -= 1;
            return true;
        }
        false
    }

    pub fn reset(&mut self) {
        self.remaining = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_above_the_threshold_is_speaking() {
        let mut detector = SpeakingDetector::new(0.05, 0);
        assert!(!detector.update(0.05));
        assert!(detector.update(0.06));
        assert!(!detector.update(0.0));
    }

    #[test]
    fn hold_keeps_the_ring_through_short_pauses() {
        let mut detector = SpeakingDetector::new(0.05, 2);
        assert!(detector.update(0.5));
        assert!(detector.update(0.0));
        assert!(detector.update(0.0));
        assert!(!detector.update(0.0));
        assert!(detector.update(0.5));
    }

    #[test]
    fn reset_clears_the_hold() {
        let mut detector = SpeakingDetector::new(0.05, 3);
        detector.update(0.5);
        detector.reset();
        assert!(!detector.update(0.0));
    }
}
