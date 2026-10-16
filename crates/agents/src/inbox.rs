use std::fmt;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::Serialize;

use crate::profile::Environment;
use crate::sessions::RunningSession;

const SENDER: &str = "rusty-teams";
const SOCKET_TIMEOUT: Duration = Duration::from_secs(5);
const WSL_PROGRAM: &str = "wsl.exe";
const SOCKET_RELAY_SCRIPT: &str = "import socket,sys;s=socket.socket(socket.AF_UNIX);s.settimeout(5);s.connect(sys.argv[1]);s.sendall(sys.stdin.buffer.read());s.close()";
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, PartialEq, Eq)]
pub enum DeliveryError {
    SessionGone,
    Failed(String),
}

impl fmt::Display for DeliveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeliveryError::SessionGone => formatter.write_str("the session is no longer running"),
            DeliveryError::Failed(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for DeliveryError {}

struct PeerToken(String);

impl fmt::Debug for PeerToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PeerToken(..)")
    }
}

#[derive(Serialize)]
struct AuthFrame<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    token: &'a str,
}

#[derive(Serialize)]
struct UserFrame<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    message: UserMessage<'a>,
    from: &'a str,
    msg_id: &'a str,
}

#[derive(Serialize)]
struct UserMessage<'a> {
    content: &'a str,
}

pub fn inbox_frame(token: &str, content: &str, message_id: &str) -> String {
    let auth = AuthFrame {
        kind: "auth",
        token,
    };
    let user = UserFrame {
        kind: "user",
        message: UserMessage { content },
        from: SENDER,
        msg_id: message_id,
    };
    format!(
        "{}\n{}\n",
        serde_json::to_string(&auth).unwrap_or_default(),
        serde_json::to_string(&user).unwrap_or_default()
    )
}

fn read_peer_token(sessions_dir: &Path, process_id: u32) -> Option<PeerToken> {
    let prefix = format!("{process_id}.");
    std::fs::read_dir(sessions_dir)
        .ok()?
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with(&prefix) && name.ends_with(".key")
        })
        .find_map(|entry| {
            let value: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(entry.path()).ok()?).ok()?;
            value["peerToken"]
                .as_str()
                .map(|token| PeerToken(token.to_owned()))
        })
}

pub fn deliver(
    environment: &Environment,
    sessions_dir: &Path,
    session: &RunningSession,
    content: &str,
) -> Result<(), DeliveryError> {
    let socket_path = session
        .socket_path
        .as_deref()
        .ok_or_else(|| DeliveryError::Failed("the session does not accept messages".to_owned()))?;
    let token = read_peer_token(sessions_dir, session.process_id).ok_or_else(|| {
        DeliveryError::Failed("the session's message key is not readable".to_owned())
    })?;
    let frame = inbox_frame(&token.0, content, &uuid::Uuid::new_v4().to_string());
    match environment {
        Environment::Wsl { distro } => send_through_wsl(distro.as_deref(), socket_path, &frame),
        Environment::Host => send_on_host(socket_path, &frame),
    }
}

#[cfg(unix)]
fn send_on_host(socket_path: &str, frame: &str) -> Result<(), DeliveryError> {
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(socket_path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused => {
            DeliveryError::SessionGone
        }
        _ => DeliveryError::Failed(format!("cannot connect to the session: {error}")),
    })?;
    let failed = |error: std::io::Error| DeliveryError::Failed(format!("send failed: {error}"));
    stream
        .set_write_timeout(Some(SOCKET_TIMEOUT))
        .map_err(failed)?;
    stream.write_all(frame.as_bytes()).map_err(failed)
}

#[cfg(not(unix))]
fn send_on_host(_socket_path: &str, _frame: &str) -> Result<(), DeliveryError> {
    Err(DeliveryError::Failed(
        "delivery to Windows-native sessions is not supported".to_owned(),
    ))
}

