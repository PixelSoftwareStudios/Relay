//! relay-net
pub mod proto {
	include!(concat!(env!("OUT_DIR"), "/relay.peer.v1.rs"));
}
