use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use tokio::task::JoinHandle as TaskHandle;

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
                if let Some(thread) = thread.take() {
                    let _ = thread.join();
                }
            }
            Capture::Task(task) => task.abort(),
        }
    }
}
