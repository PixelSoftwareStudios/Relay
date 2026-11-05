//! Support bundle and raw-capture gating.

mod bundle;
mod capture;
mod redact;

pub use bundle::{BUNDLE_MAX_BYTES, BundleInput, build_bundle};
pub use capture::RawCaptureConsent;
pub use redact::{database_location, fingerprint, redact_line};
