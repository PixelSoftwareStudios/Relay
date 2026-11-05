//! Re-chunks audio between producers and consumers with different frame
//! sizes (e.g. 8 ms DSP output into 10 ms codec input).
//!
//! A frame is released the moment enough samples exist; nothing waits for a
//! common multiple, so the added latency is at most one output frame minus
//! one sample.

use crate::error::AssemblyError;

/// Fixed-capacity FIFO that emits `frame_len`-sample frames.
#[derive(Debug, Clone)]
pub struct FrameAssembler<T> {
	buf: Box<[T]>,
	head: usize,
	len: usize,
	frame_len: usize,
}

impl<T: Copy + Default> FrameAssembler<T> {
	/// `max_push` is the largest slice a single [`Self::push`] will carry.
	pub fn new(frame_len: usize, max_push: usize) -> Self {
		assert!(frame_len > 0, "frame length must be non-zero");
		let capacity = frame_len - 1 + max_push;
		Self {
			buf: vec![T::default(); capacity.max(1)].into_boxed_slice(),
			head: 0,
			len: 0,
			frame_len,
		}
	}

	pub const fn frame_len(&self) -> usize {
		self.frame_len
	}

	/// Samples waiting for the next frame.
	pub const fn buffered(&self) -> usize {
		self.len
	}

	/// Whether a full frame can be popped.
	pub const fn has_frame(&self) -> bool {
		self.len >= self.frame_len
	}

	/// Appends samples. Pop every ready frame before the next push.
	pub fn push(&mut self, samples: &[T]) -> Result<(), AssemblyError> {
		let capacity = self.buf.len();
		if self.len + samples.len() > capacity {
			return Err(AssemblyError::Overflow {
				pushed: samples.len(),
				capacity,
			});
		}
		let tail = (self.head + self.len) % capacity;
		let first = samples.len().min(capacity - tail);
		self.buf[tail..tail + first].copy_from_slice(&samples[..first]);
		self.buf[..samples.len() - first].copy_from_slice(&samples[first..]);
		self.len += samples.len();
		Ok(())
	}

	/// Copies one frame into `out` (which must hold `frame_len` samples).
	/// Returns `false` when not enough samples are buffered.
	pub fn pop_frame(&mut self, out: &mut [T]) -> bool {
		if !self.has_frame() {
			return false;
		}
		let capacity = self.buf.len();
		let out = &mut out[..self.frame_len];
		let first = self.frame_len.min(capacity - self.head);
		out[..first].copy_from_slice(&self.buf[self.head..self.head + first]);
		out[first..].copy_from_slice(&self.buf[..self.frame_len - first]);
		self.head = (self.head + self.frame_len) % capacity;
		self.len -= self.frame_len;
		true
	}

	/// Discards buffered samples, e.g. at a talk-spurt boundary.
	pub fn clear(&mut self) {
		self.head = 0;
		self.len = 0;
	}
}
