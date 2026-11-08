//! `relay` binary entry point; all logic lives in the library.

fn main() -> std::process::ExitCode {
	// A daemon started by this binary (`relay daemon start`, autostart) runs here, first.
	relay_daemon::exit_if_requested();
	relay_cli::main_exit()
}
