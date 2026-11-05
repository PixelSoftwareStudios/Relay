//! Session messaging in the daemon (`docs/session-messages.md`).
//!
//! One actor (`Hub`) owns every transcript ([`relay_messages::HistoryManager`], one
//! [`relay_messages::SessionStore`] per live call, retained read-only after the call), the
//! transfer records and the event feed. Work reaches it as closures on its inbox:
//!
//! - the control actor reports the calls it knows; a session starts when a call goes live
//!   and ends with it;
//! - the peer layer delivers `SessionMessage`s from the *authenticated* link of a peer, and
//!   a message can only reach the session that peer is in (no call id travels on the wire,
//!   so routing is by peer, never by a number the peer chose);
//! - the API layer calls the operations (capability `messages`);
//! - transfer tasks report progress and results.
//!
//! File bytes never pass through the actor: each accepted offer runs one task owning its
//! `relay-messages` state machine, its file or sink and its `relay-net` data link.
//! Message text, file names and paths never reach a log above TRACE (nothing logs them).

mod config;
mod feed;
mod handle;
mod hub;
mod inbound;
mod ops;
mod ops_offers;
mod ops_state;
mod problem;
mod queries;
mod session;
mod sink;
mod source;
mod transfers;
mod view;
mod wire;
mod xfer;

pub use config::{ConfigStore, MessagingConfig, NoStore, SharedConfig};
pub use feed::FeedItem;
pub use handle::{MessagingHandle, Stopped};
pub use hub::{CallFact, Hub, HubParts, Phase, spawn};
pub use ops::{Caller, EXPORT_INLINE_MAX, MAX_INLINE_IMAGE_BYTES};
pub use problem::Refusal;
pub use queries::Subscribed;
pub use sink::{MessagingLimits, MessagingOptions, SinkFactory, directory_sinks};
pub use source::{Prepared, SourceError, prepare, preview_text};
