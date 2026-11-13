//! Linux notification and ringtone playback through the system audio output.
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

fn open_sink() -> Result<rodio::MixerDeviceSink, String> {
    let chosen = SOUND_OUTPUT
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    let device = chosen.and_then(|key| {
        rodio::cpal::default_host()
            .output_devices()
            .ok()?
            .find(|device| {
                device.id().is_ok_and(|id| id.1 == key)
                    || device
                        .description()
                        .is_ok_and(|description| description.name() == key)
            })
    });
    if let Some(device) = device {
        if let Ok(sink) =
            rodio::DeviceSinkBuilder::from_device(device).and_then(|builder| builder.open_stream())
        {
            return Ok(sink);
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
