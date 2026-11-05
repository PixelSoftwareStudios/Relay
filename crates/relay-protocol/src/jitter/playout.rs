/// One render-tick decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Playout<T> {
	/// Decode and play this packet.
	Play(Played<T>),
	/// Ask the decoder for packet-loss concealment of one frame.
	Conceal(Concealment),
	/// Emit one frame of silence without involving the decoder.
	Skip(SkipReason),
}

/// A packet that is due.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Played<T> {
	pub payload: T,
	/// Extended sequence number.
	pub sequence: u64,
	/// Extended RTP timestamp.
	pub timestamp: u64,
	/// Packets known to be lost immediately before this one.
	pub lost_before: u32,
}

/// Concealment request for one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Concealment {
	/// Consecutive concealed frames including this one.
	pub run_frames: u32,
	/// Apply the fade-to-silence ramp to this frame.
	pub fade: bool,
	pub cause: ConcealCause,
}

/// Why concealment was requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConcealCause {
	/// Packet missing at its deadline.
	Loss,
	/// Deliberate one-frame hold that deepens the buffer.
	Stretch,
}

/// Why a silent frame is emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
	/// Nothing received yet.
	Idle,
	/// Filling the playout margin.
	Prefill,
	/// Deliberate timestamp gap (DTX / talk-spurt pause); not loss.
	Gap,
	/// Concealment limit exceeded; waiting for fresh packets.
	Interrupted,
}
