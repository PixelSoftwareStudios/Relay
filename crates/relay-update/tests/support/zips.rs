//! Builders for update archives, honest and hostile.

use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;

pub const INFO_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleExecutable</key>
	<string>relay-desktop</string>
	<key>CFBundleIdentifier</key>
	<string>dev.relay.relay-desktop</string>
	<key>CFBundleShortVersionString</key>
	<string>NUMERIC</string>
</dict>
</plist>
"#;

pub fn file_options(mode: u32) -> SimpleFileOptions {
	SimpleFileOptions::default()
		.compression_method(zip::CompressionMethod::Deflated)
		.unix_permissions(mode)
}

/// A well-formed `Relay.app` update archive for `numeric` (major.minor.patch).
pub fn app_zip(numeric: &str, bundle_id: &str) -> Vec<u8> {
	let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
	writer
		.add_directory("Relay.app/", file_options(0o755))
		.unwrap();
	writer
		.add_directory("Relay.app/Contents/", file_options(0o755))
		.unwrap();
	writer
		.add_directory("Relay.app/Contents/MacOS/", file_options(0o755))
		.unwrap();
	let plist = INFO_PLIST
		.replace("NUMERIC", numeric)
		.replace("dev.relay.relay-desktop", bundle_id);
	writer
		.start_file("Relay.app/Contents/Info.plist", file_options(0o644))
		.unwrap();
	writer.write_all(plist.as_bytes()).unwrap();
	writer
		.start_file(
			"Relay.app/Contents/MacOS/relay-desktop",
			file_options(0o755),
		)
		.unwrap();
	writer
		.write_all(format!("#!/bin/sh\necho new {numeric}\n").as_bytes())
		.unwrap();
	writer.finish().unwrap().into_inner()
}

/// An archive of `(name, body, mode)` entries, in order, with raw names.
pub fn zip_of(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
	let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
	for (name, body, mode) in entries {
		writer.start_file(*name, file_options(*mode)).unwrap();
		writer.write_all(body).unwrap();
	}
	writer.finish().unwrap().into_inner()
}

pub fn zip_with_symlink() -> Vec<u8> {
	let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
	writer
		.start_file("Relay.app/a", file_options(0o644))
		.unwrap();
	writer.write_all(b"a").unwrap();
	writer
		.add_symlink("Relay.app/link", "/etc/passwd", file_options(0o777))
		.unwrap();
	writer.finish().unwrap().into_inner()
}
