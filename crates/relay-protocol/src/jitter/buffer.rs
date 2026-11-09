use super::{
	ConcealCause, Concealment, InterarrivalJitter, JitterConfig, JitterStats, MarginController,
	Played, Playout, SkipReason,
};
use crate::seq::{ReplayVerdict, ReplayWindow, SeqExtender, TimestampExtender};

/// Header fields the buffer needs; time is supplied by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketMeta {
	pub sequence: u16,
	pub timestamp: u32,
	/// Arrival time in microseconds on any monotonic clock.
	pub arrival_us: u64,
}

/// What happened to an inserted packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertOutcome {
	Stored,
	Duplicate,
	/// Past its deadline; dropped, never played after newer speech.
	Late,
	/// Beyond the admission horizon or colliding with a pending slot.
	TooFar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
	Idle,
	Prefill { left: u32 },
	Playing,
	Interrupted,
}

#[derive(Debug)]
struct Slot<T> {
	sequence: u64,
	timestamp: u64,
	payload: T,
}

/// Receive scheduler for one stream generation.
///
/// Memory is fixed at construction. Playout delay can only grow through
/// prefill and one-frame stretches, both capped by the configured ceiling.
#[derive(Debug)]
pub struct JitterBuffer<T> {
	cfg: JitterConfig,
	slots: Box<[Option<Slot<T>>]>,
	seq: SeqExtender,
	ts: TimestampExtender,
	seen: ReplayWindow<2>,
	state: State,
	next_seq: u64,
	play_ts: u64,
	last_played: Option<u64>,
	conceal_run: u32,
	margin: MarginController,
	effective_margin: u32,
	highest_ts: u64,
	jitter: InterarrivalJitter,
	stats: JitterStats,
}

impl<T> JitterBuffer<T> {
	/// Creates a receive scheduler for a trusted, static configuration.
	///
	/// # Panics
	/// Panics for invalid configuration; use [`Self::try_new`] for external input.
	pub fn new(cfg: JitterConfig) -> Self {
		Self::try_new(cfg).expect("invalid jitter configuration")
	}

	/// Creates a scheduler after validating all configuration bounds.
	///
	/// # Errors
	/// Returns [`crate::error::ConfigError`] for invalid slot, frame, or margin limits.
	pub fn try_new(cfg: JitterConfig) -> Result<Self, crate::error::ConfigError> {
		let margin = MarginController::try_new(&cfg)?;
		Ok(Self {
			slots: (0..cfg.slots).map(|_| None).collect(),
			seq: SeqExtender::new(),
			ts: TimestampExtender::new(),
			seen: ReplayWindow::new(),
			state: State::Idle,
			next_seq: 0,
			play_ts: 0,
			last_played: None,
			conceal_run: 0,
			effective_margin: margin.target(),
			margin,
			highest_ts: 0,
			jitter: InterarrivalJitter::new(),
			stats: JitterStats::default(),
			cfg,
		})
	}

	pub const fn stats(&self) -> &JitterStats {
		&self.stats
	}

	/// RFC 3550 jitter estimate in samples.
	pub fn jitter_samples(&self) -> u32 {
		self.jitter.samples()
	}

	/// Margin currently realised by the playhead, in frames.
	pub const fn margin_frames(&self) -> u32 {
		self.effective_margin
	}

	/// Margin the controller wants, in frames.
	pub const fn target_margin_frames(&self) -> u32 {
		self.margin.target()
	}

	/// Packets currently stored.
	pub fn buffered(&self) -> usize {
		self.slots.iter().flatten().count()
	}

	/// Samples of audio queued ahead of the playhead, for drift estimation.
	pub fn occupancy_samples(&self) -> u32 {
		if self.state != State::Playing {
			return 0;
		}
		let end = self
			.highest_ts
			.saturating_add(u64::from(self.cfg.frame_samples));
		end.saturating_sub(self.play_ts).min(u64::from(u32::MAX)) as u32
	}

