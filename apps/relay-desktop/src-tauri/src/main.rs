// Keep the console window out of release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
	relay_desktop_lib::run();
}
