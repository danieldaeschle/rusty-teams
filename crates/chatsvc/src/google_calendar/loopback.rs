use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::error::{Error, Result};

use super::oauth::AuthPending;

/// Bind a free loopback port and return `http://127.0.0.1:<port>/`.
pub async fn allocate_redirect_uri() -> Result<(String, TcpListener)> {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .map_err(|error| Error::UnexpectedAnswer(error.to_string()))?;
    let port = listener
        .local_addr()
        .map_err(|error| Error::UnexpectedAnswer(error.to_string()))?
        .port();
    Ok((format!("http://127.0.0.1:{port}/"), listener))
}

/// Wait for the browser redirect, validate `state`, return the authorization code.
pub async fn wait_for_code(pending: &AuthPending, listener: TcpListener) -> Result<String> {
    let accept = tokio::time::timeout(Duration::from_secs(180), listener.accept());
    let (mut stream, _) = accept
        .await
        .map_err(|_| Error::UnexpectedAnswer("Google sign-in timed out".into()))?
        .map_err(|error| Error::UnexpectedAnswer(error.to_string()))?;
    let mut buffer = vec![0_u8; 4096];
    let read = stream
        .read(&mut buffer)
        .await
        .map_err(|error| Error::UnexpectedAnswer(error.to_string()))?;
    let request = String::from_utf8_lossy(&buffer[..read]);
    let line = request.lines().next().unwrap_or_default();
    let path = line
        .strip_prefix("GET ")
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or("/");
    let query = path.split('?').nth(1).unwrap_or("");
    let mut code = None;
    let mut state = None;
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        match (parts.next(), parts.next()) {
            (Some("code"), Some(value)) => code = Some(urlencoding_decode(value)),
            (Some("state"), Some(value)) => state = Some(urlencoding_decode(value)),
            _ => {}
        }
    }
    let body = if state.as_deref() == Some(pending.state.as_str()) && code.is_some() {
        "You can close this window and return to Rusty Teams."
    } else {
        "Sign-in failed. You can close this window."
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
    if state.as_deref() != Some(pending.state.as_str()) {
        return Err(Error::UnexpectedAnswer("Google OAuth state mismatch".into()));
    }
    code.ok_or_else(|| Error::UnexpectedAnswer("Google OAuth code missing".into()))
}

fn urlencoding_decode(value: &str) -> String {
    percent_encoding::percent_decode_str(value)
        .decode_utf8_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::google_calendar::oauth::GoogleOAuth;

    #[tokio::test]
    async fn allocate_redirect_is_loopback() {
        let (uri, listener) = allocate_redirect_uri().await.unwrap();
        assert!(uri.starts_with("http://127.0.0.1:"));
        assert!(uri.ends_with('/'));
        drop(listener);
    }

    #[tokio::test]
    async fn wait_for_code_reads_query() {
        let oauth = GoogleOAuth {
            client_id: "cid".into(),
            client_secret: String::new(),
        };
        let (uri, listener) = allocate_redirect_uri().await.unwrap();
        let pending = oauth.begin(&uri);
        let port = listener.local_addr().unwrap().port();
        let state = pending.state.clone();
        let code_value = "the-code";
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            let request = format!(
                "GET /?code={code_value}&state={state} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(request.as_bytes()).await.unwrap();
        });
        let code = wait_for_code(&pending, listener).await.unwrap();
        assert_eq!(code, code_value);
    }
}
