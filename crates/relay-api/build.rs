//! Generates the `relay.v1` gRPC service from the checked-in proto contract.

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let protoc = protoc_bin_vendored::protoc_bin_path()?;
	let mut config = tonic_prost_build::Config::new();
	config.protoc_executable(protoc);
	tonic_prost_build::configure()
		.build_client(true)
		.build_server(true)
		.compile_with_config(config, &["proto/relay/v1/engine.proto"], &["proto"])?;
	println!("cargo:rerun-if-changed=proto");
	Ok(())
}
