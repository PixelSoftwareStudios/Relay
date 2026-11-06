//! Embeds `resources/Info.plist` in the macOS binary, so a build run outside an
//! app bundle still carries `NSMicrophoneUsageDescription`. A packaged `.app`
//! has its own Info.plist with the same key; see docs/gates.md.

fn main() {
	println!("cargo:rerun-if-changed=resources/Info.plist");
	if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
		let dir = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets the manifest dir");
		println!(
			"cargo:rustc-link-arg-bins=-Wl,-sectcreate,__TEXT,__info_plist,{dir}/resources/Info.plist"
		);
	}
}
