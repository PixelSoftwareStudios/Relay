//! LAN discovery, device identity and the secure peer transport for Relay.

mod address;
pub mod handshake;
pub mod identity;
pub mod limits;
mod noise;
pub mod record;
pub mod session;

pub mod proto {
	//! Generated `relay.peer.v1` messages carried inside authenticated control records.
	include!(concat!(env!("OUT_DIR"), "/relay.peer.v1.rs"));
}

pub use address::{AddressError, AddressPolicy, endpoint_to_proto, is_lan_ip};
pub use noise::PROTOCOL_MAJOR;
