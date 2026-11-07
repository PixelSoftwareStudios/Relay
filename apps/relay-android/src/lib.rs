//! Relay for Android: the Rust half. Kotlin owns the Activity, the foreground
//! service and every Android framework call; this library owns the engine, the
//! shared egui UI and the audio streams. See docs/android.md.

// Some host pieces are only reached from the Android activity.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

uniffi::setup_scaffolding!();

mod audio_session;
mod back;
mod bridge;
mod ffi;
mod host;
mod logging;
mod sealed;
mod shell;

#[cfg(target_os = "android")]
mod activity;

pub use bridge::*;
pub use ffi::*;
