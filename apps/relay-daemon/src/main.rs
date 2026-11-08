//! `relayd`: the Relay engine as its own per-user process (see the `relay_daemon` library).

fn main() {
	std::process::exit(relay_daemon::run(std::env::args_os()));
}
