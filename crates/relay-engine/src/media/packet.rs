//! Fixed-size packet buffers: the receive path never allocates per packet.

use relay_net::media::CallId as WireCallId;
use relay_protocol::rtp::RtpProfile;

/// Largest Opus payload the profile admits.
pub const PAYLOAD_MAX: usize = RtpProfile::V1.max_payload;
/// Largest RTP packet (header plus payload).
pub const RTP_MAX: usize = relay_protocol::rtp::HEADER_LEN + PAYLOAD_MAX;

/// One Opus payload in a fixed buffer, stored in the jitter buffer.
#[derive(Clone)]
pub struct PacketBuf {
	len: u16,
	data: [u8; PAYLOAD_MAX],
}

impl Default for PacketBuf {
	fn default() -> Self {
		Self {
			len: 0,
			data: [0; PAYLOAD_MAX],
		}
	}
}

impl PacketBuf {
	/// Copies `payload`; `None` if it exceeds the profile limit.
	pub fn from_payload(payload: &[u8]) -> Option<Self> {
		let mut buf = Self::default();
		buf.data.get_mut(..payload.len())?.copy_from_slice(payload);
		buf.len = payload.len() as u16;
		Some(buf)
	}

	/// The stored payload.
	pub fn payload(&self) -> &[u8] {
		&self.data[..usize::from(self.len)]
	}
}

/// An authenticated RTP packet handed from the network task to the render
/// worker through a bounded queue.
#[derive(Clone)]
pub struct Ingress {
	/// The call the sealed datagram was bound to.
	pub call: WireCallId,
	/// The stream generation the sealed datagram was bound to.
	pub generation: u32,
	/// Arrival time in microseconds since the media system started.
	pub arrival_us: u64,
	len: u16,
	rtp: [u8; RTP_MAX],
}

impl Ingress {
	/// Copies `rtp`; `None` if it exceeds the profile limit.
	pub fn new(call: WireCallId, generation: u32, arrival_us: u64, rtp: &[u8]) -> Option<Self> {
		let mut packet = Self {
			call,
			generation,
			arrival_us,
			len: 0,
			rtp: [0; RTP_MAX],
		};
		packet.rtp.get_mut(..rtp.len())?.copy_from_slice(rtp);
		packet.len = rtp.len() as u16;
		Some(packet)
	}

	/// The RTP bytes.
	pub fn rtp(&self) -> &[u8] {
		&self.rtp[..usize::from(self.len)]
	}
}