	/// Drops all state, e.g. on a new stream generation.
	pub fn reset(&mut self) {
		self.slots.iter_mut().for_each(|s| *s = None);
		self.seq.reset();
		self.ts.reset();
		self.seen.reset();
		self.state = State::Idle;
		self.last_played = None;
		self.conceal_run = 0;
		self.margin = MarginController::new(&self.cfg);
		self.effective_margin = self.margin.target();
		self.jitter = InterarrivalJitter::new();
	}

	/// Offers one authenticated packet.
	pub fn insert(&mut self, meta: PacketMeta, payload: T) -> InsertOutcome {
		self.stats.received += 1;
		let (Ok(seq), Ok(ts)) = (
			self.seq.try_resolve(meta.sequence),
			self.ts.try_resolve(meta.timestamp),
		) else {
			self.stats.rejected_far += 1;
			return InsertOutcome::TooFar;
		};

		match self.seen.check(seq) {
			ReplayVerdict::Duplicate => {
				self.stats.duplicate += 1;
				return InsertOutcome::Duplicate;
			}
			ReplayVerdict::TooOld => return self.late(),
			ReplayVerdict::Fresh => {}
		}

		match self.state {
			State::Idle => self.begin_segment(seq, ts),
			State::Interrupted => {
				if self.last_played.is_some_and(|last| seq <= last) {
					return self.late();
				}
				self.resume_after_interruption(seq, ts);
			}
			State::Prefill { .. } if seq < self.next_seq => {
				if self.next_seq - seq > self.cfg.horizon_frames() || ts > self.play_ts {
					return self.late();
				}
				self.next_seq = seq;
				self.play_ts = ts;
			}
			State::Prefill { .. } | State::Playing => {}
		}

		let frame = u64::from(self.cfg.frame_samples);
		if seq < self.next_seq || ts.saturating_add(frame) <= self.play_ts {
			self.accept_numbers(seq, ts);
			return self.late();
		}
		if seq - self.next_seq >= self.cfg.horizon_frames() {
			self.stats.rejected_far += 1;
			return InsertOutcome::TooFar;
		}
		let index = (seq % self.slots.len() as u64) as usize;
		if self.slots[index].is_some() {
			self.stats.rejected_far += 1;
			return InsertOutcome::TooFar;
		}

		self.accept_numbers(seq, ts);
		self.jitter.update(meta.arrival_us, ts);
		self.highest_ts = self.highest_ts.max(ts);
		self.slots[index] = Some(Slot {
			sequence: seq,
			timestamp: ts,
			payload,
		});
		InsertOutcome::Stored
	}

	/// Produces the decision for the next render frame.
	pub fn poll(&mut self) -> Playout<T> {
		self.margin.on_tick();
		match self.state {
			State::Idle => Playout::Skip(SkipReason::Idle),
			State::Interrupted => Playout::Skip(SkipReason::Interrupted),
			State::Prefill { left } if left > 0 => {
				self.state = State::Prefill { left: left - 1 };
				Playout::Skip(SkipReason::Prefill)
			}
			State::Prefill { .. } => {
				self.state = State::Playing;
				self.poll_playing()
			}
			State::Playing => self.poll_playing(),
		}
	}

	fn late(&mut self) -> InsertOutcome {
		self.stats.late_drop += 1;
		self.margin.on_miss();
		InsertOutcome::Late
	}

	fn accept_numbers(&mut self, seq: u64, ts: u64) {
		self.seen.accept(seq);
		self.seq.commit(seq);
		self.ts.commit(ts);
	}

	fn begin_segment(&mut self, seq: u64, ts: u64) {
		for slot in self.slots.iter_mut() {
			if slot.as_ref().is_some_and(|s| s.sequence < seq) {
				*slot = None;
			}
		}
		self.next_seq = seq;
		self.play_ts = ts;
		self.highest_ts = ts;
		self.conceal_run = 0;
		self.effective_margin = self.margin.target();
		self.state = State::Prefill {
			left: self.effective_margin,
		};
	}

