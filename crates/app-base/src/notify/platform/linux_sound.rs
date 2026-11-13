//! Linux and macOS notification and ringtone playback through the system audio output.
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rodio::cpal::traits::{DeviceTrait, HostTrait};

const SOUND_BYTES: &[u8] = include_bytes!("../../../../app/assets/sounds/notification.mp3");
const POLL: Duration = Duration::from_millis(20);
static SOUND_OUTPUT: Mutex<Option<String>> = Mutex::new(None);

pub fn set_sound_output(device_key: Option<String>) {
    *SOUND_OUTPUT.lock().unwrap_or_else(|p| p.into_inner()) = device_key;
}

/// True when a call-output key names this cpal device (id, friendly name, or Pulse-style id).
pub(crate) fn device_matches_key(device_id: &str, device_name: &str, key: &str) -> bool {
    let key = key.trim();
    if key.is_empty() {
        return false;
    }
    if device_id == key || device_name == key {
        return true;
    }
    let key_lower = key.to_ascii_lowercase();
    let id_lower = device_id.to_ascii_lowercase();
    let name_lower = device_name.to_ascii_lowercase();
    id_lower == key_lower
        || name_lower == key_lower
        || (!key_lower.contains(' ') && id_lower.contains(&key_lower))
        || (key_lower.len() >= 4 && name_lower.contains(&key_lower))
}

fn open_sink() -> Result<rodio::MixerDeviceSink, String> {
    let chosen = SOUND_OUTPUT
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    if let Some(key) = chosen.as_ref() {
        if let Some(device) = rodio::cpal::default_host()
            .output_devices()
            .ok()
            .into_iter()
            .flatten()
            .find(|device| {
                let id = device.id().ok().map(|id| id.1).unwrap_or_default();
                let name = device
                    .description()
                    .ok()
                    .map(|description| description.name().to_owned())
                    .unwrap_or_default();
                device_matches_key(&id, &name, key)
            })
        {
            match rodio::DeviceSinkBuilder::from_device(device)
                .and_then(|builder| builder.open_stream())
            {
                Ok(sink) => return Ok(sink),
                Err(error) => crate::diagnostics::record(format!(
                    "sound: selected output {key:?} failed ({error}), using system default"
                )),
            }
        } else {
            crate::diagnostics::record(format!(
                "sound: selected output {key:?} not found, using system default"
            ));
        }
    }
    rodio::DeviceSinkBuilder::open_default_sink()
        .map_err(|e| format!("cannot open audio output: {e}"))
}

fn play(bytes: Vec<u8>, looped: bool, stop: Arc<AtomicBool>) -> Result<(), String> {
    if stop.load(Ordering::SeqCst) {
        return Ok(());
    }
    let mut sink = open_sink()?;
    sink.log_on_drop(false);
    loop {
        if stop.load(Ordering::SeqCst) {
            return Ok(());
        }
        let player = rodio::play(sink.mixer(), Cursor::new(bytes.clone()))
            .map_err(|e| format!("cannot decode notification sound: {e}"))?;
        while !player.empty() {
            if stop.load(Ordering::SeqCst) {
                player.stop();
                return Ok(());
            }
            std::thread::sleep(POLL);
        }
        if !looped {
            return Ok(());
        }
    }
}

pub fn play_mp3_until(bytes: Vec<u8>, looped: bool, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        if let Err(error) = play(bytes, looped, stop) {
            crate::diagnostics::record(format!("sound: {error}"));
        }
    });
}

pub fn play_mp3(bytes: Vec<u8>) {
    play_mp3_until(bytes, false, Arc::new(AtomicBool::new(false)));
}

pub fn play_sound() {
    play_mp3(SOUND_BYTES.to_vec());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_notification_decodes_to_audible_samples() {
        let decoded = rodio::Decoder::try_from(Cursor::new(SOUND_BYTES)).unwrap();
        assert!(decoded.take(48000).any(|sample| sample.abs() > 0.001));
    }

    #[test]
    fn cancelled_playback_does_not_open_a_device_or_decode_audio() {
        assert!(play(Vec::new(), true, Arc::new(AtomicBool::new(true))).is_ok());
    }

    #[test]
    fn call_output_keys_match_cpal_ids_and_friendly_names() {
        assert!(device_matches_key(
            "sink-a",
            "Front Speakers",
            "Front Speakers"
        ));
        assert!(device_matches_key("sink-a", "Front Speakers", "sink-a"));
        assert!(device_matches_key(
            "sink-a",
            "Front Speakers",
            "front speakers"
        ));
        assert!(device_matches_key(
            "alsa_output.pci-0000_00_1f.3.analog-stereo",
            "Built-in Audio Analog Stereo",
            "alsa_output.pci-0000_00_1f.3.analog-stereo"
        ));
        assert!(!device_matches_key(
            "sink-a",
            "Front Speakers",
            "Headphones"
        ));
        assert!(!device_matches_key("id", "Speaker", ""));
    }

    #[test]
    #[ignore = "plays a short notification on the real default audio output"]
    fn notification_plays_on_default_output() {
        play(
            SOUND_BYTES.to_vec(),
            false,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
    }
}
