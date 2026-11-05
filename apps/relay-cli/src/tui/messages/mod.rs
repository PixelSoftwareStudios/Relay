//! The Messages tab of the TUI: a per-call transcript with compose box, search, pins and
//! file offers, fed by its own `messages` connection (`client::run_session_feed`).

pub mod exec;
pub mod keys;
mod prompt;
mod render;
mod state;
pub mod view;

pub use exec::MsgOutcome;
pub use keys::{Handled, MsgCommand};
pub use prompt::{Prompt, PromptKind};
pub use state::{Link, MAX_ROWS, MessagesState, MsgUpdate, Row, SessionPane};
