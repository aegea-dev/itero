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

/// A blocking client for a download of gigabytes: it gives up when it
/// cannot connect within `connect`, never on the whole's time.
pub fn download_client(connect: Duration) -> reqwest::Result<reqwest::blocking::Client> {
    ring();
    reqwest::blocking::Client::builder()
        .connect_timeout(connect)
        .timeout(None)
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
