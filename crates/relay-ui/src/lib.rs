//! Shared Relay UI: view models, state application, theme, responsive layout
//! and egui widgets, with no windowing, tray, native dialog or runtime
//! ownership. A shell (the desktop app, a future Android activity) supplies
//! a tokio `Handle` and implements [`shell::Shell`].

pub mod actions;
pub mod apply;
pub mod autostart;
pub mod avatar;
pub mod capabilities;
pub mod channels;
pub mod controller;
pub mod convert;
pub mod daemon_control;
pub mod emitter;
pub mod engine;
pub mod fonts;
pub mod format;
pub mod history_settings;
pub mod history_time;
pub mod history_view;
pub mod host_options;
pub mod hotkey_settings;
pub mod hub;
pub mod icons;
pub mod i18n;
pub mod imaging;
pub mod inbox;
pub mod layout;
pub mod limits_settings;
pub mod messaging;
pub mod microphone;
pub mod mock;
pub mod model;
pub mod qr;
pub mod qr_scan;
pub mod relay_link;
pub mod setting_support;
pub mod shell;
pub mod supervisor;
pub mod tasks;
pub mod theme;
pub mod timefmt;
pub mod ui;
pub mod viewmodel;

pub use shell::Insets;
