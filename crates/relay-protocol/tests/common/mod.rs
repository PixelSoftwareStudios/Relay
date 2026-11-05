//! Shared deterministic simulation harness: one packet per 10 ms frame, one
//! render poll per frame, packets delivered on explicit arrival ticks.
#![allow(dead_code)]

use relay_protocol::jitter::{InsertOutcome, JitterBuffer, JitterConfig, PacketMeta, Playout};

pub const FRAME: u32 = 480;

#[derive(Debug, Clone)]
pub struct Arrival {
	/// Render tick at which the packet reaches the buffer.
	pub tick: u32,
	/// Index of the packet in the sender's stream.
	pub index: u32,
	/// Extra timestamp offset in frames, to model DTX gaps.
	pub ts_skip: u32,
}

pub struct Sim {
	pub buf: JitterBuffer<u32>,
	pub seq_base: u16,
	pub ts_base: u32,
	pub log: Vec<Playout<u32>>,
	pub outcomes: Vec<InsertOutcome>,
	pub tick: u32,
}

impl Sim {
	pub fn new(cfg: JitterConfig) -> Self {
		Self::with_bases(cfg, 0, 0)
	}

	pub fn with_bases(cfg: JitterConfig, seq_base: u16, ts_base: u32) -> Self {
		Self {
			buf: JitterBuffer::new(cfg),
			seq_base,
			ts_base,
			log: Vec::new(),
			outcomes: Vec::new(),
			tick: 0,
		}
	}

	pub fn meta(&self, index: u32, ts_skip: u32) -> PacketMeta {
		PacketMeta {
			sequence: self.seq_base.wrapping_add(index as u16),
			timestamp: self
				.ts_base
				.wrapping_add((index + ts_skip).wrapping_mul(FRAME)),
			arrival_us: u64::from(self.tick) * 10_000,
		}
	}

	pub fn deliver(&mut self, index: u32, ts_skip: u32) -> InsertOutcome {
		let meta = self.meta(index, ts_skip);
		let outcome = self.buf.insert(meta, index);
		self.outcomes.push(outcome);
		outcome
	}

	/// Runs `ticks` render frames, delivering arrivals scheduled per tick.
	pub fn run(&mut self, arrivals: &[Arrival], ticks: u32) {
		for _ in 0..ticks {
			let now = self.tick;
			for a in arrivals.iter().filter(|a| a.tick == now) {
				self.deliver(a.index, a.ts_skip);
			}
			self.log.push(self.buf.poll());
			self.tick += 1;
		}
	}

	/// Decisions up to the last played packet, excluding the end-of-stream tail.
	pub fn body(&self) -> &[Playout<u32>] {
		let end = self.log.iter().rposition(|p| matches!(p, Playout::Play(_)));
		&self.log[..end.map_or(0, |e| e + 1)]
	}

	pub fn concealed_in_body(&self) -> usize {
		self.body()
			.iter()
			.filter(|p| matches!(p, Playout::Conceal(_)))
			.count()
	}

	pub fn played(&self) -> Vec<u32> {
		self.log
			.iter()
			.filter_map(|p| match p {
				Playout::Play(p) => Some(p.payload),
				_ => None,
			})
			.collect()
	}
}

/// In-order stream: packet `i` arrives at tick `i`.
pub fn steady(count: u32) -> Vec<Arrival> {
	(0..count)
		.map(|i| Arrival {
			tick: i,
			index: i,
			ts_skip: 0,
		})
		.collect()
}
