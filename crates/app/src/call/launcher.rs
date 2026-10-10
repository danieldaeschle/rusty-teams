use std::sync::Arc;

use calling::{
    CallHandle, CallState, CallUpdate, EndReason, TestCallOptions, call_channel, run_test_call,
};
use session::{Session, SessionConfig, Transport};
use tokio::sync::Mutex;

pub struct CallLauncher {
    transport: Arc<dyn Transport>,
    one_call_at_a_time: Arc<Mutex<()>>,
}

impl CallLauncher {
    pub fn new(transport: Arc<dyn Transport>) -> Self {
        CallLauncher {
            transport,
            one_call_at_a_time: Arc::new(Mutex::new(())),
        }
    }

    pub fn start_test_call(&self) -> CallHandle {
        let (handle, control) = call_channel();
        let transport = self.transport.clone();
        let gate = self.one_call_at_a_time.clone();
        crate::runtime::handle().spawn(async move {
            let _previous_call_finished = gate.lock().await;
            let sessions = async {
                let session = Session::with_transport(transport.clone(), SessionConfig::default()).await?;
                let poll_session = Session::with_transport(transport, SessionConfig::default()).await?;
                Ok::<_, session::Error>((session, poll_session))
            };
            match sessions.await {
                Ok((session, poll_session)) => {
                    let _ = run_test_call(&session, &poll_session, TestCallOptions::default(), control).await;
                }
                Err(error) => control.send(CallUpdate::State(CallState::Ended {
                    reason: EndReason::Failed(crate::notice::short_error(&error)),
                })),
            }
        });
        handle
    }
}
