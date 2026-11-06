//! Generates the `relay.v1` gRPC service from the checked-in proto contract.

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let protoc = protoc_bin_vendored::protoc_bin_path()?;
	let mut config = tonic_prost_build::Config::new();
	config.protoc_executable(protoc);
	// Messages carrying secrets get hand-written redacting Debug impls (src/redact.rs).
	config.skip_debug([
		".relay.v1.CreateChannelRequest",
		".relay.v1.JoinChannelRequest",
		".relay.v1.CreateInviteResponse",
		// Message text, file names, paths and image bytes are private to the call.
		".relay.v1.SendSessionMessageRequest",
		".relay.v1.SendSessionImageRequest",
		".relay.v1.OfferFileRequest",
		".relay.v1.RespondOfferRequest",
		".relay.v1.ExportSessionRequest",
		".relay.v1.ExportSessionResponse",
		".relay.v1.SearchSessionRequest",
		".relay.v1.TranscriptEntry",
		".relay.v1.FileOfferBody",
		".relay.v1.ImageBody",
	]);
	tonic_prost_build::configure()
		.build_client(true)
		.build_server(true)
		.compile_with_config(config, &["proto/relay/v1/engine.proto"], &["proto"])?;
	println!("cargo:rerun-if-changed=proto");
	embed_build_revision();
	Ok(())
}

/// Puts the git commit (and `-dirty` when the tree has changes) into `RELAY_BUILD_REVISION`;
/// without git it is the crate version plus `-dev`.
fn embed_build_revision() {
	use std::process::Command;
	let git = |args: &[&str]| {
		Command::new("git")
			.args(args)
			.output()
			.ok()
			.filter(|out| out.status.success())
			.map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
	};
	let revision = match git(&["rev-parse", "--short=12", "HEAD"]) {
		Some(commit) if !commit.is_empty() => {
			let dirty = git(&["status", "--porcelain", "--untracked-files=no"])
				.is_some_and(|status| !status.is_empty());
			if dirty {
				format!("{commit}-dirty")
			} else {
				commit
			}
		}
		_ => format!("{}-dev", env!("CARGO_PKG_VERSION")),
	};
	println!("cargo:rustc-env=RELAY_BUILD_REVISION={revision}");
	if let Some(head) = git(&["rev-parse", "--git-path", "HEAD"]) {
		println!("cargo:rerun-if-changed={head}");
	}
}
