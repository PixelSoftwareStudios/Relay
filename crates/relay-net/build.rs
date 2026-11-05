//! Generates the `relay.peer.v1` prost types with a vendored `protoc`.

use std::path::PathBuf;

const PROTOS: [&str; 2] = [
	"proto/relay/peer/v1/peer.proto",
	"proto/relay/peer/v1/channel.proto",
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
	for proto in PROTOS {
		println!("cargo:rerun-if-changed={proto}");
	}

	let mut config = tonic_prost_build::Config::new();
	config.protoc_executable(protoc_bin_vendored::protoc_bin_path()?);

	let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);
	config.out_dir(out_dir);
	config.compile_protos(&PROTOS, &["proto"])?;
	Ok(())
}
