use criterion::{Criterion, criterion_group, criterion_main};
use relay_protocol::jitter::{JitterBuffer, JitterConfig, PacketMeta};
use relay_protocol::rtp::{RtpHeader, RtpProfile};
use std::hint::black_box;

fn bench_parse(c: &mut Criterion) {
	let profile = RtpProfile::V1;
	let header = RtpHeader {
		marker: false,
		payload_type: 111,
		sequence: 7,
		timestamp: 480,
		ssrc: 1,
	};
	let mut datagram = [0u8; 52];
	profile.write(&header, &[0xab; 40], &mut datagram).unwrap();
	c.bench_function("rtp_parse", |b| {
		b.iter(|| profile.parse(black_box(&datagram)).unwrap())
	});
}

fn bench_pipeline(c: &mut Criterion) {
	c.bench_function("jitter_insert_poll_10ms_frame", |b| {
		let mut buffer = JitterBuffer::<u32>::new(JitterConfig::BALANCED);
		let mut n: u32 = 0;
		b.iter(|| {
			let meta = PacketMeta {
				sequence: n as u16,
				timestamp: n.wrapping_mul(480),
				arrival_us: u64::from(n) * 10_000,
			};
			buffer.insert(meta, n);
			n = n.wrapping_add(1);
			black_box(buffer.poll())
		})
	});
}

criterion_group!(benches, bench_parse, bench_pipeline);
criterion_main!(benches);
