use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tokio::task::JoinHandle as TaskHandle;

const STOP_WAIT: Duration = Duration::from_secs(2);
const STOP_POLL: Duration = Duration::from_millis(5);

/// A running frame source; dropping it stops the capture.
pub enum Capture {
    Thread { stop: Arc<AtomicBool>, thread: Option<JoinHandle<()>> },
    Task(TaskHandle<()>),
}

impl Capture {
    pub fn spawn_thread(name: &str, body: impl FnOnce(Arc<AtomicBool>) + Send + 'static) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread = std::thread::Builder::new().name(name.to_owned()).spawn(move || body(thread_stop))?;
        Ok(Capture::Thread { stop, thread: Some(thread) })
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        match self {
            Capture::Thread { stop, thread } => {
                stop.store(true, Ordering::SeqCst);
                let Some(thread) = thread.take() else { return };
                let started = Instant::now();
                while !thread.is_finished() && started.elapsed() < STOP_WAIT {
                    std::thread::sleep(STOP_POLL);
                }
                if thread.is_finished() {
                    let _ = thread.join();
                }
            }
            Capture::Task(task) => task.abort(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::Capture;

    #[test]
    fn a_stuck_capture_thread_does_not_hold_up_the_caller() {
        let capture = Capture::spawn_thread("stuck", |_| loop {
            std::thread::sleep(Duration::from_secs(60));
        })
        .unwrap();
        let started = Instant::now();
        drop(capture);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_stopped_capture_thread_is_joined() {
        let capture = Capture::spawn_thread("quick", |stop| {
            while !stop.load(std::sync::atomic::Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
            }
        })
        .unwrap();
        let started = Instant::now();
        drop(capture);
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
