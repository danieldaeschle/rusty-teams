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
            note_missing_output(key);
        }
    }
    rodio::DeviceSinkBuilder::open_default_sink()
        .map_err(|e| format!("cannot open audio output: {e}"))
}

fn current_output_key() -> Option<String> {
    SOUND_OUTPUT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

fn note_missing_output(key: &str) {
    static LAST: Mutex<String> = Mutex::new(String::new());
    let mut last = LAST.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if *last == key {
        return;
    }
    *last = key.to_owned();
    crate::diagnostics::record(format!(
        "sound: selected output {key:?} not found, using system default"
    ));
}

/// Saved call-output keys are Pulse descriptions. Match the description or the
/// sink name, ignoring case and one leading `default:` prefix.
pub(crate) fn pulse_names_match(description: &str, name: &str, key: &str) -> bool {
    let key = normalize_output_key(key);
    if key.is_empty() {
        return false;
    }
    normalize_output_key(description) == key || normalize_output_key(name) == key
}

fn normalize_output_key(value: &str) -> String {
    let trimmed = value.trim();
    let stripped = trimmed
        .split_once(':')
        .filter(|(prefix, _)| prefix.eq_ignore_ascii_case("default"))
        .map(|(_, rest)| rest.trim())
        .unwrap_or(trimmed);
    stripped.to_ascii_lowercase()
}

fn play(bytes: Vec<u8>, looped: bool, stop: Arc<AtomicBool>) -> Result<(), String> {
    if stop.load(Ordering::SeqCst) {
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    if let Some(key) = current_output_key()
        && play_on_pulse(&key, &bytes, looped, &stop)
    {
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

/// Plays on the matching Pulse sink. `false` means no sink matched or Pulse
/// could not be opened, and the caller should use the system default.
#[cfg(target_os = "linux")]
fn play_on_pulse(key: &str, bytes: &[u8], looped: bool, stop: &AtomicBool) -> bool {
    use rodio::Source;

    let Some(sink_name) = pulse_sink_name(key) else {
        return false;
    };
    let Ok(decoder) = rodio::Decoder::try_from(Cursor::new(bytes.to_vec())) else {
        return false;
    };
    let rate = decoder.sample_rate().get();
    let channels = decoder.channels().get();
    let samples: Vec<f32> = decoder.collect();
    let spec = libpulse_binding::sample::Spec {
        format: libpulse_binding::sample::Format::FLOAT32NE,
        rate,
        channels: channels as u8,
    };
    if !spec.is_valid() {
        return false;
    }
    let Ok(simple) = libpulse_simple_binding::Simple::new(
        None,
        "rusty-teams",
        libpulse_binding::stream::Direction::Playback,
        Some(&sink_name),
        "Call tone",
        &spec,
        None,
        None,
    ) else {
        return false;
    };
    let frame = usize::from(channels);
    let chunk = (rate as usize / 20).max(1) * frame;
    loop {
        let mut offset = 0;
        while offset < samples.len() {
            if stop.load(Ordering::SeqCst) {
                return true;
            }
            let end = (offset + chunk).min(samples.len());
            let mut pcm = Vec::with_capacity((end - offset) * 4);
            for sample in &samples[offset..end] {
                pcm.extend_from_slice(&sample.to_ne_bytes());
            }
            if simple.write(&pcm).is_err() {
                return true;
            }
            offset = end;
        }
        if simple.drain().is_err() {
            return true;
        }
        if !looped || stop.load(Ordering::SeqCst) {
            return true;
        }
    }
}

#[cfg(target_os = "linux")]
fn pulse_sink_name(key: &str) -> Option<String> {
    use libpulse_binding::callbacks::ListResult;
    use libpulse_binding::context::{Context, FlagSet, State};
    use libpulse_binding::mainloop::standard::{IterateResult, Mainloop};
    use libpulse_binding::operation::State as OpState;

    let mut mainloop = Mainloop::new()?;
    let mut context = Context::new(&mainloop, "rusty-teams")?;
    context
        .connect(None, FlagSet::NOAUTOSPAWN, None)
        .ok()?;
    loop {
        if matches!(
            mainloop.iterate(true),
            IterateResult::Quit(_) | IterateResult::Err(_)
        ) {
            return None;
        }
        match context.get_state() {
            State::Ready => break,
            State::Failed | State::Terminated => return None,
            _ => {}
        }
    }
    let found = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&found);
    let key = key.to_owned();
    let operation = context
        .introspect()
        .get_sink_info_list(move |item| {
            let ListResult::Item(info) = item else {
                return;
            };
            let description = info.description.as_deref().unwrap_or("");
            let name = info.name.as_deref().unwrap_or("");
            if pulse_names_match(description, name, &key) {
                *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(name.to_owned());
            }
        });
    while operation.get_state() == OpState::Running {
        if matches!(
            mainloop.iterate(true),
            IterateResult::Quit(_) | IterateResult::Err(_)
        ) {
            return None;
        }
    }
    found
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
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
    fn a_pulse_description_matches_the_saved_key_and_keeps_game_and_chat_apart() {
        assert!(pulse_names_match(
            "Headset Game",
            "alsa_output.usb-game",
            "Headset Game"
        ));
        assert!(pulse_names_match(
            "Headset Game",
            "alsa_output.usb-game",
            "default: Headset Game"
        ));
        assert!(pulse_names_match(
            "Ignored",
            "alsa_output.usb-game",
            "alsa_output.usb-game"
        ));
        assert!(!pulse_names_match(
            "Headset Chat",
            "alsa_output.usb-chat",
            "Headset Game"
        ));
        assert!(!pulse_names_match(
            "Headset Game",
            "alsa_output.usb-game",
            "default:"
        ));
        assert!(!pulse_names_match("Speaker", "speaker", ""));
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
