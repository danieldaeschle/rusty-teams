use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct TimelineEntry {
    pub elapsed_ms: u128,
    pub label: String,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct Timeline {
    started: Instant,
    entries: Arc<Mutex<Vec<TimelineEntry>>>,
    echo: bool,
}

impl Timeline {
    pub fn new(echo: bool) -> Self {
        Timeline {
            started: Instant::now(),
            entries: Arc::default(),
            echo,
        }
    }

    pub fn elapsed_ms(&self) -> u128 {
        self.started.elapsed().as_millis()
    }

    pub fn record(&self, label: impl Into<String>, detail: impl Into<String>) {
        let entry = TimelineEntry {
            elapsed_ms: self.elapsed_ms(),
            label: label.into(),
            detail: detail.into(),
        };
        if self.echo {
            println!("{:>6} ms  {:<34} {}", entry.elapsed_ms, entry.label, entry.detail);
        }
        self.entries.lock().expect("timeline lock").push(entry);
    }

    pub fn entries(&self) -> Vec<TimelineEntry> {
        self.entries.lock().expect("timeline lock").clone()
    }
}
