use std::f32::consts::TAU;
use std::io::Write;
use std::path::Path;

pub const SAMPLE_RATE: u32 = 48_000;
pub const FRAME_SAMPLES: usize = 480;

pub struct SineGenerator {
    frequency: f32,
    amplitude: f32,
    phase: f32,
}

impl SineGenerator {
    pub fn new(frequency: f32, amplitude: f32) -> Self {
        SineGenerator {
            frequency,
            amplitude,
            phase: 0.0,
        }
    }

    pub fn next_frame(&mut self) -> Vec<i16> {
        let step = TAU * self.frequency / SAMPLE_RATE as f32;
        (0..FRAME_SAMPLES)
            .map(|_| {
                let sample = self.phase.sin() * self.amplitude * i16::MAX as f32;
                self.phase = (self.phase + step) % TAU;
                sample as i16
            })
            .collect()
    }
}

pub fn rms(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|&sample| (sample as f64).powi(2)).sum();
    ((sum / samples.len() as f64).sqrt() / i16::MAX as f64) as f32
}

/// Share of the signal power at `frequency` (Goertzel), 0..1; a clean sine scores near 1.
pub fn tone_ratio(samples: &[i16], frequency: f32) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let coefficient = 2.0 * (TAU * frequency / SAMPLE_RATE as f32).cos() as f64;
    let (mut previous, mut before_previous) = (0.0f64, 0.0f64);
    let mut energy = 0.0f64;
    for &sample in samples {
        let value = sample as f64;
        energy += value * value;
        let current = value + coefficient * previous - before_previous;
        before_previous = previous;
        previous = current;
    }
    if energy == 0.0 {
        return 0.0;
    }
    let power = previous * previous + before_previous * before_previous - coefficient * previous * before_previous;
    (2.0 * power / (samples.len() as f64 * energy)) as f32
}

pub fn write_wav(path: &Path, samples: &[i16]) -> std::io::Result<()> {
    let data_bytes = (samples.len() * 2) as u32;
    let mut file = std::io::BufWriter::new(std::fs::File::create(path)?);
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + data_bytes).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&SAMPLE_RATE.to_le_bytes())?;
    file.write_all(&(SAMPLE_RATE * 2).to_le_bytes())?;
    file.write_all(&2u16.to_le_bytes())?;
    file.write_all(&16u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_bytes.to_le_bytes())?;
    for sample in samples {
        file.write_all(&sample.to_le_bytes())?;
    }
    file.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_is_detected_and_silence_is_not() {
        let mut generator = SineGenerator::new(440.0, 0.3);
        let samples: Vec<i16> = (0..100).flat_map(|_| generator.next_frame()).collect();
        assert!(tone_ratio(&samples, 440.0) > 0.9);
        assert!(tone_ratio(&samples, 1000.0) < 0.05);
        assert!((rms(&samples) - 0.3 / 2f32.sqrt()).abs() < 0.01);
        assert_eq!(tone_ratio(&vec![0; 4800], 440.0), 0.0);
    }
}
