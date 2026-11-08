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
mod fixed_hex;
pub mod handshake;
pub mod identity;
pub mod limits;
pub mod media;
mod noise;
pub mod onlink;
pub mod record;
pub mod session;
pub mod transfer;

pub mod proto {
	//! Generated `relay.peer.v1` messages carried inside authenticated control records.
	include!(concat!(env!("OUT_DIR"), "/relay.peer.v1.rs"));
}

pub use address::{
	AddressError, AddressPolicy, DIAL_PORT_MIN, endpoint_to_proto, is_lan_ip,
	is_shared_address_space,
};
pub use noise::PROTOCOL_MAJOR;
pub use onlink::InboundScope;
