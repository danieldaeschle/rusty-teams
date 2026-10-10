#[cfg(not(windows))]
fn main() {
    eprintln!("engine_probe runs on Windows only; it uses the app's WebView2 host");
    std::process::exit(2);
}

#[cfg(windows)]
#[tokio::main]
async fn main() {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use calling::{CallEngine, EngineConfig, EngineEvent};
    use session::{Session, SessionConfig};

    const START_LIMIT: Duration = Duration::from_secs(90);

    let folder = std::env::args().nth(1).map(PathBuf::from).expect("usage: engine_probe <user-data-folder> [hold-seconds]");
    let hold = Duration::from_secs(std::env::args().nth(2).and_then(|value| value.parse().ok()).unwrap_or(15));
    let started = Instant::now();
    let transport = webview::start(webview::HostConfig {
        user_data_folder: folder,
        window_title: "Rusty Teams engine probe - Sign in".into(),
    });
    let sessions = async {
        let session = Session::with_transport(transport.clone(), SessionConfig::default()).await?;
        let poll_session = Session::with_transport(transport, SessionConfig::default()).await?;
        Ok::<_, session::Error>((session, poll_session))
    };
    let (session, poll_session) = match sessions.await {
        Ok(sessions) => sessions,
        Err(error) => return println!("# session failed after {:?}: {error}", started.elapsed()),
    };
    println!("# webview session ready after {:?}", started.elapsed());
    let page_ready = async {
        let (control, _events) = session.subscribe(session::App::Teams).await.ok()?;
        loop {
            let host = control.evaluate("location.host").await.ok()?.as_str().unwrap_or_default().to_owned();
            if host.starts_with("teams.") {
                return Some(true);
            }
            if host.starts_with("login.") {
                return Some(false);
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    };
    match tokio::time::timeout(Duration::from_secs(60), page_ready).await {
        Ok(Some(true)) => println!("# teams page on its origin after {:?}", started.elapsed()),
        Ok(Some(false)) => return println!("# teams page is on the sign-in page, profile not signed in; stopping"),
        _ => return println!("# teams page did not reach its origin"),
    }
    let engine_started = Instant::now();
    let outcome = tokio::time::timeout(START_LIMIT, CallEngine::start(session, poll_session, EngineConfig::default())).await;
    let (engine, mut events) = match outcome {
        Ok(Ok(started_engine)) => started_engine,
        Ok(Err(error)) => return println!("# engine start failed after {:?}: {error}", engine_started.elapsed()),
        Err(_) => return println!("# engine start timed out after {START_LIMIT:?}"),
    };
    println!("# engine ready (registrar registration ok) after {:?}", engine_started.elapsed());
    let mut incoming = 0;
    let _ = tokio::time::timeout(hold, async {
        while let Some(event) = events.recv().await {
            incoming += 1;
            match event {
                EngineEvent::Incoming(ring) => println!("# evt: incoming ring id {} group {}", ring.ring_id, ring.is_group),
                EngineEvent::RingEnded { ring_id, kind, .. } => println!("# evt: ring {ring_id} ended {kind:?}"),
            }
        }
    })
    .await;
    println!("# held {hold:?}, engine events: {incoming}");
    let stop_started = Instant::now();
    engine.stop().await;
    println!("# engine stopped after {:?}, total {:?}", stop_started.elapsed(), started.elapsed());
}
