//! The DevTools port of this installation.
//!
//! A fixed, well-known port (9222) lets any local program guess where the signed-in browser
//! listens. Each installation picks a free port once and keeps it, so a browser left running
//! by an earlier start is found again. This only raises the bar: listening sockets stay
//! visible to local users (`ss -ltn`).

use std::net::{Ipv4Addr, TcpListener};
use std::path::Path;

use crate::config::DEFAULT_PORT;

/// Ports below this need privileges or belong to well-known services; a stored value under it is ignored.
const LOWEST_PORT: u16 = 1024;

/// The stored port, or a newly picked free one that is stored for the next start.
/// Falls back to [`DEFAULT_PORT`] when nothing can be stored, so restarts keep finding the browser.
pub(crate) fn installation_port(port_file: &Path) -> u16 {
    if let Some(port) = stored_port(port_file) {
        return port;
    }
    let Some(port) = free_port() else {
        return DEFAULT_PORT;
    };
    let stored = port_file
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(port_file, port.to_string()));
    if stored.is_ok() { port } else { DEFAULT_PORT }
}

fn stored_port(port_file: &Path) -> Option<u16> {
    std::fs::read_to_string(port_file)
        .ok()?
        .trim()
        .parse::<u16>()
        .ok()
        .filter(|port| *port >= LOWEST_PORT)
}

fn free_port() -> Option<u16> {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|listener| listener.local_addr())
        .map(|address| address.port())
        .ok()
        .filter(|port| *port >= LOWEST_PORT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_free_port_is_picked_once_and_kept() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("profile.port");
        let first = installation_port(&file);
        assert_ne!(first, DEFAULT_PORT);
        assert!(first >= LOWEST_PORT);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), first.to_string());
        assert_eq!(installation_port(&file), first);
    }

    #[test]
    fn a_damaged_or_privileged_stored_port_is_replaced() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("profile.port");
        for stored in ["not a port", "80", "70000"] {
            std::fs::write(&file, stored).unwrap();
            let port = installation_port(&file);
            assert!(port >= LOWEST_PORT, "{stored}");
            assert_eq!(std::fs::read_to_string(&file).unwrap(), port.to_string());
        }
    }

    #[test]
    fn without_a_writable_place_the_default_port_stays() {
        let directory = tempfile::tempdir().unwrap();
        let blocker = directory.path().join("file");
        std::fs::write(&blocker, "").unwrap();
        assert_eq!(
            installation_port(&blocker.join("profile.port")),
            DEFAULT_PORT
        );
    }
}
