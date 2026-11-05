//! Thin sample-oriented wrappers over the wait-free SPSC ring.

use rtrb::RingBuffer;

/// Create a bounded `f32` ring; all storage is allocated here.
pub fn sample_ring(capacity: usize) -> (SampleProducer, SampleConsumer) {
	let (producer, consumer) = RingBuffer::new(capacity.max(1));
	(SampleProducer(producer), SampleConsumer(consumer))
}

/// Producing half. Never blocks, allocates or grows.
pub struct SampleProducer(rtrb::Producer<f32>);

impl SampleProducer {
	/// Push as many samples as fit; returns the number written.
	pub fn push(&mut self, samples: &[f32]) -> usize {
		let (written, _rest) = self.0.push_partial_slice(samples);
		written.len()
	}

	pub fn vacant(&self) -> usize {
		self.0.slots()
	}

	pub fn capacity(&self) -> usize {
		self.0.buffer().capacity()
	}
}

/// Consuming half.
pub struct SampleConsumer(rtrb::Consumer<f32>);

impl SampleConsumer {
	/// Pop up to `out.len()` samples; returns the number read.
	pub fn pop(&mut self, out: &mut [f32]) -> usize {
		let (read, _rest) = self.0.pop_partial_slice(out);
		read.len()
	}

	pub fn len(&self) -> usize {
		self.0.slots()
	}

	pub fn is_empty(&self) -> bool {
		self.0.is_empty()
	}

	/// Discard up to `count` samples; returns the number discarded.
	pub fn discard(&mut self, count: usize) -> usize {
		let n = count.min(self.0.slots());
		match self.0.read_chunk(n) {
			Ok(chunk) => chunk.commit_all(),
			Err(_) => return 0,
		}
		n
	}

	pub fn discard_all(&mut self) -> usize {
		self.discard(self.len())
	}
}
