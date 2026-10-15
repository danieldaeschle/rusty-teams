use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::audio::SAMPLE_RATE;

pub const AUDIO_DUMP_ENV: &str = "CALLING_AUDIO_DUMP";
const HEADER_BYTES: u32 = 44;

/// Mono 16-bit WAV writer for checking each mic processing stage; the header gets its sizes on drop.
pub struct WavDump {
    writer: BufWriter<File>,
    data_bytes: u32,
}

impl WavDump {
    pub fn create(path: &Path) -> std::io::Result<WavDump> {
        let mut writer = BufWriter::new(File::create(path)?);
        writer.write_all(&header(0))?;
        Ok(WavDump { writer, data_bytes: 0 })
    }

    pub fn write(&mut self, samples: &[i16]) {
        for sample in samples {
            if self.writer.write_all(&sample.to_le_bytes()).is_err() {
                return;
            }
            self.data_bytes += 2;
        }
    }

    fn finish(&mut self) -> std::io::Result<()> {
        self.writer.seek(SeekFrom::Start(0))?;
        self.writer.write_all(&header(self.data_bytes))?;
        self.writer.flush()
    }
}

impl Drop for WavDump {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

fn header(data_bytes: u32) -> Vec<u8> {
    let byte_rate = SAMPLE_RATE * 2;
    let mut header = Vec::with_capacity(HEADER_BYTES as usize);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(HEADER_BYTES - 8 + data_bytes).to_le_bytes());
    header.extend_from_slice(b"WAVEfmt ");
    header.extend_from_slice(&16u32.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    header.extend_from_slice(&byte_rate.to_le_bytes());
    header.extend_from_slice(&2u16.to_le_bytes());
    header.extend_from_slice(&16u16.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&data_bytes.to_le_bytes());
    header
}

/// One WAV per stage of the mic chain, written only when `CALLING_AUDIO_DUMP` names a directory.
pub struct StageDumps {
    stages: Vec<WavDump>,
}

impl StageDumps {
    pub const STAGES: [&str; 4] = ["1-raw", "2-echo-highpass", "3-noise", "4-gain"];

    pub fn from_env() -> Option<StageDumps> {
        let directory = PathBuf::from(std::env::var_os(AUDIO_DUMP_ENV)?);
        StageDumps::create(&directory).ok()
    }

    pub fn create(directory: &Path) -> std::io::Result<StageDumps> {
        std::fs::create_dir_all(directory)?;
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let stages = StageDumps::STAGES
            .iter()
            .map(|stage| WavDump::create(&directory.join(format!("mic-{stamp}-{stage}.wav"))))
            .collect::<std::io::Result<Vec<_>>>()?;
        Ok(StageDumps { stages })
    }

    pub fn write(&mut self, stage_index: usize, samples: &[i16]) {
        if let Some(stage) = self.stages.get_mut(stage_index) {
            stage.write(samples);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dump_is_a_readable_wav_with_the_written_samples() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("dump.wav");
        {
            let mut dump = WavDump::create(&path).unwrap();
            dump.write(&[1, -2, 3]);
        }
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), SAMPLE_RATE);
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 6);
        assert_eq!(&bytes[44..], &[1, 0, 254, 255, 3, 0]);
    }

    #[test]
    fn every_stage_gets_its_own_file() {
        let directory = tempfile::tempdir().unwrap();
        let mut dumps = StageDumps::create(directory.path()).unwrap();
        dumps.write(3, &[5]);
        drop(dumps);
        let names: Vec<String> =
            std::fs::read_dir(directory.path()).unwrap().map(|entry| entry.unwrap().file_name().into_string().unwrap()).collect();
        assert_eq!(names.len(), StageDumps::STAGES.len());
        assert!(names.iter().any(|name| name.ends_with("4-gain.wav")));
    }
}
