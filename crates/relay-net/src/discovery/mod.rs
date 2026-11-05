//! DNS-SD over mDNS discovery. Everything learned here is an untrusted hint: it never pairs a
//! device, authorizes media, or starts capture.

mod advert;
mod interfaces;
mod service;
mod table;

use thiserror::Error;

pub use advert::{
	Advertisement, DiscoveredPeer, FingerprintHint, parse_resolved, sanitize_display_name,
};
pub use interfaces::InterfacePolicy;
pub use service::{Discovery, DiscoveryEvent};
pub use table::DiscoveryTable;

/// mDNS service type: a project convention, not an allocated public registry name.
pub const SERVICE_TYPE: &str = "_praxis-relay._tcp.local.";

#[derive(Debug, Error)]
pub enum DiscoveryError {
	#[error("mdns daemon error: {0}")]
	Daemon(String),
	#[error("advertisement refused: {0}")]
	Advertisement(&'static str),
	#[error("discovery table is full")]
	TableFull,
}

impl From<mdns_sd::Error> for DiscoveryError {
	fn from(error: mdns_sd::Error) -> Self {
		Self::Daemon(error.to_string())
	}
}
