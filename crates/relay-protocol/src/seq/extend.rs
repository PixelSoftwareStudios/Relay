//! Extension of wrapping 16-bit sequence numbers and 32-bit RTP timestamps
//! to monotonic 64-bit values.
//!
//! The first value seen is placed one full wrap above zero so that packets
//! reordered slightly *before* the first one never underflow.

macro_rules! extender {
	($name:ident, $narrow:ty, $doc:literal) => {
		#[doc = $doc]
		#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
		pub struct $name {
			reference: Option<u64>,
		}

		impl $name {
			const SPAN: u64 = 1 << <$narrow>::BITS;

			pub const fn new() -> Self {
				Self { reference: None }
			}

			/// Resolves `value` to the extended number nearest the reference
			/// without moving the reference.
			pub fn resolve(&self, value: $narrow) -> u64 {
				let Some(reference) = self.reference else {
					return Self::SPAN + u64::from(value);
				};
				let base = reference & !(Self::SPAN - 1);
				let candidate = base | u64::from(value);
				let half = Self::SPAN / 2;
				if candidate > reference + half {
					candidate.saturating_sub(Self::SPAN)
				} else if candidate + half <= reference {
					candidate + Self::SPAN
				} else {
					candidate
				}
			}

			/// Resolves `value` and advances the reference if it is newer.
			pub fn extend(&mut self, value: $narrow) -> u64 {
				let extended = self.resolve(value);
				self.commit(extended);
				extended
			}

			/// Moves the reference forward to `extended` if it is newer.
			pub fn commit(&mut self, extended: u64) {
				if self.reference.is_none_or(|r| extended > r) {
					self.reference = Some(extended);
				}
			}

			/// Highest extended value committed so far.
			pub const fn highest(&self) -> Option<u64> {
				self.reference
			}

			/// Forgets the reference, e.g. on a new stream generation.
			pub fn reset(&mut self) {
				self.reference = None;
			}
		}
	};
}

extender!(
	SeqExtender,
	u16,
	"Extends 16-bit RTP sequence numbers to 64 bits."
);
extender!(
	TimestampExtender,
	u32,
	"Extends 32-bit RTP timestamps to 64 bits."
);
