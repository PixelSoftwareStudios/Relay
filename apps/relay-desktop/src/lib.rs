//! Relay desktop client: a thin native (egui) window over the `relay-api`
//! client. No audio, PCM or inference lives here; the daemon owns all of it.
//!
//! Logic (state, validation, supervision, view models) is kept apart from
//! drawing (`ui`) so it can be tested without a window.

pub use relay_ui::{
	apply, avatar, channels, controller, convert, emitter, engine, format, hub, imaging, inbox,
	mock, model, qr, relay_link, supervisor, viewmodel,
};
pub mod shell;
pub mod tasks;
pub mod tray;
pub mod ui;
