/// Counters following the documented semantics. Totals are not independent:
/// one late packet may count both as `late_drop` and inside `network_missing`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JitterStats {
	/// Authenticated packets offered to the buffer.
	pub received: u64,
	/// Packets handed out for decoding.
	pub played: u64,
	/// Frames of concealment requested because of loss.
	pub concealed: u64,
	/// Sequence numbers confirmed missing after the reorder horizon.
	pub network_missing: u64,
	/// Packets that arrived after their useful deadline.
	pub late_drop: u64,
	pub duplicate: u64,
	/// Packets beyond the admission horizon or colliding with a slot.
	pub rejected_far: u64,
	/// Resumptions after a deliberate silence (not loss).
	pub intentional_silence: u64,
	pub interruptions: u64,
	pub stretches: u64,
	pub shrinks: u64,
}
