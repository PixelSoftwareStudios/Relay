use relay_update::version::{from_tag, is_offerable, parse};

fn v(text: &str) -> semver::Version {
	parse(text).expect(text)
}

#[test]
fn newer_is_offered_equal_and_older_are_not() {
	assert!(is_offerable(&v("0.1.0"), &v("0.1.1")));
	assert!(is_offerable(&v("0.9.9"), &v("0.10.0")));
	assert!(is_offerable(&v("1.9.0"), &v("2.0.0")));
	assert!(!is_offerable(&v("0.1.1"), &v("0.1.1")));
	assert!(!is_offerable(&v("0.2.0"), &v("0.1.9")));
}

#[test]
fn build_metadata_never_decides() {
	assert!(!is_offerable(&v("0.1.0+a"), &v("0.1.0+b")));
}

#[test]
fn prerelease_only_for_prerelease_builds() {
	assert!(!is_offerable(&v("0.1.0"), &v("0.2.0-rc.1")));
	assert!(is_offerable(&v("0.2.0-rc.1"), &v("0.2.0-rc.2")));
	assert!(is_offerable(&v("0.2.0-rc.1"), &v("0.2.0")));
	assert!(is_offerable(&v("0.1.0-beta.1"), &v("0.2.0-rc.1")));
}

#[test]
fn prerelease_orders_below_its_release() {
	assert!(!is_offerable(&v("0.2.0"), &v("0.2.0-rc.9")));
	assert!(!is_offerable(&v("0.2.0-rc.2"), &v("0.2.0-rc.1")));
}

#[test]
fn tags_are_v_semver_only() {
	assert_eq!(from_tag("v1.2.3").unwrap(), v("1.2.3"));
	for bad in [
		"1.2.3", "V1.2.3", "v1.2", "v01.2.3", "vv1.2.3", "v1.2.3 ", "latest", "",
	] {
		assert!(from_tag(bad).is_err(), "{bad:?}");
	}
}

#[test]
fn the_running_version_must_be_semver() {
	let platform = relay_update::Platform {
		os: relay_update::Os::Other,
		arch: relay_update::Arch::Other,
		install: relay_update::InstallKind::Manual(relay_update::ManualReason::DevBuild),
	};
	assert!(relay_update::Updater::new("0.1", platform.clone()).is_err());
	assert!(relay_update::Updater::new("v0.1.0", platform.clone()).is_err());
	assert!(relay_update::Updater::new("0.1.0", platform).is_ok());
}
