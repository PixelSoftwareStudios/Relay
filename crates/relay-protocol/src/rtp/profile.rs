use super::header::{RtpHeader, RtpPacket};
use crate::error::RtpError;

/// Negotiated limits a received packet is validated against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RtpProfile {
	pub payload_type: u8,
	pub max_payload: usize,
}

impl RtpProfile {
	/// Relay v1: dynamic payload type 111, Opus packets up to 1000 bytes.
	pub const V1: Self = Self {
		payload_type: 111,
		max_payload: 1000,
	};

	/// Validates every flag and length, then borrows the payload.
	pub fn parse<'a>(&self, datagram: &'a [u8]) -> Result<RtpPacket<'a>, RtpError> {
		let (header, payload) = RtpHeader::decode(datagram)?;
		if header.payload_type != self.payload_type {
			return Err(RtpError::PayloadType(header.payload_type));
		}
		if payload.is_empty() {
			return Err(RtpError::EmptyPayload);
		}
		if payload.len() > self.max_payload {
			return Err(RtpError::PayloadTooLarge {
				len: payload.len(),
				max: self.max_payload,
			});
		}
		Ok(RtpPacket { header, payload })
	}

	/// Serializes `header` and `payload` into `out`, returning bytes written.
	pub fn write(
		&self,
		header: &RtpHeader,
		payload: &[u8],
		out: &mut [u8],
	) -> Result<usize, RtpError> {
		if header.payload_type != self.payload_type {
			return Err(RtpError::PayloadType(header.payload_type));
		}
		if payload.is_empty() {
			return Err(RtpError::EmptyPayload);
		}
		if payload.len() > self.max_payload {
			return Err(RtpError::PayloadTooLarge {
				len: payload.len(),
				max: self.max_payload,
			});
		}
		let need = super::HEADER_LEN + payload.len();
		if out.len() < need {
			return Err(RtpError::OutputTooSmall { need });
		}
		header.encode(&mut out[..super::HEADER_LEN]);
		out[super::HEADER_LEN..need].copy_from_slice(payload);
		Ok(need)
	}
}
