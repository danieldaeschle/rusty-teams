use std::io::Write;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use gpui_kit::*;

const ENVIRONMENT_VARIABLE: &str = "RUSTY_TEAMS_FRAME_LOG";
const REPORT_INTERVAL: Duration = Duration::from_secs(1);

struct FrameStatistics {
    path: String,
    window_start: Instant,
    last_draw: Option<Instant>,
    prepaint_at: Option<Instant>,
    draws: u32,
    longest_interval: Duration,
    total_span: Duration,
    longest_span: Duration,
    gpu_reported: bool,
}

fn statistics() -> Option<&'static Mutex<FrameStatistics>> {
    static STATISTICS: OnceLock<Option<Mutex<FrameStatistics>>> = OnceLock::new();
    STATISTICS
        .get_or_init(|| {
            let path = std::env::var(ENVIRONMENT_VARIABLE).ok()?;
            Some(Mutex::new(FrameStatistics {
                path,
                window_start: Instant::now(),
                last_draw: None,
                prepaint_at: None,
                draws: 0,
                longest_interval: Duration::ZERO,
                total_span: Duration::ZERO,
                longest_span: Duration::ZERO,
                gpu_reported: false,
            }))
        })
        .as_ref()
}

fn append(path: &str, line: &str) {
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{line}");
    }
}

pub fn probe(position: &'static str) -> impl IntoElement {
    canvas(
        move |_, _, _| {
            if let Some(statistics) = statistics()
                && position == "first"
            {
                statistics.lock().unwrap().prepaint_at = Some(Instant::now());
            }
        },
        move |_, _, window, _| {
            let Some(statistics) = statistics() else {
                return;
            };
            if position != "last" {
                return;
            }
            let mut statistics = statistics.lock().unwrap();
            let now = Instant::now();
            if !statistics.gpu_reported {
                statistics.gpu_reported = true;
                let line = format!("gpu {:?}", window.gpu_specs());
                append(&statistics.path, &line);
            }
            if let Some(last) = statistics.last_draw {
                statistics.longest_interval = statistics.longest_interval.max(now - last);
            }
            statistics.last_draw = Some(now);
            if let Some(prepaint_at) = statistics.prepaint_at.take() {
                let span = now - prepaint_at;
                statistics.total_span += span;
                statistics.longest_span = statistics.longest_span.max(span);
            }
            statistics.draws += 1;
            let elapsed = now - statistics.window_start;
            if elapsed >= REPORT_INTERVAL {
                let line = format!(
                    "draws {} in {:.2}s | longest gap {:.1} ms | prepaint-to-paint mean {:.2} ms max {:.2} ms",
                    statistics.draws,
                    elapsed.as_secs_f64(),
                    statistics.longest_interval.as_secs_f64() * 1000.,
                    statistics.total_span.as_secs_f64() * 1000. / statistics.draws as f64,
                    statistics.longest_span.as_secs_f64() * 1000.,
                );
                append(&statistics.path, &line);
                statistics.window_start = now;
                statistics.draws = 0;
                statistics.longest_interval = Duration::ZERO;
                statistics.total_span = Duration::ZERO;
                statistics.longest_span = Duration::ZERO;
            }
        },
    )
    .absolute()
    .size_0()
}
