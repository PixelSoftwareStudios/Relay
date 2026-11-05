//! The webview may call exactly the commands the build script registers.

use std::fs;

fn manifest_commands() -> Vec<String> {
	let build = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/build.rs")).unwrap();
	let list = build.split("const COMMANDS").nth(1).unwrap();
	let list = list.split("];").next().unwrap();
	list.split('"')
		.skip(1)
		.step_by(2)
		.map(str::to_owned)
		.collect()
}

#[test]
fn capability_grants_exactly_the_registered_commands() {
	let raw = fs::read_to_string(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/capabilities/main.json"
	))
	.unwrap();
	let cap: serde_json::Value = serde_json::from_str(&raw).unwrap();
	let mut granted: Vec<String> = cap["permissions"]
		.as_array()
		.unwrap()
		.iter()
		.filter_map(|p| {
			p.as_str()?
				.strip_prefix("allow-")
				.map(|c| c.replace('-', "_"))
		})
		.collect();
	let mut registered = manifest_commands();
	granted.sort();
	registered.sort();
	assert_eq!(granted, registered);
	assert_eq!(registered.len(), 14);
}

#[test]
fn no_filesystem_shell_or_network_plugins_are_granted() {
	let raw = fs::read_to_string(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/capabilities/main.json"
	))
	.unwrap();
	for forbidden in ["fs:", "shell:", "http:", "opener:", "core:default"] {
		assert!(!raw.contains(forbidden), "{forbidden} must not be granted");
	}
}

#[test]
fn global_tauri_object_is_not_exposed_to_pages() {
	let raw = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tauri.conf.json")).unwrap();
	let conf: serde_json::Value = serde_json::from_str(&raw).unwrap();
	assert_eq!(conf["app"]["withGlobalTauri"], false);
	assert!(
		conf["app"]["security"]["csp"]
			.as_str()
			.unwrap()
			.contains("default-src 'self'")
	);
}
