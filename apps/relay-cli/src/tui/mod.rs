//! Terminal UI (Ratatui + crossterm). Terminal only: no WebView dependencies.
//!
//! [`Model`] is pure state fed by [`crate::client::Update`]s; [`keys`] maps
//! keys to [`TuiCommand`]s; [`view`] draws a model; [`app`] glues them to a
//! terminal and a [`crate::client::Control`].

pub mod app;
pub mod exec;
pub mod keys;
pub mod model;
pub mod view;

pub use app::{drive, run};
pub use keys::TuiCommand;
pub use model::{HotkeyBackend, Model, Prompt, Tab};
