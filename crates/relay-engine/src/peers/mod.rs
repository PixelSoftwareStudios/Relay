//! Peer links: discovery hints, secure control links, trust gating, link
//! election, reconnect and the mapping to domain events.

mod backoff;
mod control;
mod dialing;
mod election;
mod handle;
mod link;
mod manager;
mod names;
mod profile;
mod profiling;
mod sessions;
mod signaling;
mod start;
mod subnet;
mod trust;

pub use backoff::BackoffConfig;
pub use control::{InterfaceSource, PeerAdapter, system_interfaces};
pub use election::{LinkFacts, Verdict, elect};
pub use handle::{
	AdvertSink, AdvertSpec, ChannelEvent, DiscoverySetup, LinkInfo, PeerConfig, PeerError,
	PeerHandle, PeerOutput, PeerSnapshot, SessionEvent, SinkHandle, fingerprint, peer_id,
};
pub use link::{LinkError, LinkTiming};
pub use names::disambiguated;
pub use profile::{CHUNK_BYTES, LocalProfile};
pub use sessions::SessionTable;
pub use signaling::{
	Inbound, LinkNet, MediaSignal, PROFILE, decode, derived_session, encode, encode_media,
};
pub use start::spawn;
pub use subnet::Subnet;
pub use trust::{ConfigTrust, KeyedTrustStore, MemoryTrust, SharedTrust};
