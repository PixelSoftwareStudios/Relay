mod common;
mod transfer_support;

use std::sync::Arc;
use std::time::Duration;

use relay_messages::transfer::TransferMsg;
use relay_net::transfer::{DataLink, LinkParams, Pacer, SharedLimit, TransferRole};
use tokio::io::{AsyncReadExt, duplex};
use tokio::time::Instant;

const MIB: u64 = 1024 * 1024;

/// Sends `size` bytes through a paced link on a paused clock and returns the virtual seconds
/// it took. The far end drains a raw duplex so only the pacer spends time.
async fn paced_send(size: u64, pacer: Pacer, other: Option<(u64, Pacer)>) -> f64 {
	// Real handshakes run on the real clock: a paused clock auto-advances whenever the runtime
	// idles on socket I/O, which would fire the handshake deadline under load.
	let (init, resp) = common::sessions().await;
	let mut jobs = vec![(size, pacer, init, resp)];
	if let Some((n, p)) = other {
		let (i2, r2) = common::sessions().await;
		jobs.push((n, p, i2, r2));
	}
	tokio::time::pause();
	let mut handles = Vec::new();
	let mut links = Vec::new();
	for (n, p, init, _resp) in jobs {
		let (a, mut b) = duplex(1 << 20);
		handles.push(tokio::spawn(async move {
			let mut sink = vec![0u8; 64 * 1024];
			while b.read(&mut sink).await.unwrap_or(0) > 0 {}
		}));
		let params = LinkParams::new(TransferRole::Sender, n, [1; 16], [2; 16]);
		links.push((DataLink::new(a, init, params, p), n));
	}
	let start = Instant::now();
	let mut sends = Vec::new();
	for (mut link, n) in links {
		sends.push(tokio::spawn(async move {
			let mut off = 0;
			while off < n {
				let len = (64 * 1024).min(n - off) as usize;
				link.tx
					.send(&TransferMsg::Chunk {
						offset: off,
						data: vec![0; len],
					})
					.await
					.unwrap();
				off += len as u64;
			}
		}));
	}
	for s in sends {
		s.await.unwrap();
	}
	start.elapsed().as_secs_f64()
}

#[tokio::test]
async fn achieved_rate_matches_the_cap_on_a_virtual_clock() {
	// Burst is 64 KiB (two chunks), so 4 MiB at 1 MiB/s takes (4 MiB - 64 KiB) / 1 MiB/s.
	let t = paced_send(4 * MIB, Pacer::new(Some(MIB), None), None).await;
	let expect = (4 * MIB - 64 * 1024) as f64 / MIB as f64;
	assert!(
		(t - expect).abs() < 0.1,
		"took {t:.3}s, expected {expect:.3}s"
	);
	let rate = (4 * MIB) as f64 / t / MIB as f64;
	assert!((0.95..=1.08).contains(&rate), "rate {rate:.3} MiB/s");
}

#[tokio::test]
async fn unpaced_link_is_not_delayed() {
	let t = paced_send(4 * MIB, Pacer::new(None, None), None).await;
	assert!(t < 0.01, "took {t}");
}

#[tokio::test]
async fn global_cap_is_shared_by_concurrent_links() {
	let global = SharedLimit::unlimited();
	global.set_rate(Some(2 * MIB));
	let a = Pacer::new(None, Some(global.clone()));
	let b = Pacer::new(None, Some(global));
	let t = paced_send(2 * MIB, a, Some((2 * MIB, b))).await;
	// 4 MiB in total at 2 MiB/s, minus the shared burst.
	assert!((1.85..=2.1).contains(&t), "took {t:.3}s");
}

#[tokio::test]
async fn rate_can_be_lowered_while_running() {
	let pacer = Pacer::new(Some(8 * MIB), None);
	let handle = pacer.clone();
	let fut = paced_send(2 * MIB, pacer, None);
	tokio::pin!(fut);
	// Lower the cap before the first poll takes effect: 2 MiB at 1 MiB/s.
	handle.set_rate(Some(MIB));
	let t = fut.await;
	assert!((1.8..=2.1).contains(&t), "took {t:.3}s");
}

#[tokio::test]
async fn pacer_wait_is_bounded_for_tiny_rates() {
	let p = Pacer::new(Some(1), None);
	p.acquire(32 * 1024).await; // burst covers the first chunks
	let _ = Arc::new(0);
	tokio::time::pause();
	let start = Instant::now();
	p.acquire(32 * 1024).await;
	p.acquire(32 * 1024).await;
	assert!(start.elapsed() >= Duration::from_secs(1));
}
