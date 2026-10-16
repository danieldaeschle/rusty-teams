//! Runs a 48 kHz mono 16-bit WAV through the mic chain and writes every stage next to it.
//! Usage: cargo run -p calling --release --example mic_chain_wav -- <input.wav> <output-dir> [auto|low|off]

use calling::audio::{ClipGenerator, FRAME_SAMPLES};
use calling::mic_chain::MicChain;
use calling::wav_dump::StageDumps;
use calling::{MicSettings, NoiseSuppression};

fn main() {
    let mut arguments = std::env::args().skip(1);
    let (Some(input), Some(output)) = (arguments.next(), arguments.next()) else {
        eprintln!("usage: mic_chain_wav <input.wav> <output-dir> [auto|low|off]");
        std::process::exit(2);
    };
    let level = arguments.next().and_then(|key| NoiseSuppression::from_key(&key)).unwrap_or_default();
    let bytes = std::fs::read(&input).expect("read input");
    let mut clip = ClipGenerator::from_wav(&bytes).expect("48 kHz mono 16-bit WAV");
    let frames = bytes.len().saturating_sub(44) / 2 / FRAME_SAMPLES;
    let settings = MicSettings { noise_suppression: level, music_mode: false };
    let dumps = StageDumps::create(std::path::Path::new(&output)).expect("create output dir");
    let mut chain = MicChain::new(settings.apm_config(), settings.ml_noise_suppression(), Some(dumps));
    let waiting = std::time::Instant::now();
    while settings.ml_noise_suppression() && !calling::noise_stage::is_ready() && waiting.elapsed().as_secs() < 30 {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let started = std::time::Instant::now();
    for _ in 0..frames {
        let mut frame = clip.next_frame();
        chain.process(&mut frame);
    }
    eprintln!("{frames} frames in {:?}", started.elapsed());
}
