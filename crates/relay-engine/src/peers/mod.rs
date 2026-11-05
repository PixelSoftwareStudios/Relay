//! Peer links: discovery hints, secure control links, trust gating, link
//! election, reconnect and the mapping to domain events.

mod backoff;
mod election;
mod handle;
mod link;
mod manager;
mod profile;
mod profiling;
mod signaling;
mod start;
mod trust;

pub use backoff::BackoffConfig;
pub use election::{LinkFacts, Verdict, elect};
pub use handle::{
	DiscoverySetup, LinkInfo, PeerConfig, PeerError, PeerHandle, PeerOutput, PeerSnapshot,
	fingerprint, peer_id,
};
pub use link::{LinkError, LinkTiming};
pub use profile::{CHUNK_BYTES, LocalProfile};
pub use signaling::{
	Inbound, MediaSignal, PROFILE, SessionTable, decode, derived_session, encode, encode_media,
};
pub use start::spawn;
pub use trust::{ConfigTrust, KeyedTrustStore, MemoryTrust, SharedTrust};
