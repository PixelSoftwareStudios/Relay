use crate::error::RtpError;

/// Size of the fixed RTP header.
pub const HEADER_LEN: usize = 12;

const VERSION: u8 = 2;

/// Decoded fixed header fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RtpHeader {
	/// Set on the first packet of a talk spurt.
	pub marker: bool,
	pub payload_type: u8,
	pub sequence: u16,
	pub timestamp: u32,
	pub ssrc: u32,
}

/// A validated packet borrowing its payload from the datagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RtpPacket<'a> {
	pub header: RtpHeader,
	pub payload: &'a [u8],
}

impl RtpHeader {
	/// Decodes the header and returns the remaining bytes. Payload policy is
	/// left to [`super::RtpProfile`].
	pub(super) fn decode(datagram: &[u8]) -> Result<(Self, &[u8]), RtpError> {
		let Some((head, payload)) = datagram.split_first_chunk::<HEADER_LEN>() else {
			return Err(RtpError::TooShort);
		};
		let version = head[0] >> 6;
		if version != VERSION {
			return Err(RtpError::BadVersion(version));
		}
		if head[0] & 0x20 != 0 {
			return Err(RtpError::Padding);
		}
		if head[0] & 0x10 != 0 {
			return Err(RtpError::Extension);
		}
		if head[0] & 0x0f != 0 {
			return Err(RtpError::Csrc);
		}
		let header = Self {
			marker: head[1] & 0x80 != 0,
			payload_type: head[1] & 0x7f,
			sequence: u16::from_be_bytes([head[2], head[3]]),
			timestamp: u32::from_be_bytes([head[4], head[5], head[6], head[7]]),
			ssrc: u32::from_be_bytes([head[8], head[9], head[10], head[11]]),
		};
		Ok((header, payload))
	}

	/// Writes the 12 header bytes. `out` must be exactly [`HEADER_LEN`] long.
	pub(super) fn encode(&self, out: &mut [u8]) {
		out[0] = VERSION << 6;
		out[1] = (u8::from(self.marker) << 7) | (self.payload_type & 0x7f);
		out[2..4].copy_from_slice(&self.sequence.to_be_bytes());
		out[4..8].copy_from_slice(&self.timestamp.to_be_bytes());
		out[8..12].copy_from_slice(&self.ssrc.to_be_bytes());
	}
}
