#![allow(dead_code)]

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use relay_messages::message::*;
use relay_messages::transfer::*;
use relay_messages::{Clock, SafeFileName, SessionInfo, SessionStore};

pub struct TestClock {
	wall: AtomicU64,
	mono: AtomicU64,
}

impl TestClock {
	pub fn new() -> Arc<Self> {
		Arc::new(Self {
			wall: AtomicU64::new(1_700_000_000_000),
			mono: AtomicU64::new(1_000),
		})
	}
	pub fn advance(&self, ms: u64) {
		self.wall.fetch_add(ms, Ordering::SeqCst);
		self.mono.fetch_add(ms, Ordering::SeqCst);
	}
}

impl Clock for TestClock {
	fn wall_ms(&self) -> u64 {
		self.wall.load(Ordering::SeqCst)
	}
	fn mono_ms(&self) -> u64 {
		self.mono.load(Ordering::SeqCst)
	}
}

pub fn peer(n: u8) -> PeerRef {
	PeerRef {
		fingerprint: PeerFingerprint([n; 32]),
		display_name: format!("peer-{n}"),
	}
}

pub fn info(session: u8, peer_n: u8) -> SessionInfo {
	SessionInfo {
		id: SessionId([session; 16]),
		peer: peer(peer_n),
		started_at_ms: 1,
	}
}

pub fn store_with(limit: u64) -> (SessionStore, Arc<TestClock>) {
	let clock = TestClock::new();
	let s = SessionStore::with_limit(info(1, 1), Permissions::default(), clock.clone(), limit);
	(s, clock)
}

pub fn store() -> (SessionStore, Arc<TestClock>) {
	store_with(relay_messages::limits::SESSION_BUDGET_BYTES)
}

pub fn wire_text(seq: u64, t: &str) -> Wire {
	Wire {
		seq,
		sent_at_ms: 5,
		payload: Payload::Text(t.to_owned()),
	}
}

pub fn meta(id: u128, name: &str, data: &[u8]) -> OfferMeta {
	OfferMeta {
		id: OfferId(id),
		name: name.to_owned(),
		size: data.len() as u64,
		hash: *blake3::hash(data).as_bytes(),
	}
}

pub fn offer(id: u128, name: &str, data: &[u8]) -> FileOffer {
	relay_messages::transfer::offer_from_meta(&meta(id, name, data)).unwrap()
}

// ---- image builders (header only) ----

pub fn png(w: u32, h: u32) -> Vec<u8> {
	let mut v = b"\x89PNG\r\n\x1a\n".to_vec();
	v.extend_from_slice(&13u32.to_be_bytes());
	v.extend_from_slice(b"IHDR");
	v.extend_from_slice(&w.to_be_bytes());
	v.extend_from_slice(&h.to_be_bytes());
	v.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
	v.extend_from_slice(&0u32.to_be_bytes());
	v.extend_from_slice(b"IDAT");
	v
}

pub fn apng(w: u32, h: u32, frames: u32) -> Vec<u8> {
	let mut v = png(w, h);
	v.truncate(8 + 25);
	v.extend_from_slice(&8u32.to_be_bytes());
	v.extend_from_slice(b"acTL");
	v.extend_from_slice(&frames.to_be_bytes());
	v.extend_from_slice(&0u32.to_be_bytes());
	v.extend_from_slice(&[0; 4]);
	v.extend_from_slice(&0u32.to_be_bytes());
	v.extend_from_slice(b"IDAT");
	v
}

pub fn jpeg(w: u16, h: u16) -> Vec<u8> {
	let mut v = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0];
	v.extend_from_slice(&[0xFF, 0xC0, 0, 11, 8]);
	v.extend_from_slice(&h.to_be_bytes());
	v.extend_from_slice(&w.to_be_bytes());
	v.extend_from_slice(&[1, 1, 0x11, 0]);
	v
}

pub fn gif(w: u16, h: u16, frames: usize) -> Vec<u8> {
	let mut v = b"GIF89a".to_vec();
	v.extend_from_slice(&w.to_le_bytes());
	v.extend_from_slice(&h.to_le_bytes());
	v.extend_from_slice(&[0, 0, 0]);
	for _ in 0..frames {
		v.push(0x2C);
		v.extend_from_slice(&[0, 0, 0, 0]);
		v.extend_from_slice(&w.to_le_bytes());
		v.extend_from_slice(&h.to_le_bytes());
		v.extend_from_slice(&[0, 2, 1, 0, 0]);
	}
	v.push(0x3B);
	v
}

