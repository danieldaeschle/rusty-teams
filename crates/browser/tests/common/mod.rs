#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use browser::{Browser, BrowserError, Config, ConfigInputs, LaunchRequest, Launcher, Platform, Timeouts};
use futures_util::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
use tokio::task::{JoinHandle, JoinSet};

#[derive(Default)]
pub struct MockState {
    pub tabs: Vec<(String, String)>,
    pub requests: Vec<String>,
    pub attached: Vec<String>,
    pub opened: usize,
    pub hung: bool,
    pub ignore_close: bool,
}

fn numbered(urls: &[&str]) -> Vec<(String, String)> {
    urls.iter().enumerate().map(|(index, url)| (format!("P{index}"), (*url).to_owned())).collect()
}

pub struct MockChrome {
    pub port: u16,
    pub state: Arc<Mutex<MockState>>,
    kill: Arc<Notify>,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl MockChrome {
    pub async fn new() -> Arc<MockChrome> {
        let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        Arc::new(MockChrome {
            port,
            state: Arc::default(),
            kill: Arc::new(Notify::new()),
            task: Mutex::new(None),
        })
    }

    pub async fn start(&self, urls: &[&str]) {
        let listener = TcpListener::bind(("127.0.0.1", self.port)).await.unwrap();
        {
            let mut state = self.state.lock().unwrap();
            state.tabs = numbered(urls);
            state.hung = false;
            state.ignore_close = false;
        }
        let state = self.state.clone();
        let kill = self.kill.clone();
        let port = self.port;
        let handle = tokio::spawn(async move {
            let accept_loop = async {
                let mut connections = JoinSet::new();
                loop {
                    let (stream, _) = listener.accept().await.unwrap();
                    connections.spawn(handle_connection(stream, state.clone(), kill.clone(), port));
                }
            };
            tokio::select! { _ = accept_loop => {}, _ = kill.notified() => {} }
        });
        *self.task.lock().unwrap() = Some(handle);
    }

    pub async fn kill(&self) {
        self.kill.notify_one();
        let handle = self.task.lock().unwrap().take();
        if let Some(handle) = handle {
            let _ = handle.await;
        }
    }

    pub fn stop_answering(&self, ignore_close: bool) {
        let mut state = self.state.lock().unwrap();
        state.hung = true;
        state.ignore_close = ignore_close;
    }

    pub fn answer_again(&self) {
        self.state.lock().unwrap().hung = false;
    }

    pub fn kill_now(&self) {
        self.kill.notify_one();
    }

    pub fn urls(&self) -> Vec<String> {
        self.state.lock().unwrap().tabs.iter().map(|(_, url)| url.clone()).collect()
    }

    pub fn tab_ids(&self) -> Vec<String> {
        self.state.lock().unwrap().tabs.iter().map(|(id, _)| id.clone()).collect()
    }

    pub fn set_urls(&self, urls: &[&str]) {
        self.state.lock().unwrap().tabs = numbered(urls);
    }

    pub fn set_attached(&self, ids: &[&str]) {
        self.state.lock().unwrap().attached = ids.iter().map(|id| (*id).to_owned()).collect();
    }

    pub fn requests(&self) -> Vec<String> {
        self.state.lock().unwrap().requests.clone()
    }
}

async fn handle_connection(mut stream: TcpStream, state: Arc<Mutex<MockState>>, kill: Arc<Notify>, port: u16) {
    let mut peeked = [0u8; 2048];
    let request_line = loop {
        let count = stream.peek(&mut peeked).await.unwrap_or(0);
        if count == 0 {
            return;
        }
        let text = String::from_utf8_lossy(&peeked[..count]).into_owned();
        if let Some(line) = text.lines().next().filter(|_| text.contains("\r\n")) {
            break line.to_owned();
        }
    };
    if request_line.starts_with("GET /devtools/") {
        let page_id = request_line
            .split_whitespace()
            .nth(1)
            .and_then(|path| path.strip_prefix("/devtools/page/"))
            .map(str::to_owned);
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        let Some(Ok(message)) = socket.next().await else { return };
        let request: serde_json::Value = serde_json::from_str(message.to_text().unwrap_or("{}")).unwrap();
        let method = request["method"].as_str().unwrap_or("").to_owned();
        state.lock().unwrap().requests.push(format!("CDP {method}"));
        let result = match (method.as_str(), &page_id) {
            ("Browser.close", None) => {
                if !state.lock().unwrap().ignore_close {
                    kill.notify_one();
                }
                return;
            }
            ("Runtime.evaluate", Some(_)) => {
                if state.lock().unwrap().hung {
                    std::future::pending::<()>().await;
                }
                serde_json::json!({"result": {"type": "number", "value": 2}})
            }
            ("Target.getTargets", None) => {
                let state = state.lock().unwrap();
                let infos: Vec<_> = state
                    .tabs
                    .iter()
                    .map(|(id, _)| serde_json::json!({"targetId": id, "attached": state.attached.contains(id)}))
                    .collect();
                serde_json::json!({"targetInfos": infos})
            }
            ("Page.navigate", Some(id)) => {
                let url = request["params"]["url"].as_str().unwrap_or("").to_owned();
                if let Some(tab) = state.lock().unwrap().tabs.iter_mut().find(|(tab_id, _)| tab_id == id) {
                    tab.1 = url;
                }
                serde_json::json!({"frameId": "F"})
            }
            _ => return,
        };
        let reply = serde_json::json!({"id": request["id"], "result": result});
        let _ = futures_util::SinkExt::send(&mut socket, tokio_tungstenite::tungstenite::Message::text(reply.to_string())).await;
        let _ = socket.next().await;
        return;
    }
    let mut raw = Vec::new();
    let mut buffer = [0u8; 2048];
    while !raw.windows(4).any(|window| window == b"\r\n\r\n") {
        let count = stream.read(&mut buffer).await.unwrap_or(0);
        if count == 0 {
            return;
        }
        raw.extend_from_slice(&buffer[..count]);
    }
    let mut parts = request_line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or("").to_owned());
    state.lock().unwrap().requests.push(format!("{method} {}", path.split('?').next().unwrap_or("")));
    let (status, body) = match (method, path.as_str()) {
        ("GET", "/json/version") => (
            200,
            format!(
                r#"{{"Browser":"Chrome/154.0.0.1","webSocketDebuggerUrl":"ws://127.0.0.1:{port}/devtools/browser/mock"}}"#
            ),
        ),
        ("GET", "/json/list") => {
            let pages: Vec<String> = state
                .lock()
                .unwrap()
                .tabs
                .iter()
                .map(|(id, url)| format!(r#"{{"id":"{id}","type":"page","url":"{url}"}}"#))
                .collect();
            let extra = r#"{"id":"W","type":"service_worker","url":"https://teams.cloud.microsoft/sw.js"}"#;
            (200, format!("[{},{extra}]", pages.join(",")))
        }
        ("PUT", path) if path.starts_with("/json/new?") => {
            let url = path.trim_start_matches("/json/new?").to_owned();
            let mut state = state.lock().unwrap();
            state.opened += 1;
            let id = format!("N{}", state.opened);
            state.tabs.push((id.clone(), url.clone()));
            (200, format!(r#"{{"id":"{id}","type":"page","url":"{url}"}}"#))
        }
        ("GET", path) if path.starts_with("/json/close/") => {
            let id = path.trim_start_matches("/json/close/");
            state.lock().unwrap().tabs.retain(|(tab_id, _)| tab_id != id);
            (200, "Target is closing".to_owned())
        }
        _ => (404, "not found".to_owned()),
    };
    let response =
        format!("HTTP/1.1 {status} X\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n{body}", body.len());
    let _ = stream.write_all(response.as_bytes()).await;
    // like Chrome, keep the connection open until the client hangs up
    let _ = stream.read(&mut buffer).await;
}

pub struct FakeLauncher {
    pub mock: Arc<MockChrome>,
    pub requests: Mutex<Vec<LaunchRequest>>,
    pub fail: Mutex<bool>,
    pub foreign: Mutex<Option<Vec<&'static str>>>,
    pub kills: Mutex<usize>,
}

impl Launcher for FakeLauncher {
    fn spawn(&self, request: &LaunchRequest) -> Result<(), BrowserError> {
        self.requests.lock().unwrap().push(request.clone());
        if *self.fail.lock().unwrap() {
            return Err(BrowserError::ChromeNotFound("fake failure".into()));
        }
        let mock = self.mock.clone();
        let urls = match self.foreign.lock().unwrap().take() {
            Some(foreign) => foreign.into_iter().map(str::to_owned).collect(),
            None => request.start_urls.clone(),
        };
        tokio::spawn(async move {
            let refs: Vec<&str> = urls.iter().map(String::as_str).collect();
            mock.start(&refs).await;
        });
        Ok(())
    }

    fn kill_listener(&self, _port: u16) -> Result<(), BrowserError> {
        *self.kills.lock().unwrap() += 1;
        self.mock.kill_now();
        Ok(())
    }
}

pub struct Harness {
    pub browser: Browser,
    pub mock: Arc<MockChrome>,
    pub launcher: Arc<FakeLauncher>,
    pub mode_file: PathBuf,
    pub directory: tempfile::TempDir,
}

pub async fn harness() -> Harness {
    let directory = tempfile::tempdir().unwrap();
    let mock = MockChrome::new().await;
    let to_local = |path: &str| path.to_owned();
    let inputs = ConfigInputs {
        windows_variables: Default::default(),
        linux_data_dir: Some(directory.path().to_path_buf()),
        to_local: &to_local,
    };
    let config = Config::resolve(Platform::Linux, mock.port, &inputs).unwrap();
    let mode_file = config.mode_file.clone();
    let launcher = Arc::new(FakeLauncher { mock: mock.clone(), requests: Mutex::default(), fail: Mutex::new(false), foreign: Mutex::default(), kills: Mutex::default() });
    let timeouts = Timeouts {
        start: Duration::from_secs(5),
        stop: Duration::from_secs(1),
        probe: Duration::from_millis(200),
        poll: Duration::from_millis(20),
        profile_release: Duration::from_millis(10),
        jitter_min: Duration::from_millis(5),
        jitter_max: Duration::from_millis(20),
    };
    let browser = Browser::with_launcher(config, launcher.clone(), timeouts);
    Harness { browser, mock, launcher, mode_file, directory }
}
