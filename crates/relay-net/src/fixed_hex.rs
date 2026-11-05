//! Fixed-width hex parsing for untrusted text, on top of the `hex` crate.

/// Decodes exactly `N` bytes of hex (either case). Any other length, sign or non-hex
/// character yields `None`.
pub(crate) fn decode<const N: usize>(text: &str) -> Option<[u8; N]> {
	let mut out = [0u8; N];
	hex::decode_to_slice(text, &mut out).ok()?;
	Some(out)
}