	fn resume_after_interruption(&mut self, seq: u64, ts: u64) {
		if let Some(last) = self.last_played {
			match seq.saturating_sub(last).saturating_sub(1) {
				0 => self.stats.intentional_silence += 1,
				missing => self.stats.network_missing += missing,
			}
		}
		self.begin_segment(seq, ts);
	}

	/// Lowest-sequence stored packet at or after `next_seq`.
	fn head(&self) -> Option<(u64, u64)> {
		let len = self.slots.len() as u64;
		(self.next_seq
			..self
				.next_seq
				.saturating_add(self.cfg.horizon_frames())
				.saturating_add(1))
			.find_map(|seq| {
				let slot = self.slots[(seq % len) as usize].as_ref()?;
				(slot.sequence == seq).then_some((slot.sequence, slot.timestamp))
			})
	}

	fn take(&mut self, seq: u64) -> Slot<T> {
		let index = (seq % self.slots.len() as u64) as usize;
		self.slots[index].take().expect("head slot is occupied")
	}

	fn poll_playing(&mut self) -> Playout<T> {
		let frame = u64::from(self.cfg.frame_samples);
		let mut head = self.head();
		while let Some((seq, ts)) = head {
			if ts.saturating_add(frame) > self.play_ts {
				break;
			}
			drop(self.take(seq));
			self.stats.late_drop += 1;
			self.stats.network_missing += seq.saturating_sub(self.next_seq);
			self.next_seq = seq.saturating_add(1);
			self.margin.on_miss();
			head = self.head();
		}

		let Some((seq, ts)) = head else {
			return self.conceal();
		};
		if ts > self.play_ts {
			if ts - self.play_ts > u64::from(self.cfg.max_gap_samples) {
				self.begin_segment(seq, ts);
				return Playout::Skip(SkipReason::Gap);
			}
			if seq == self.next_seq {
				return self.gap(ts);
			}
			return self.conceal();
		}
		if self.effective_margin < self.margin.target() && self.conceal_run == 0 {
			self.effective_margin += 1;
			self.stats.stretches += 1;
			return Playout::Conceal(Concealment {
				run_frames: 1,
				fade: false,
				cause: ConcealCause::Stretch,
			});
		}

		let slot = self.take(seq);
		let lost_before = (seq - self.next_seq) as u32;
		self.stats.network_missing += u64::from(lost_before);
		self.stats.played += 1;
		self.next_seq = seq.saturating_add(1);
		self.play_ts = ts.saturating_add(frame);
		self.last_played = Some(seq);
		self.conceal_run = 0;
		Playout::Play(Played {
			payload: slot.payload,
			sequence: seq,
			timestamp: ts,
			lost_before,
		})
	}

	/// Deliberate silence ahead of a contiguous packet.
	fn gap(&mut self, head_ts: u64) -> Playout<T> {
		let frame = u64::from(self.cfg.frame_samples);
		self.play_ts = self.play_ts.saturating_add(frame);
		if self.effective_margin > self.margin.target()
			&& head_ts >= self.play_ts.saturating_add(frame)
		{
			self.play_ts = self.play_ts.saturating_add(frame);
			self.effective_margin -= 1;
			self.stats.shrinks += 1;
		}
		Playout::Skip(SkipReason::Gap)
	}

	fn conceal(&mut self) -> Playout<T> {
		let frame = u64::from(self.cfg.frame_samples);
		self.conceal_run += 1;
		if self.conceal_run == 1 {
			self.margin.on_miss();
		}
		let plc = self.cfg.max_plc_frames;
		if self.conceal_run > plc + self.cfg.fade_frames {
			self.state = State::Interrupted;
			self.stats.interruptions += 1;
			return Playout::Skip(SkipReason::Interrupted);
		}
		self.play_ts = self.play_ts.saturating_add(frame);
		self.stats.concealed += 1;
		Playout::Conceal(Concealment {
			run_frames: self.conceal_run,
			fade: self.conceal_run > plc,
			cause: ConcealCause::Loss,
		})
	}
}
