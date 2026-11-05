//! Generates the `relay.peer.v1` prost types with a vendored `protoc`.

use std::path::PathBuf;

const PROTO: &str = "proto/relay/peer/v1/peer.proto";

fn main() -> Result<(), Box<dyn std::error::Error>> {
	println!("cargo:rerun-if-changed={PROTO}");

	let mut config = tonic_prost_build::Config::new();
	config.protoc_executable(protoc_bin_vendored::protoc_bin_path()?);

	let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);
	config.out_dir(out_dir);
	config.compile_protos(&[PROTO], &["proto"])?;
	Ok(())
}
