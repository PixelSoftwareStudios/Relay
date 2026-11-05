//! The fixed Relay RTP profile: RTP v2, 12-byte header, no CSRC, no
//! extension, no padding, one Opus frame per packet.

mod header;
mod profile;

pub use header::{HEADER_LEN, RtpHeader, RtpPacket};
pub use profile::RtpProfile;
