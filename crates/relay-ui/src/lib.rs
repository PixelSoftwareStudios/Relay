//! Shared Relay UI: view models, state application, theme, responsive layout
//! and egui widgets, with no windowing, tray, native dialog or runtime
//! ownership. A shell (the desktop app, a future Android activity) supplies
//! a tokio `Handle` and implements [`shell::Shell`].

pub mod layout;
pub mod shell;
pub mod theme;
