//! `itero webhook`: takes requests from outside, each checked against a
//! secret, and saves them as `itero.webhook.received`. A server of its own,
//! apart from the daemon's API: it has only the paths it is given.
pub mod check;
