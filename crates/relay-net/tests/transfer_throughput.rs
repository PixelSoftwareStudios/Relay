mod common;
mod transfer_support;

use std::time::{Duration, Instant};

use relay_messages::transfer::Verdict;
use transfer_support::*;

/// Loopback smoke measurement, not a Wi-Fi claim: unpaced, 16 MiB, through the full stack
/// (Noise records, framing, TCP). Run with `--release -- --nocapture` for a meaningful figure.
#[tokio::test]
async fn loopback_throughput_smoke() {
	let (a, b) = (peer(config(None)).await, peer(config(None)).await);
	let size = 16 * 1024 * 1024;
	let (rx_link, tx_link) = connect(&a, &b, size as u64).await;
	let data = pattern(size);
	let started = Instant::now();
	let (got, verdict) = within(
		Duration::from_secs(120),
		pump(data, (tx_link.tx, tx_link.rx), (rx_link.tx, rx_link.rx)),
	)
	.await;
	let secs = started.elapsed().as_secs_f64();
	assert_eq!(got.len(), size);
	assert_eq!(verdict, Verdict::Ok);
	println!(
		"loopback throughput: {:.0} MiB/s ({secs:.2}s for 16 MiB)",
		16.0 / secs
	);
}
