//! LAN discovery, device identity and the secure peer transport for Relay.
//!
//! The crate moves opaque bytes: control records carry `relay.peer.v1` envelopes and media
//! datagrams carry opaque payloads. Neither discovery nor a completed handshake grants trust,
//! call acceptance or microphone authority.

mod address;
pub mod channel;
pub mod connect;
pub mod control;
pub mod discovery;
pub mod handshake;
pub mod identity;
pub mod limits;
pub mod media;
mod noise;
pub mod record;
pub mod session;

pub mod proto {
	//! Generated `relay.peer.v1` messages carried inside authenticated control records.
	include!(concat!(env!("OUT_DIR"), "/relay.peer.v1.rs"));
}

pub use address::{
	AddressError, AddressPolicy, endpoint_to_proto, is_lan_ip, is_shared_address_space,
};
pub use noise::PROTOCOL_MAJOR;
