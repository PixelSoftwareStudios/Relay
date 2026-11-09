//! The live channel driver: wires `relay-net`'s sans-IO channel protocol and
//! `relay-core`'s visibility rules into the running engine.
//!
//! Rules this module keeps (see `docs/channels.md`):
//!
//! - Channel proof and joins run only on a link whose peer already passed trust
//!   evaluation (`PeerOutput::Channel(LinkUp)` is emitted after it), never before.
//! - Channel messages never reach call state. The host routes `PeerOutput::Channel`
//!   here and nowhere else.
//! - Membership never grants microphone consent, accepts a call or links calls. The
//!   only call-side effect is [`ChannelHub::screen`]: an invite from a peer that proved
//!   no shared channel may be dropped when the user chose `SharedChannelOnly`.
//! - Only proven evidence counts as shared; a blinded advertisement tag is a hint
//!   that triggers a dial, nothing more.
//! - Secrets (passwords, invites, credentials) are never logged and never put in
//!   events, snapshots or operation results.

mod advert;
mod clock;
mod failure;
mod hub;
mod inbound;
mod joining;
mod ledger;
mod nearby;
mod ops;
mod proofs;
mod screen;
mod state;
mod view;

pub use clock::Clock;
pub use failure::Failure;
pub use hub::{ChannelHub, HubParts};
pub use joining::JoinRequest;
pub use ledger::{Replay, Stored, StoredInvite};
pub use nearby::LISTED_TTL;
pub use view::ChannelView;
