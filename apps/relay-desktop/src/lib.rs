//! Relay desktop shell: the eframe window, tray icon, native file dialog and
//! tokio runtime ownership around the shared `relay-ui` crate. No audio, PCM
//! or inference lives here; the daemon owns all of it.

pub mod app;
pub mod host;
pub mod login;
pub mod notify;
pub mod options;
pub mod prefs;
pub mod tray;
