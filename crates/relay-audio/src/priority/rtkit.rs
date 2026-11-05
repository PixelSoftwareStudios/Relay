//! RealtimeKit (`org.freedesktop.RealtimeKit1`) on the system bus, behind the `rtkit` feature.
//!
//! One blocking D-Bus round trip on the worker thread before its loop starts, never from a
//! callback. The service requires `RLIMIT_RTTIME` to be finite for the caller, so it is set to
//! 200 ms of uninterrupted real-time CPU if unlimited (the kernel then signals the process that
//! overruns it, which is the safety net RealtimeKit relies on). See the RealtimeKit D-Bus API.

use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};
use zbus::blocking::Connection;

const DEST: &str = "org.freedesktop.RealtimeKit1";
const PATH: &str = "/org/freedesktop/RealtimeKit1";
const RTTIME_US: u64 = 200_000;

fn property(conn: &Connection, name: &str) -> Result<i32, zbus::Error> {
	let reply = conn.call_method(
		Some(DEST),
		PATH,
		Some("org.freedesktop.DBus.Properties"),
		"Get",
		&(DEST, name),
	)?;
	let value: zbus::zvariant::OwnedValue = reply.body().deserialize()?;
	i32::try_from(value).map_err(Into::into)
}

/// Asks RealtimeKit to make thread `tid` real-time at a bounded priority.
pub fn make_realtime(tid: u64) -> Result<(), String> {
	let limit = getrlimit(Resource::Rttime);
	if limit.current.is_none() {
		let capped = Rlimit {
			current: Some(RTTIME_US),
			maximum: limit.maximum,
		};
		setrlimit(Resource::Rttime, capped).map_err(|e| format!("RLIMIT_RTTIME: {e}"))?;
	}
	let conn = Connection::system().map_err(|e| e.to_string())?;
	let max = property(&conn, "MaxRealtimePriority").map_err(|e| e.to_string())?;
	let wanted = u32::try_from(max)
		.unwrap_or(1)
		.clamp(1, super::policy::MAX_RT_PRIORITY);
	conn.call_method(
		Some(DEST),
		PATH,
		Some(DEST),
		"MakeThreadRealtime",
		&(tid, wanted),
	)
	.map(|_| ())
	.map_err(|e| e.to_string())
}
