use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use calling::meeting::{LiveMeeting, fetch_live_meeting};
use calling::{
    CallEngine, CallHandle, CallSpec, CallState, CallUpdate, EndReason, EngineConfig, EngineEvent, call_channel,
};
use session::{Session, SessionConfig, Transport};

const START_RETRY_STEPS: [Duration; 3] = [Duration::from_secs(1), Duration::from_secs(2), Duration::from_secs(5)];
const START_RETRY_STEADY: Duration = Duration::from_secs(10);
const NOT_READY: &str = "Calls are not ready yet";

pub struct CallLauncher {
    engine: Mutex<Option<CallEngine>>,
}

impl CallLauncher {
    pub fn start(
        transport: Arc<dyn Transport>,
        ringable: bool,
        on_event: impl Fn(EngineEvent) + Send + 'static,
    ) -> Arc<CallLauncher> {
        let launcher = Arc::new(CallLauncher {
            engine: Mutex::new(None),
        });
        let running = launcher.clone();
        crate::runtime::handle().spawn(async move {
            let (engine, mut events) =
                start_with_retry(|| start_engine(transport.clone(), ringable), &START_RETRY_STEPS, START_RETRY_STEADY).await;
            eprintln!("calls: engine ready, registered with the registrar");
            *running.engine.lock().expect("engine lock") = Some(engine);
            while let Some(event) = events.recv().await {
                on_event(event);
            }
        });
        launcher
    }

    fn engine(&self) -> Option<CallEngine> {
        self.engine.lock().expect("engine lock").clone()
    }

    pub fn start_call(&self, spec: CallSpec) -> CallHandle {
        match self.engine() {
            Some(engine) => engine.start_call(spec),
            None => failed_handle(NOT_READY),
        }
    }

    pub async fn accept_ring(&self, ring_id: u64) -> Option<CallHandle> {
        self.engine()?.accept_ring(ring_id).await
    }

    pub fn decline_ring(&self, ring_id: u64) {
        if let Some(engine) = self.engine() {
            engine.decline_ring(ring_id);
        }
    }

    pub fn drop_ring(&self, ring_id: u64) {
        if let Some(engine) = self.engine() {
            engine.drop_ring(ring_id);
        }
    }

    pub async fn live_meeting(&self, thread_id: &str) -> Option<LiveMeeting> {
        let session = self.engine()?.session();
        fetch_live_meeting(&session, thread_id).await.ok().flatten()
    }
}

async fn start_with_retry<T, E, F, S>(mut start: S, steps: &[Duration], steady: Duration) -> T
where
    T: Send + 'static,
    E: std::fmt::Display + Send + 'static,
    F: Future<Output = Result<T, E>> + Send + 'static,
    S: FnMut() -> F,
{
    let mut last_failure = String::new();
    let mut failures = 0usize;
    loop {
        let failure = match tokio::spawn(start()).await {
            Ok(Ok(started)) => return started,
            Ok(Err(error)) => error.to_string(),
            Err(join_error) => format!("start task panicked: {join_error}"),
        };
        let delay = steps.get(failures).copied().unwrap_or(steady);
        failures += 1;
        if failure != last_failure {
            eprintln!("calls: engine start failed, retrying in {}s: {failure}", delay.as_secs());
            last_failure = failure;
        }
        tokio::time::sleep(delay).await;
    }
}

async fn start_engine(
    transport: Arc<dyn Transport>,
    ringable: bool,
) -> calling::Result<(CallEngine, tokio::sync::mpsc::UnboundedReceiver<EngineEvent>)> {
    let session = Session::with_transport(transport.clone(), SessionConfig::default()).await?;
    let poll_session = Session::with_transport(transport, SessionConfig::default()).await?;
    let config = EngineConfig {
        ringable,
        ..EngineConfig::default()
    };
    CallEngine::start(session, poll_session, config).await
}

fn failed_handle(message: &str) -> CallHandle {
    let (handle, control) = call_channel();
    control.send(CallUpdate::State(CallState::Ended {
        reason: EndReason::Failed(message.to_owned()),
    }));
    handle
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    #[test]
    fn engine_start_survives_errors_and_panics() {
        let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let attempts = Arc::new(AtomicU32::new(0));
        let counter = attempts.clone();
        let started = runtime.block_on(start_with_retry(
            move || {
                let attempt = counter.fetch_add(1, Ordering::SeqCst);
                async move {
                    match attempt {
                        0 => panic!("boom"),
                        1 => Err("registrar_http_500".to_owned()),
                        _ => Ok(attempt),
                    }
                }
            },
            &[Duration::ZERO],
            Duration::ZERO,
        ));
        assert_eq!(started, 2);
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }
}
