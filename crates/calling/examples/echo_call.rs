use std::path::PathBuf;
use std::time::Duration;

use calling::audio_io::AUDIO_MODE_ENV;
use calling::{AudioMode, CallOptions, CallReport, call_channel, run_test_call};
use session::{DEFAULT_ENDPOINT, Session};

const HOLD_AFTER_CONNECTED: Duration = Duration::from_secs(12);
const HARD_LIMIT: Duration = Duration::from_secs(20);

fn print_report(report: &CallReport) {
    println!("# ice connected at {:?} ms, peer connected at {:?} ms", report.ice_connected_ms, report.peer_connected_ms);
    println!("# selected pair: {}", report.selected_pair.as_deref().unwrap_or("-"));
    println!("# dtls {} {} srtp {}", report.tls_version, report.dtls_cipher, report.srtp_cipher);
    println!(
        "# inbound audio: {} packets, {} bytes, {} s with new packets; outbound {} packets",
        report.inbound_packets, report.inbound_bytes, report.seconds_with_inbound_audio, report.outbound_packets
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
    let options = CallOptions {
        audio,
        hold_after_connected: Some(HOLD_AFTER_CONNECTED),
        hard_limit: Some(HARD_LIMIT),
        record_remote: true,
        wav_path: wav_path.clone(),
        sdp_dump_dir: std::env::var("CALLING_SDP_DUMP").ok().map(PathBuf::from),
        trace: true,
        ..CallOptions::default()
    };
    println!(
        "# Test call, audio {:?}, hang up after {:?} connected (hard limit {:?})",
        options.audio, HOLD_AFTER_CONNECTED, HARD_LIMIT
    );
    let (mut handle, control) = call_channel();
    let drain = tokio::spawn(async move { while handle.updates.recv().await.is_some() {} });
    let outcome = run_test_call(&session, &poll_session, options, control).await;
    let _ = drain.await;
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
