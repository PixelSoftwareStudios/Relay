//! Session messaging: transcripts, delivery state, file offers and the rules
//! around them. Logic and daemon plumbing only; the drawing is in
//! `ui::messages`. See `docs/session-messages.md` and `docs/ui.md`.

pub mod actions;
pub mod convert;
pub mod engine;
pub mod gates;
pub mod grouping;
pub mod hub;
pub mod image;
pub mod link;
pub mod mock;
mod mock_seed;
pub mod model;
pub mod rules;
pub mod state;
pub mod supervisor;
pub mod virtual_list;