fn send_through_wsl(
    distro: Option<&str>,
    socket_path: &str,
    frame: &str,
) -> Result<(), DeliveryError> {
    let mut command = Command::new(WSL_PROGRAM);
    if let Some(distro) = distro {
        command.args(["-d", distro]);
    }
    command
        .args(["-e", "python3", "-c", SOCKET_RELAY_SCRIPT, socket_path])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command
        .spawn()
        .map_err(|error| DeliveryError::Failed(format!("cannot start {WSL_PROGRAM}: {error}")))?;
    let written = child
        .stdin
        .take()
        .map(|mut stdin| stdin.write_all(frame.as_bytes()));
    let output = child
        .wait_with_output()
        .map_err(|error| DeliveryError::Failed(format!("send failed: {error}")))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("ConnectionRefusedError") || stderr.contains("FileNotFoundError") {
        return Err(DeliveryError::SessionGone);
    }
    let detail = stderr.lines().last().unwrap_or_default().trim();
    let write_note = match written {
        Some(Err(error)) => format!(" ({error})"),
        _ => String::new(),
    };
    Err(DeliveryError::Failed(format!(
        "delivery failed: {detail}{write_note}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_frame_is_an_auth_line_then_a_user_line() {
        assert_eq!(
            inbox_frame("tok", "Hi \"you\"\nbye", "id-1"),
            "{\"type\":\"auth\",\"token\":\"tok\"}\n{\"type\":\"user\",\"message\":{\"content\":\"Hi \\\"you\\\"\\nbye\"},\"from\":\"rusty-teams\",\"msg_id\":\"id-1\"}\n"
        );
    }

    #[test]
    fn errors_and_the_token_wrapper_never_show_the_token() {
        let token = PeerToken("s3cret".to_owned());
        assert!(!format!("{token:?}").contains("s3cret"));
        for error in [
            DeliveryError::SessionGone,
            DeliveryError::Failed("delivery failed: boom".to_owned()),
        ] {
            assert!(!format!("{error:?} {error}").contains("s3cret"));
        }
    }

    #[test]
    fn a_session_without_a_socket_cannot_be_delivered_to() {
        let directory = tempfile::tempdir().unwrap();
        let session = RunningSession {
            session_id: "abc".to_owned(),
            name: None,
            working_dir: String::new(),
            socket_path: None,
            process_id: 1,
        };
        assert!(matches!(
            deliver(&Environment::Host, directory.path(), &session, "hi"),
            Err(DeliveryError::Failed(_))
        ));
    }

    #[cfg(unix)]
    mod unix {
        use std::io::Read;
        use std::os::unix::net::UnixListener;

        use super::*;

        fn session(socket_path: &Path, process_id: u32) -> RunningSession {
            RunningSession {
                session_id: "abc".to_owned(),
                name: None,
                working_dir: String::new(),
                socket_path: Some(socket_path.to_string_lossy().into_owned()),
                process_id,
            }
        }

        #[test]
        fn a_host_delivery_writes_both_lines_to_the_socket() {
            let directory = tempfile::tempdir().unwrap();
            let socket_path = directory.path().join("s.sock");
            let listener = UnixListener::bind(&socket_path).unwrap();
            std::fs::write(
                directory.path().join("42.ab12.key"),
                r#"{"peerToken":"tok"}"#,
            )
            .unwrap();
            let reader = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut received = String::new();
                stream.read_to_string(&mut received).unwrap();
                received
            });
            deliver(
                &Environment::Host,
                directory.path(),
                &session(&socket_path, 42),
                "hello",
            )
            .unwrap();
            let received = reader.join().unwrap();
            let lines: Vec<&str> = received.lines().collect();
            assert_eq!(lines.len(), 2);
            assert_eq!(lines[0], r#"{"type":"auth","token":"tok"}"#);
            assert!(lines[1].starts_with(
                r#"{"type":"user","message":{"content":"hello"},"from":"rusty-teams","msg_id":""#
            ));
        }

        #[test]
        fn a_missing_socket_means_the_session_is_gone() {
            let directory = tempfile::tempdir().unwrap();
            std::fs::write(
                directory.path().join("42.ab12.key"),
                r#"{"peerToken":"tok"}"#,
            )
            .unwrap();
            let result = deliver(
                &Environment::Host,
                directory.path(),
                &session(&directory.path().join("none.sock"), 42),
                "hello",
            );
            assert_eq!(result, Err(DeliveryError::SessionGone));
        }
    }
}
