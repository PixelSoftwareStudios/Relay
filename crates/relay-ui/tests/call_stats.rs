use relay_api::pb;
use relay_ui::convert::call_stats;
use relay_ui::model::MediaPath;

#[test]
fn a_sample_converts_and_drops_readings_that_are_not_finite() {
	let sample = pb::CallStats {
		call_id: vec![0xab; 16],
		path_state: pb::MediaPathState::Connected as i32,
		sending: true,
		rtt_ms: Some(41.0),
		jitter_ms: Some(f64::NAN),
		mouth_to_ear_ms: Some(f64::INFINITY),
		loss_percent_5s: f64::NAN,
		transport: "overlay".into(),
		..pb::CallStats::default()
	};
	let stats = call_stats(&sample);
	assert_eq!(stats.call_id, "ab".repeat(16));
	assert_eq!(stats.path, MediaPath::Connected);
	assert!(stats.sending && !stats.receiving);
	assert_eq!(stats.rtt_ms, Some(41.0));
	assert_eq!(stats.jitter_ms, None);
	assert_eq!(stats.mouth_to_ear_ms, None);
	assert_eq!(stats.loss_percent_5s, 0.0);
	assert_eq!(stats.transport, "overlay");
}

#[test]
fn an_unknown_path_state_reads_as_waiting() {
	let stats = call_stats(&pb::CallStats::default());
	assert_eq!(stats.path, MediaPath::Waiting);
}
