//! Linux and macOS notification and ringtone playback.
//!
//! On Linux the saved call output is a Pulse sink description or name. Tones go to that
//! sink. When the Pulse daemon is not running, Linux uses the same cpal lookup as macOS.
//! A sink that disappears while a tone is playing falls back to the system default.

use std::collections::HashSet;
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use rodio::Source;
use rodio::cpal::traits::{DeviceTrait, HostTrait};

const SOUND_BYTES: &[u8] = include_bytes!("../../../../app/assets/sounds/notification.mp3");
const POLL: Duration = Duration::from_millis(20);
static SOUND_OUTPUT: Mutex<Option<String>> = Mutex::new(None);
static LOGGED_MISSES: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

pub fn set_sound_output(device_key: Option<String>) {
    *SOUND_OUTPUT.lock().unwrap_or_else(|p| p.into_inner()) = device_key;
}

/// One Pulse sink, as listed by the daemon.
#[derive(Clone)]
pub(crate) struct PulseSink {
    pub name: String,
    pub description: String,
}

/// Trim, drop one leading `default:` prefix, then ASCII-lowercase.
pub(crate) fn normalized_audio_key(value: &str) -> String {
    let trimmed = value.trim();
    let rest = strip_default_prefix(trimmed).trim();
    rest.to_ascii_lowercase()
}

fn strip_default_prefix(value: &str) -> &str {
    const PREFIX: &str = "default:";
    if value.len() >= PREFIX.len() && value[..PREFIX.len()].eq_ignore_ascii_case(PREFIX) {
        &value[PREFIX.len()..]
    } else {
        value
    }
}

/// Exact name or description match. A Game sink does not satisfy a Chat key.
pub(crate) fn matching_sink<'a>(sinks: &'a [PulseSink], key: &str) -> Option<&'a PulseSink> {
    let key = normalized_audio_key(key);
    if key.is_empty() {
        return None;
    }
    sinks.iter().find(|sink| {
        normalized_audio_key(&sink.name) == key || normalized_audio_key(&sink.description) == key
    })
}

/// True the first time this key is reported missing.
fn claim_miss(key: &str) -> bool {
    LOGGED_MISSES
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(key.to_owned())
}

fn note_missing_once(key: &str, message: String) {
    if claim_miss(key) {
        crate::diagnostics::record(message);
    }
}

/// True when a call-output key names this cpal device (id or friendly name).
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

struct Decoded {
    rate: u32,
    channels: u16,
    samples: Vec<f32>,
}

fn decode(bytes: &[u8]) -> Result<Decoded, String> {
    let decoder = rodio::Decoder::try_from(Cursor::new(bytes.to_vec()))
        .map_err(|error| format!("cannot decode notification sound: {error}"))?;
    let rate = decoder.sample_rate().get();
    let channels = decoder.channels().get();
    let samples = decoder.collect();
    Ok(Decoded {
        rate,
        channels,
        samples,
    })
}

