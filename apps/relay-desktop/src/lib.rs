//! Relay desktop client: a thin native (egui) window over the `relay-api`
//! client. No audio, PCM or inference lives here; the daemon owns all of it.
//!
//! Logic (state, validation, supervision, view models) is kept apart from
//! drawing (`ui`) so it can be tested without a window.

pub mod apply;
pub mod avatar;
pub mod controller;
pub mod convert;
pub mod emitter;
pub mod engine;
pub mod format;
pub mod hub;
pub mod mock;
pub mod model;
pub mod relay_link;
pub mod shell;
pub mod supervisor;
pub mod tray;
pub mod ui;
pub mod viewmodel;