pub fn webp_vp8x(w: u32, h: u32, frames: usize) -> Vec<u8> {
	let mut v = b"RIFF\0\0\0\0WEBPVP8X".to_vec();
	v.extend_from_slice(&10u32.to_le_bytes());
	v.push(if frames > 0 { 0x02 } else { 0 });
	v.extend_from_slice(&[0, 0, 0]);
	v.extend_from_slice(&(w - 1).to_le_bytes()[..3]);
	v.extend_from_slice(&(h - 1).to_le_bytes()[..3]);
	for _ in 0..frames {
		v.extend_from_slice(b"ANMF");
		v.extend_from_slice(&16u32.to_le_bytes());
		v.extend_from_slice(&[0; 16]);
	}
	v
}

// ---- in-memory file sink and a loop-back transfer harness ----

#[derive(Default)]
pub struct MemSink {
	pub free: u64,
	pub temp: Option<Vec<u8>>,
	pub existing: Vec<String>,
	pub committed: Vec<(String, Vec<u8>)>,
	pub aborted: bool,
	pub fail_write: bool,
	pub name: String,
	pub synced: bool,
}

impl MemSink {
	pub fn new(free: u64) -> Self {
		Self {
			free,
			..Self::default()
		}
	}
}

impl FileSink for MemSink {
	fn free_space(&mut self) -> Result<u64, SinkError> {
		Ok(self.free)
	}
	fn create_temp(&mut self, name: &SafeFileName, _size: u64) -> Result<(), SinkError> {
		self.name = name.as_str().to_owned();
		self.temp = Some(Vec::new());
		Ok(())
	}
	fn write(&mut self, data: &[u8]) -> Result<(), SinkError> {
		if self.fail_write {
			return Err(SinkError("disk error".into()));
		}
		self.temp.as_mut().unwrap().extend_from_slice(data);
		Ok(())
	}
	fn sync(&mut self) -> Result<(), SinkError> {
		self.synced = true;
		Ok(())
	}
	fn commit(&mut self) -> Result<String, SinkError> {
		assert!(self.synced, "commit before fsync");
		let mut name = self.name.clone();
		let mut n = 1;
		while self.existing.contains(&name) {
			name = format!("{} ({n})", self.name);
			n += 1;
		}
		self.existing.push(name.clone());
		self.committed
			.push((name.clone(), self.temp.take().unwrap()));
		Ok(name)
	}
	fn abort(&mut self) {
		self.temp = None;
		self.aborted = true;
	}
}

pub enum Item {
	ToRecv(TransferMsg),
	ToSend(TransferMsg),
	Read(u64, usize),
	Check,
}

pub struct Harness<'a> {
	pub s: Sender,
	pub r: Receiver,
	pub sink: &'a mut MemSink,
	pub src: Vec<u8>,
	pub corrupt_at: Option<usize>,
	pub checks: VecDeque<bool>,
	pub log: Vec<TransferMsg>,
	q: VecDeque<Item>,
}

impl<'a> Harness<'a> {
	pub fn new(o: &FileOffer, src: Vec<u8>, sink: &'a mut MemSink) -> Self {
		Self {
			s: Sender::new(o),
			r: Receiver::new(o),
			sink,
			src,
			corrupt_at: None,
			checks: VecDeque::new(),
			log: Vec::new(),
			q: VecDeque::new(),
		}
	}

	fn dispatch(&mut self, fx: Vec<Effect>, from_sender: bool) {
		for e in fx {
			match e {
				Effect::Send(m) => {
					self.log.push(m.clone());
					self.q.push_back(if from_sender {
						Item::ToRecv(m)
					} else {
						Item::ToSend(m)
					})
				}
				Effect::ReadChunk { offset, len } => self.q.push_back(Item::Read(offset, len)),
				Effect::CheckSource => self.q.push_back(Item::Check),
			}
		}
	}

	pub fn start(&mut self) {
		let fx = self.r.accept(self.sink).unwrap();
		self.dispatch(fx, false);
		self.run();
	}

	pub fn run(&mut self) {
		while let Some(item) = self.q.pop_front() {
			match item {
				Item::ToRecv(m) => {
					let fx = self.r.on_message(m, self.sink);
					self.dispatch(fx, false);
				}
				Item::ToSend(m) => {
					let fx = self.s.on_message(m);
					self.dispatch(fx, true);
				}
				Item::Read(off, len) => {
					let mut d = self.src[off as usize..off as usize + len].to_vec();
					if let Some(c) = self.corrupt_at
						&& (off as usize..off as usize + len).contains(&c)
					{
						d[c - off as usize] ^= 0xFF;
					}
					let fx = self.s.chunk_read(d);
					self.dispatch(fx, true);
				}
				Item::Check => {
					let ok = self.checks.pop_front().unwrap_or(true);
					let fx = self.s.source_checked(ok);
					self.dispatch(fx, true);
				}
			}
		}
	}
}

pub fn pseudo_data(n: usize) -> Vec<u8> {
	let mut x = 0x9E37_79B9u32;
	(0..n)
		.map(|_| {
			x ^= x << 13;
			x ^= x >> 17;
			x ^= x << 5;
			x as u8
		})
		.collect()
}