fn play_on_rodio(
    sink: &mut rodio::MixerDeviceSink,
    bytes: Vec<u8>,
    looped: bool,
    stop: &AtomicBool,
) -> Result<(), String> {
    sink.log_on_drop(false);
    loop {
        if stop.load(Ordering::SeqCst) {
            return Ok(());
        }
        let player = rodio::play(sink.mixer(), Cursor::new(bytes.clone()))
            .map_err(|error| format!("cannot decode notification sound: {error}"))?;
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

fn open_default_and_play(bytes: Vec<u8>, looped: bool, stop: &AtomicBool) -> Result<(), String> {
    let mut sink = rodio::DeviceSinkBuilder::open_default_sink()
        .map_err(|error| format!("cannot open audio output: {error}"))?;
    play_on_rodio(&mut sink, bytes, looped, stop)
}

fn open_sink() -> Result<rodio::MixerDeviceSink, String> {
    let chosen = SOUND_OUTPUT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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
        .map_err(|error| format!("cannot open audio output: {error}"))
}

#[cfg(target_os = "linux")]
fn pulse_sinks() -> Option<Vec<PulseSink>> {
    use libpulse_binding::callbacks::ListResult;
    use libpulse_binding::context::{Context, FlagSet, State};
    use libpulse_binding::mainloop::standard::Mainloop;

    let mut mainloop = Mainloop::new()?;
    let mut context = Context::new(&mainloop, "rusty-teams")?;
    context.connect(None, FlagSet::NOAUTOSPAWN, None).ok()?;
    let deadline = std::time::Instant::now() + Duration::from_millis(400);
    loop {
        if std::time::Instant::now() > deadline {
            return None;
        }
        let _ = mainloop.iterate(false);
        match context.get_state() {
            State::Ready => break,
            State::Failed | State::Terminated => return None,
            _ => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    let found = Arc::new(Mutex::new(Vec::new()));
    let done = Arc::new(AtomicBool::new(false));
    let found_cb = Arc::clone(&found);
    let done_cb = Arc::clone(&done);
    let operation = context
        .introspect()
        .get_sink_info_list(move |item| match item {
            ListResult::Item(info) => found_cb
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(PulseSink {
                    name: info.name.as_deref().unwrap_or("").to_owned(),
                    description: info.description.as_deref().unwrap_or("").to_owned(),
                }),
            ListResult::End | ListResult::Error => done_cb.store(true, Ordering::SeqCst),
        });
    while !done.load(Ordering::SeqCst) {
        if std::time::Instant::now() > deadline {
            return None;
        }
        let _ = mainloop.iterate(false);
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(operation);
    Some(
        found
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone(),
    )
}

#[cfg(target_os = "linux")]
fn play_on_pulse(
    sink_name: &str,
    decoded: &Decoded,
    looped: bool,
    stop: &AtomicBool,
) -> Result<(), String> {
    use libpulse_binding::sample::{Format, Spec};
    use libpulse_binding::stream::Direction;
    use libpulse_simple_binding::Simple;

    let channels =
        u8::try_from(decoded.channels).map_err(|_| "too many audio channels".to_owned())?;
    let spec = Spec {
        format: Format::FLOAT32NE,
        channels,
        rate: decoded.rate,
    };
    if !spec.is_valid() {
        return Err(format!(
            "pulse cannot play {} Hz {} ch",
            decoded.rate, decoded.channels
        ));
    }
    let simple = Simple::new(
        None,
        "rusty-teams",
        Direction::Playback,
        Some(sink_name),
        "call tone",
        &spec,
        None,
        None,
    )
    .map_err(|error| format!("pulse open {sink_name}: {error}"))?;
    let frame = decoded.channels as usize;
    let chunk = (decoded.rate as usize / 10).max(1) * frame.max(1);
    loop {
        for piece in decoded.samples.chunks(chunk.max(1)) {
            if stop.load(Ordering::SeqCst) {
                return Ok(());
            }
            let mut bytes = Vec::with_capacity(piece.len() * 4);
            for sample in piece {
                bytes.extend_from_slice(&sample.to_ne_bytes());
            }
            simple
                .write(&bytes)
                .map_err(|error| format!("pulse write: {error}"))?;
        }
        if !looped {
            simple
                .drain()
                .map_err(|error| format!("pulse drain: {error}"))?;
            return Ok(());
        }
    }
}

#[cfg(target_os = "linux")]
fn play_linux(bytes: Vec<u8>, looped: bool, stop: &AtomicBool) -> Result<(), String> {
    let key = SOUND_OUTPUT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    if let Some(key) = key.filter(|key| !normalized_audio_key(key).is_empty()) {
        match pulse_sinks() {
            Some(sinks) => {
                if let Some(sink) = matching_sink(&sinks, &key) {
                    let decoded = decode(&bytes)?;
                    match play_on_pulse(&sink.name, &decoded, looped, stop) {
                        Ok(()) => return Ok(()),
                        Err(error)
                            if error.starts_with("pulse open ")
                                || error.starts_with("pulse cannot play")
                                || error.starts_with("pulse write")
                                || error.starts_with("pulse drain") =>
                        {
                            note_missing_once(
                                &key,
                                format!(
                                    "sound: selected output {key:?} failed ({error}), using system default"
                                ),
                            );
                        }
                        Err(error) => return Err(error),
                    }
                } else {
                    note_missing_once(
                        &key,
                        format!("sound: selected output {key:?} not found, using system default"),
                    );
                }
            }
            None => {
                note_missing_once(
                    &format!("pulse:{key}"),
                    format!("sound: pulse unavailable for {key:?}, trying the device list"),
                );
                let mut sink = open_sink()?;
                return play_on_rodio(&mut sink, bytes, looped, stop);
            }
        }
    }
    open_default_and_play(bytes, looped, stop)
}

fn play(bytes: Vec<u8>, looped: bool, stop: Arc<AtomicBool>) -> Result<(), String> {
    if stop.load(Ordering::SeqCst) {
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        return play_linux(bytes, looped, &stop);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let mut sink = open_sink()?;
        play_on_rodio(&mut sink, bytes, looped, &stop)
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

    fn sink(name: &str, description: &str) -> PulseSink {
        PulseSink {
            name: name.to_owned(),
            description: description.to_owned(),
        }
    }

    fn headset_sinks() -> Vec<PulseSink> {
        vec![
            sink("alsa_output.headset.mono-chat", "Headset Chat"),
            sink("alsa_output.headset.stereo-game", "Headset Game"),
        ]
    }

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
    fn a_pulse_description_selects_that_sink_and_not_the_other_output() {
        let sinks = headset_sinks();
        let game = matching_sink(&sinks, "Headset Game").unwrap();
        assert_eq!(game.name, "alsa_output.headset.stereo-game");
        let prefixed = matching_sink(&sinks, "default: Headset Game").unwrap();
        assert_eq!(prefixed.name, "alsa_output.headset.stereo-game");
        let chat = matching_sink(&sinks, "Headset Chat").unwrap();
        assert_eq!(chat.name, "alsa_output.headset.mono-chat");
        assert!(matching_sink(&sinks, "Headset Game").unwrap().name != chat.name);
        assert!(matching_sink(&sinks, "missing output").is_none());
        assert!(matching_sink(&sinks, "Game").is_none());
    }

    #[test]
    fn a_missing_key_is_noted_once() {
        let key = format!("missing-output-{}", std::process::id());
        assert!(claim_miss(&key));
        assert!(!claim_miss(&key));
        assert!(claim_miss(&format!("{key}-other")));
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

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "plays a short tone on the Pulse sink named by CALL_OUTPUT_KEY"]
    fn selected_pulse_sink_plays_the_tone() {
        let key = std::env::var("CALL_OUTPUT_KEY").expect("CALL_OUTPUT_KEY");
        let sinks = pulse_sinks().expect("pulse");
        let sink = matching_sink(&sinks, &key).unwrap_or_else(|| {
            panic!(
                "no sink for {key:?}; have {}",
                sinks
                    .iter()
                    .map(|sink| format!("{} ({})", sink.description, sink.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        });
        eprintln!("pulse sink {} ({})", sink.name, sink.description);
        set_sound_output(Some(key));
        play(
            SOUND_BYTES.to_vec(),
            false,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
    }
}
