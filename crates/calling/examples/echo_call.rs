use std::path::PathBuf;
use std::time::Duration;

use calling::audio_io::AUDIO_MODE_ENV;
use calling::{AudioMode, CallCommand, CallOptions, CallReport, CallState, CallUpdate, call_channel, run_test_call};
use session::{DEFAULT_ENDPOINT, Session};

const HOLD_AFTER_CONNECTED: Duration = Duration::from_secs(12);
const HARD_LIMIT: Duration = Duration::from_secs(20);
const HOLD_START: Duration = Duration::from_secs(4);
const HOLD_LENGTH: Duration = Duration::from_secs(10);
const AFTER_RESUME: Duration = Duration::from_secs(20);

fn print_report(report: &CallReport) {
    println!("# ice connected at {:?} ms, peer connected at {:?} ms", report.ice_connected_ms, report.peer_connected_ms);
    println!("# selected pair: {}", report.selected_pair.as_deref().unwrap_or("-"));
    println!("# dtls {} {} srtp {}", report.tls_version, report.dtls_cipher, report.srtp_cipher);
    println!(
        "# inbound audio: {} packets, {} bytes, {} s with new packets; outbound {} packets",
        report.inbound_packets, report.inbound_bytes, report.seconds_with_inbound_audio, report.outbound_packets
    );
    let video = &report.video;
    println!(
        "# video: inbound {} packets, {} frames decoded ({}x{}), {} frames to the UI hub; outbound {} packets, {} frames encoded ({}x{})",
        video.inbound_packets, video.inbound_frames_decoded, video.inbound_width, video.inbound_height, video.frames_published,
        video.outbound_packets, video.outbound_frames_encoded, video.outbound_width, video.outbound_height
    );
    for second in &report.seconds {
        println!(
            "#   t+{:>2}s packets={:>5} level={:.3} rms={:.3} tone440={:.2}",
            second.second, second.packets, second.audio_level, second.decoded_rms, second.tone_ratio
        );
    }
    match &report.end {
        Some(end) => println!("# call end: code={} subCode={} phrase={}", end.code, end.sub_code, end.phrase),
        None => println!("# call end: no call/end callback"),
    }
    println!("# left cleanly: {}, decoded samples: {}", report.left_cleanly, report.recorded_samples);
}

#[tokio::main]
async fn main() {
    let wav_path = std::env::args().skip_while(|argument| argument != "--wav").nth(1).map(PathBuf::from);
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    let poll_session = Session::connect(&endpoint).await.expect("browser");
    let audio = AudioMode::parse(Some(&std::env::var(AUDIO_MODE_ENV).unwrap_or_else(|_| "tone".to_owned())));
    let hold = std::env::args().any(|argument| argument == "--hold");
    let rounds: usize = std::env::args().skip_while(|argument| argument != "--rounds").nth(1).and_then(|value| value.parse().ok()).unwrap_or(1);
    for round in 1..=rounds {
        println!("# round {round} of {rounds}");
        let round_wav = match (&wav_path, rounds) {
            (Some(path), 2..) => Some(path.with_extension(format!("{round}.wav"))),
            _ => wav_path.clone(),
        };
        test_call(&session, &poll_session, audio, round_wav, hold).await;
    }
}

async fn test_call(session: &Session, poll_session: &Session, audio: AudioMode, wav_path: Option<PathBuf>, hold: bool) {
    let seconds = std::env::args().skip_while(|argument| argument != "--seconds").nth(1).and_then(|value| value.parse().ok());
    let connected_for = match (hold, seconds) {
        (true, _) => HOLD_START + HOLD_LENGTH + AFTER_RESUME,
        (false, Some(seconds)) => Duration::from_secs(seconds),
        (false, None) => HOLD_AFTER_CONNECTED,
    };
    let options = CallOptions {
        audio,
        hold_after_connected: Some(connected_for),
        hard_limit: Some(connected_for + HARD_LIMIT - HOLD_AFTER_CONNECTED),
        record_remote: true,
        wav_path: wav_path.clone(),
        sdp_dump_dir: std::env::var("CALLING_SDP_DUMP").ok().map(PathBuf::from),
        trace: true,
        ..CallOptions::default()
    };
    println!(
        "# Test call, audio {:?}, hang up after {:?} connected (hard limit {:?})",
        options.audio, options.hold_after_connected, options.hard_limit
    );
    let (mut handle, control) = call_channel();
    let commands = handle.commands.clone();
    let drain = tokio::spawn(async move {
        let mut hold_scheduled = !hold;
        while let Some(update) = handle.updates.recv().await {
            match update {
                CallUpdate::State(CallState::Connected { .. }) if !hold_scheduled => {
                    hold_scheduled = true;
                    let commands = commands.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(HOLD_START).await;
                        println!("# hold");
                        let _ = commands.send(CallCommand::Hold(true));
                        tokio::time::sleep(HOLD_LENGTH).await;
                        println!("# resume");
                        let _ = commands.send(CallCommand::Hold(false));
                    });
                }
                CallUpdate::Hold(state) => println!("# hold state {state:?}"),
                CallUpdate::Notice(text) => println!("# notice {text}"),
                _ => {}
            }
        }
    });
    let outcome = run_test_call(session, poll_session, options, control).await;
    drain.abort();
    match outcome {
        Ok(report) => {
            print_report(&report);
            if let Some(path) = wav_path {
                println!("# wav written to {}", path.display());
            }
        }
        Err(error) => {
            eprintln!("# test call failed: {error}");
            std::process::exit(1);
        }
    }
}
