use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::error::BrowserError;

pub(crate) struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

pub(crate) async fn request(
    port: u16,
    method: &str,
    path: &str,
    timeout: Duration,
) -> Result<Response, BrowserError> {
    tokio::time::timeout(timeout, exchange(port, method, path))
        .await
        .map_err(|_| BrowserError::Http(format!("{method} timed out after {}s", timeout.as_secs())))?
}

async fn exchange(port: u16, method: &str, path: &str) -> Result<Response, BrowserError> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .await
        .map_err(|error| BrowserError::NoBrowser(format!("127.0.0.1:{port}: {error}")))?;
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(head.as_bytes())
        .await
        .map_err(|error| BrowserError::Http(error.to_string()))?;
    let mut raw = Vec::new();
    let mut buffer = [0u8; 8192];
    while !is_complete(&raw) {
        let count = stream
            .read(&mut buffer)
            .await
            .map_err(|error| BrowserError::Http(error.to_string()))?;
        if count == 0 {
            break;
        }
        raw.extend_from_slice(&buffer[..count]);
    }
    parse_response(&raw)
}

fn is_complete(raw: &[u8]) -> bool {
    let Some(split) = find(raw, b"\r\n\r\n") else { return false };
    let head = String::from_utf8_lossy(&raw[..split]).to_ascii_lowercase();
    let body = &raw[split + 4..];
    if head.contains("transfer-encoding: chunked") {
        return body.ends_with(b"0\r\n\r\n");
    }
    let length = head
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .and_then(|value| value.trim().parse::<usize>().ok());
    length.is_some_and(|length| body.len() >= length)
}

pub(crate) fn parse_response(raw: &[u8]) -> Result<Response, BrowserError> {
    let split = find(raw, b"\r\n\r\n")
        .ok_or_else(|| BrowserError::Http("response without header end".into()))?;
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let body = &raw[split + 4..];
    let mut lines = head.lines();
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| BrowserError::Http("bad status line".into()))?;
    let chunked = lines.any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.starts_with("transfer-encoding:") && lower.contains("chunked")
    });
    let body = if chunked { dechunk(body)? } else { body.to_vec() };
    Ok(Response { status, body })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

fn dechunk(mut data: &[u8]) -> Result<Vec<u8>, BrowserError> {
    let bad = || BrowserError::Http("bad chunked body".into());
    let mut out = Vec::new();
    loop {
        let line_end = find(data, b"\r\n").ok_or_else(bad)?;
        let size_text = String::from_utf8_lossy(&data[..line_end]);
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| bad())?;
        data = &data[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if data.len() < size + 2 {
            return Err(bad());
        }
        out.extend_from_slice(&data[..size]);
        data = &data[size + 2..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_content_length_response() {
        let response = parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}").unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"{}");
    }

    #[test]
    fn parses_chunked_response() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n";
        assert_eq!(parse_response(raw).unwrap().body, b"abcde");
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_response(b"nonsense").is_err());
    }
}
