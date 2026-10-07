use std::future::Future;
use std::sync::OnceLock;

use tokio::runtime::{Builder, Runtime};
use tokio::sync::oneshot;

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name("teams-net")
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}

pub fn handle() -> tokio::runtime::Handle {
    runtime().handle().clone()
}

/// Runs the future on the network runtime. The receiver is runtime-agnostic, so the GPUI executor can await it.
pub fn spawn<T: Send + 'static>(
    future: impl Future<Output = T> + Send + 'static,
) -> oneshot::Receiver<T> {
    let (sender, receiver) = oneshot::channel();
    runtime().spawn(async move {
        let _ = sender.send(future.await);
    });
    receiver
}
