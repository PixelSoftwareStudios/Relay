use std::ffi::OsString;
use std::path::PathBuf;

use relay_update::{Arch, DetectInput, InstallKind, ManualReason, Os, Platform};

mod support;

fn input(os: Os, arch: Arch, exe: &str) -> DetectInput {
	DetectInput {
		os,
		arch,
		exe: Some(PathBuf::from(exe)),
		appimage: None,
		program_files: Vec::new(),
		debug_build: false,
	}
}

fn version(text: &str) -> semver::Version {
	relay_update::version::parse(text).unwrap()
}

#[test]
fn macos_bundle_is_found_from_the_executable() {
	let p = Platform::from_input(&input(
		Os::Macos,
		Arch::Arm64,
		"/Applications/Relay.app/Contents/MacOS/relay-desktop",
	));
	assert_eq!(
		p.install,
		InstallKind::MacosBundle {
			bundle: "/Applications/Relay.app".into()
		}
	);
	assert_eq!(
		p.asset_name(&version("0.1.1")).unwrap(),
		"relay-0.1.1-macos-arm64-update.zip"
	);
	let p = Platform::from_input(&input(
		Os::Macos,
		Arch::X64,
		"/Users/a/My Relay.app/Contents/MacOS/relay-desktop",
	));
	assert_eq!(
		p.asset_name(&version("1.0.0-rc.1")).unwrap(),
		"relay-1.0.0-rc.1-macos-x64-update.zip"
	);
}

#[test]
fn macos_outside_a_bundle_is_manual() {
	for exe in [
		"/usr/local/bin/relay-desktop",
		"/Applications/Relay.app/Contents/relay-desktop",
		"/x/Relay/Contents/MacOS/relay-desktop",
	] {
		let p = Platform::from_input(&input(Os::Macos, Arch::Arm64, exe));
		assert_eq!(
			p.install,
			InstallKind::Manual(ManualReason::Packaged),
			"{exe}"
		);
		assert!(p.asset_name(&version("0.1.1")).is_none());
	}
}

#[test]
fn windows_msi_is_the_per_machine_install_folder() {
	let mut i = input(
		Os::Windows,
		Arch::X64,
		"C:/Program Files/Relay/relay-desktop.exe",
	);
	i.program_files = vec!["C:/Program Files".into()];
	let p = Platform::from_input(&i);
	assert_eq!(
		p.install,
		InstallKind::WindowsMsi {
			dir: "C:/Program Files/Relay".into()
		}
	);
	assert_eq!(
		p.asset_name(&version("0.1.1")).unwrap(),
		"relay-0.1.1-windows-x64.msi"
	);
	// Case and separators do not matter on Windows.
	i.exe = Some("c:/program files/RELAY/relay-desktop.exe".into());
	assert!(matches!(
		Platform::from_input(&i).install,
		InstallKind::WindowsMsi { .. }
	));
}

#[test]
fn windows_elsewhere_is_portable() {
	let mut i = input(
		Os::Windows,
		Arch::Arm64,
		"D:/tools/relay-0.1.0-windows-arm64/bin/relay-desktop.exe",
	);
	i.program_files = vec!["C:/Program Files".into()];
	let p = Platform::from_input(&i);
	assert!(matches!(p.install, InstallKind::WindowsPortable { .. }));
	assert_eq!(
		p.asset_name(&version("0.1.1")).unwrap(),
		"relay-0.1.1-windows-arm64-portable.zip"
	);
	// A sibling folder that merely starts with the same text is not the MSI folder.
	i.exe = Some("C:/Program Files/Relay2/relay-desktop.exe".into());
	assert!(matches!(
		Platform::from_input(&i).install,
		InstallKind::WindowsPortable { .. }
	));
}

#[test]
fn appimage_needs_an_existing_absolute_file() {
	let dir = support::tempdir();
	let image = support::write(dir.path().join("Relay.AppImage"), b"x");
	let mut i = input(
		Os::Linux,
		Arch::X64,
		"/tmp/.mount_RelayAb/usr/bin/relay-desktop",
	);
	i.appimage = Some(image.clone().into_os_string());
	let p = Platform::from_input(&i);
	assert_eq!(p.install, InstallKind::LinuxAppImage { path: image });
	assert_eq!(
		p.asset_name(&version("0.1.1")).unwrap(),
		"relay-0.1.1-linux-x64.AppImage"
	);
	for bad in [
		OsString::new(),
		"relative/Relay.AppImage".into(),
		dir.path().join("missing").into_os_string(),
		dir.path().into(),
	] {
		i.appimage = Some(bad.clone());
		assert!(
			matches!(Platform::from_input(&i).install, InstallKind::Manual(_)),
			"{bad:?}"
		);
	}
	i.appimage = None;
	assert_eq!(
		Platform::from_input(&i).install,
		InstallKind::Manual(ManualReason::Packaged)
	);
}

#[test]
fn packaged_and_dev_builds_never_self_replace() {
	// deb/rpm/tarball: /usr/bin without $APPIMAGE.
	let p = Platform::from_input(&input(Os::Linux, Arch::X64, "/usr/bin/relay-desktop"));
	assert_eq!(p.install, InstallKind::Manual(ManualReason::Packaged));
	// A debug build, even inside a bundle.
	let mut i = input(
		Os::Macos,
		Arch::Arm64,
		"/Applications/Relay.app/Contents/MacOS/relay-desktop",
	);
	i.debug_build = true;
	assert_eq!(
		Platform::from_input(&i).install,
		InstallKind::Manual(ManualReason::DevBuild)
	);
}

#[test]
fn a_cargo_target_directory_is_a_dev_build() {
	let dir = support::tempdir();
	let release = dir.path().join("target/release");
	std::fs::create_dir_all(&release).unwrap();
	support::write(
		dir.path().join("target/CACHEDIR.TAG"),
		b"Signature: 8a477f597d28d172789f06886806bc55",
	);
	let exe = release.join("relay-desktop");
	let mut i = input(Os::Linux, Arch::X64, exe.to_str().unwrap());
	i.appimage = Some(support::write(dir.path().join("a.AppImage"), b"x").into_os_string());
	assert_eq!(
		Platform::from_input(&i).install,
		InstallKind::Manual(ManualReason::DevBuild)
	);
}

#[test]
fn unsupported_architecture_or_os_is_manual() {
	let p = Platform::from_input(&input(Os::Linux, Arch::Other, "/usr/bin/relay-desktop"));
	assert!(matches!(p.install, InstallKind::Manual(_)));
	assert!(p.id().is_none());
	let p = Platform::from_input(&input(Os::Other, Arch::X64, "/x/relay"));
	assert_eq!(
		p.install,
		InstallKind::Manual(ManualReason::UnsupportedPlatform)
	);
}

#[test]
fn every_platform_id_matches_the_packager() {
	let ids = [
		(Os::Windows, Arch::X64, "windows-x64"),
		(Os::Windows, Arch::Arm64, "windows-arm64"),
		(Os::Macos, Arch::Arm64, "macos-arm64"),
		(Os::Macos, Arch::X64, "macos-x64"),
		(Os::Linux, Arch::X64, "linux-x64"),
		(Os::Linux, Arch::Arm64, "linux-arm64"),
	];
	for (os, arch, id) in ids {
		let p = Platform {
			os,
			arch,
			install: InstallKind::Manual(ManualReason::Packaged),
		};
		assert_eq!(p.id(), Some(id));
	}
}
