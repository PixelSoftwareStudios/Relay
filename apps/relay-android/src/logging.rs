//! Logs go to logcat under the tag `relay`: `adb logcat -s relay`.

#[cfg(target_os = "android")]
pub fn init() {
	use std::sync::Once;
	use tracing_subscriber::layer::SubscriberExt;
	use tracing_subscriber::util::SubscriberInitExt;

	static INIT: Once = Once::new();
	INIT.call_once(|| {
		if let Ok(layer) = tracing_android::layer("relay") {
			let _ = tracing_subscriber::registry()
				.with(tracing_subscriber::filter::LevelFilter::INFO)
				.with(layer)
				.try_init();
		}
	});
}

/// Host builds (tests) log nowhere.
#[cfg(not(target_os = "android"))]
pub fn init() {}
