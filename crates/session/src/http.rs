use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use url::Url;

use crate::app::Target;
use crate::error::{Error, Result};

const HTTP_TIMEOUT: Duration = Duration::from_secs(5);

async fn get(endpoint: &str, path: &str) -> std::result::Result<Vec<u8>, String> {
    let parsed = Url::parse(endpoint).map_err(|error| error.to_string())?;
    let host = parsed.host_str().ok_or("endpoint has no host")?;
    let port = parsed.port_or_known_default().unwrap_or(80);
    let exchange = async {
        let mut stream = TcpStream::connect((host, port)).await.map_err(|error| error.to_string())?;
        let request = format!("GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).await.map_err(|error| error.to_string())?;
        read_response(&mut stream).await
    };
    tokio::time::timeout(HTTP_TIMEOUT, exchange)
        .await
        .map_err(|_| "timed out".to_owned())?
}

// Chrome keeps the socket open after the answer, so reading to EOF would hang.
async fn read_response(stream: &mut TcpStream) -> std::result::Result<Vec<u8>, String> {
    let mut response = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        if let Some(complete) = complete_length(&response)?
            && response.len() >= complete
        {
            return split_response(&response[..complete]);
        }
        let count = stream.read(&mut chunk).await.map_err(|error| error.to_string())?;
        if count == 0 {
            return split_response(&response);
        }
        response.extend_from_slice(&chunk[..count]);
    }
}

fn complete_length(response: &[u8]) -> std::result::Result<Option<usize>, String> {
    let Some(separator) = response.windows(4).position(|window| window == b"\r\n\r\n") else {
        return Ok(None);
    };
    let head = String::from_utf8_lossy(&response[..separator]);
    let length = head.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim().eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().ok())?
    });
    Ok(length.map(|length| separator + 4 + length))
}

fn split_response(response: &[u8]) -> std::result::Result<Vec<u8>, String> {
    let separator = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or("malformed HTTP response")?;
    let head = String::from_utf8_lossy(&response[..separator]);
    let status_line = head.lines().next().unwrap_or_default();
    if !status_line.contains(" 200 ") {
        return Err(format!("unexpected answer: {status_line}"));
    }
    Ok(response[separator + 4..].to_vec())
}

pub async fn list_targets(endpoint: &str) -> Result<Vec<Target>> {
    let no_browser = |reason: String| Error::NoBrowser {
        endpoint: endpoint.to_owned(),
        reason,
    };
    let body = get(endpoint, "/json/list").await.map_err(no_browser)?;
    serde_json::from_slice(&body).map_err(|error| no_browser(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_header_from_body() {
        let response = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n[]";
        assert_eq!(split_response(response).unwrap(), b"[]");
    }

    #[test]
    fn content_length_marks_completeness() {
        let partial = b"HTTP/1.1 200 OK\r\nContent-Length:5\r\n\r\nab";
        assert_eq!(complete_length(partial).unwrap(), Some(partial.len() + 3));
        assert_eq!(complete_length(b"HTTP/1.1 200 OK\r\n").unwrap(), None);
    }

    #[test]
    fn rejects_non_200() {
        assert!(split_response(b"HTTP/1.1 404 Not Found\r\n\r\n").is_err());
    }
}
