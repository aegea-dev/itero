//! The one way itero makes an HTTP client.

use std::time::Duration;

/// A blocking client that gives up after `timeout`. HTTPS goes through
/// rustls with ring (see Cargo.toml), installed here as the process's
/// provider the first time; certificates are the system's.
pub fn client(timeout: Duration) -> reqwest::Result<reqwest::blocking::Client> {
    ring();
    reqwest::blocking::Client::builder()
        .timeout(timeout)
        .build()
}

/// A blocking client for a server on a Unix socket (itero-ai's), giving
/// up after `timeout`; URLs name any host, which is not looked up.
#[cfg(unix)]
pub fn unix_client(
    socket: &std::path::Path,
    timeout: Duration,
) -> reqwest::Result<reqwest::blocking::Client> {
    ring();
    reqwest::blocking::Client::builder()
        .unix_socket(socket)
        .timeout(timeout)
        .build()
}

/// Installs ring as the process's TLS provider, the first time.
fn ring() {
    static RING: std::sync::Once = std::sync::Once::new();
    RING.call_once(|| {
        // Err only when a provider is already installed, which is all this
        // needs.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}
