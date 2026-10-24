//! The one way itero makes an HTTP client.

use std::time::Duration;

/// A blocking client that gives up after `timeout`. HTTPS goes through
/// rustls with ring (see Cargo.toml), installed here as the process's
/// provider the first time; certificates are the system's.
pub fn client(timeout: Duration) -> reqwest::Result<reqwest::blocking::Client> {
    static RING: std::sync::Once = std::sync::Once::new();
    RING.call_once(|| {
        // Err only when a provider is already installed, which is all this
        // needs.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
    reqwest::blocking::Client::builder()
        .timeout(timeout)
        .build()
}
