//! Extension of wrapping 16-bit sequence numbers and 32-bit RTP timestamps
//! to monotonic 64-bit values.
//!
//! The first value seen is placed one full wrap above zero so that packets
//! reordered slightly *before* the first one never underflow.

use crate::error::ExtendError;

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
			///
			/// # Panics
			/// Panics if the nearest number exceeds `u64::MAX`. Use [`Self::try_resolve`]
			/// when references may approach timeline exhaustion.
			pub fn resolve(&self, value: $narrow) -> u64 {
				self.try_resolve(value).expect("extended counter exhausted")
			}

			/// Resolves a wrapping value without changing the reference.
			///
			/// # Errors
			/// Returns [`ExtendError`] if the nearest extended value cannot fit in `u64`.
			pub fn try_resolve(&self, value: $narrow) -> Result<u64, ExtendError> {
				let Some(reference) = self.reference else {
					return Ok(Self::SPAN + u64::from(value));
				};
				let reference = u128::from(reference);
				let span = u128::from(Self::SPAN);
				let base = reference & !(span - 1);
				let candidate = base | u128::from(value);
				let half = span / 2;
				let extended = if candidate > reference + half {
					candidate.saturating_sub(span)
				} else if candidate + half <= reference {
					candidate + span
				} else {
					candidate
				};
				u64::try_from(extended).map_err(|_| ExtendError)
			}

			/// Resolves `value` and advances the reference if it is newer.
			///
			/// # Panics
			/// Panics on timeline exhaustion; use [`Self::try_extend`] for a fallible operation.
			pub fn extend(&mut self, value: $narrow) -> u64 {
				self.try_extend(value).expect("extended counter exhausted")
			}

			/// Resolves a value and commits it only when representable.
			///
			/// # Errors
			/// Returns [`ExtendError`] at timeline exhaustion without changing the reference.
			pub fn try_extend(&mut self, value: $narrow) -> Result<u64, ExtendError> {
				let extended = self.try_resolve(value)?;
				self.commit(extended);
				Ok(extended)
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
