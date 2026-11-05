use relay_audio::dsp::{
	BackendKind, BenchReport, DspError, Harness, HarnessConfig, ManifestError, ModelManifest,
	NoiseSuppressor, OffSuppressor, create_suppressor, measure_delay_samples,
};
use relay_audio::format::BlockTiming;

const CANDIDATE: &str = r#"{
	"schema_version": 1,
	"backend_id": "dtln-candidate",
	"status": "candidate",
	"source": {"url": "https://example.invalid/model", "revision": null},
	"model": {"variant": "baseline", "sha256": null},
	"runtime": {"name": "adapter-under-audit", "version": null, "threads": 1},
	"audio": {"sample_rate_hz": 16000, "hop_samples": 128, "analysis_window_samples": 512, "measured_total_delay_ms": null},
	"distribution": {"code_license": null, "model_license": null, "approved": false},
	"qualification": {"report": null, "reviewer": null}
}"#;

fn qualified() -> serde_json::Value {
	let mut v: serde_json::Value = serde_json::from_str(CANDIDATE).unwrap();
	v["status"] = "qualified".into();
	v["source"]["revision"] = "abc123".into();
	v["model"]["sha256"] = "0".repeat(64).into();
	v["runtime"]["version"] = "1.0".into();
	v["audio"]["measured_total_delay_ms"] = 12.5.into();
	v["distribution"] =
		serde_json::json!({"code_license": "MIT", "model_license": "MIT", "approved": true});
	v["qualification"] = serde_json::json!({"report": "r.md", "reviewer": "someone"});
	v
}

#[test]
fn candidate_manifest_is_valid_with_unknowns() {
	let m = ModelManifest::from_json(CANDIDATE).unwrap();
	assert_eq!(m.audio.hop_samples, Some(128));
	assert!(m.model.sha256.is_none());
}

#[test]
fn qualified_manifest_needs_complete_evidence() {
	ModelManifest::from_json(&qualified().to_string()).unwrap();
	let cases: [(&str, serde_json::Value); 6] = [
		("/source/revision", "".into()),
		("/model/sha256", serde_json::Value::Null),
		("/distribution/approved", false.into()),
		("/audio/measured_total_delay_ms", serde_json::Value::Null),
		("/qualification/reviewer", "".into()),
		("/runtime/version", serde_json::Value::Null),
	];
	for (pointer, value) in cases {
		let mut v = qualified();
		*v.pointer_mut(pointer).unwrap() = value;
		assert!(
			matches!(
				ModelManifest::from_json(&v.to_string()),
				Err(ManifestError::Invalid(_))
			),
			"{pointer} should be rejected"
		);
	}
}

#[test]
fn schema_shape_violations_are_rejected() {
	let mut v = qualified();
	v["model"]["sha256"] = "ABC".into();
	assert!(ModelManifest::from_json(&v.to_string()).is_err());
	let mut v = qualified();
	v["extra"] = 1.into();
	assert!(
		ModelManifest::from_json(&v.to_string()).is_err(),
		"unknown top-level key"
	);
	let mut v = qualified();
	v["runtime"]["threads"] = 0.into();
	assert!(ModelManifest::from_json(&v.to_string()).is_err());
	let mut v = qualified();
	v["audio"]["sample_rate_hz"] = 4000.into();
	assert!(ModelManifest::from_json(&v.to_string()).is_err());
	let mut v = qualified();
	v["schema_version"] = 2.into();
	assert!(matches!(
		ModelManifest::from_json(&v.to_string()),
		Err(ManifestError::SchemaVersion(2))
	));
	let mut v = qualified();
	v["audio"].as_object_mut().unwrap().remove("hop_samples");
	assert!(
		ModelManifest::from_json(&v.to_string()).is_err(),
		"required key missing"
	);
	let mut v = qualified();
	v["status"] = "maybe".into();
	assert!(ModelManifest::from_json(&v.to_string()).is_err());
}

#[test]
fn off_suppressor_is_exact_and_zero_delay() {
	let mut off = OffSuppressor::default();
	let input: Vec<f32> = (0..480).map(|i| i as f32 / 480.0).collect();
	let mut out = vec![0.0; 480];
	let p = off
		.process_into(
			&input,
			&mut out,
			BlockTiming {
				generation: 3,
				first_sample: 960,
			},
		)
		.unwrap();
	assert_eq!(
		(p.consumed, p.produced, p.first_output_index),
		(480, 480, 960)
	);
	assert_eq!(out, input);
	assert_eq!(off.descriptor().declared_delay_samples, Some(0));
	assert_eq!(measure_delay_samples(&mut off, 480, 4800).unwrap(), Some(0));
}

#[test]
fn neural_backends_are_stubbed_behind_clear_errors() {
	assert!(create_suppressor(BackendKind::Off, None).is_ok());
	for kind in [BackendKind::Rnnoise, BackendKind::Dtln] {
		let Err(DspError::BackendUnavailable { reason, .. }) = create_suppressor(kind, None) else {
			panic!("stub must refuse");
		};
		assert!(!reason.is_empty());
		let m = ModelManifest::from_json(CANDIDATE).unwrap();
		assert!(create_suppressor(kind, Some(&m)).is_err());
	}
}

#[test]
fn harness_records_real_timings_and_sample_accounting() {
	let harness = Harness::new(HarnessConfig {
		block_samples: 480,
		warmup_blocks: 5,
		measured_blocks: 50,
	});
	let mut off = OffSuppressor::default();
	let report = harness.run(&mut off, |i| (i % 100) as f32 / 100.0).unwrap();
	assert_eq!(report.status, "measured");
	let summary = report.process.as_ref().unwrap();
	assert_eq!(summary.samples, 50);
	assert!(summary.min_ns <= summary.p50_ns && summary.p50_ns <= summary.p99_ns);
	assert_eq!(report.samples_consumed, report.samples_produced);
	assert_eq!(report.samples_consumed, 55 * 480);
	// Nothing the harness cannot observe is filled in.
	assert!(report.callback_allocations.is_none());
	assert!(report.measured_delay_samples.is_none());
	let json: serde_json::Value = serde_json::from_str(&report.to_json()).unwrap();
	assert!(json["callback_allocations"].is_null());
}

#[test]
fn not_run_report_contains_no_numbers() {
	let r = BenchReport::not_run("off");
	let json: serde_json::Value = serde_json::from_str(&r.to_json()).unwrap();
	assert_eq!(json["status"], "not_run");
	assert!(json["process"].is_null() && json["prepare_ns"].is_null());
}
