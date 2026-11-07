//! Relay desktop shell: the eframe window, tray icon, native file dialog and
//! tokio runtime ownership around the shared `relay-ui` crate. No audio, PCM
//! or inference lives here; the daemon owns all of it.

pub mod app;
pub mod host;
pub mod hotkey;
pub mod login;
pub mod microphone;
pub mod notify;
pub mod options;
pub mod prefs;
pub mod ptt;
pub mod ringtone_file;
pub mod tray;
