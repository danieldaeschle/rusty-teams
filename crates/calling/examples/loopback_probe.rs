use std::time::{Duration, Instant};

use calling::LoopbackKind;
use calling::pcm::SampleQueue;
use calling::sound_capture::start_loopback;

const CAPTURE_TIME: Duration = Duration::from_secs(3);
const FRAME_PERIOD: Duration = Duration::from_millis(10);

fn main() {
    let queue = SampleQueue::default();
    let started = match start_loopback(queue.clone()) {
        Ok(started) => started,
        Err(error) => {
            println!("loopback failed to start: {error}");
            return;
        }
    };
    let mut frames = 0u32;
    let mut loud_frames = 0u32;
    let begin = Instant::now();
    while begin.elapsed() < CAPTURE_TIME {
        std::thread::sleep(FRAME_PERIOD);
        let (frame, had_data) = queue.next_frame();
        if had_data {
            frames += 1;
            if frame.iter().any(|&sample| sample != 0) {
                loud_frames += 1;
            }
        }
    }
    println!("process exclude mode available: {}", started.kind == LoopbackKind::ProcessExclude);
    println!("mode: {:?}", started.kind);
    if let Some(warning) = &started.warning {
        println!("warning: {warning}");
    }
    println!("captured frames (10 ms): {frames}, with non-zero samples: {loud_frames}");
}
