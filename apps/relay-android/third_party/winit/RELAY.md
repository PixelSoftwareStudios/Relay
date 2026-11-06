# winit 0.30.13 with one Android patch

Copied from crates.io. Only `src/platform_impl/android/mod.rs` differs (the
lines marked "Relay patch"): GameActivity reports soft-keyboard typing as
`InputEvent::TextEvent` (the edit buffer), which winit 0.30 drops with a
warning, so no text typed on the on-screen keyboard reached egui. The patch
turns each change of the buffer into key events (characters with their text,
backspaces) and clears the buffer when a text field asks for the keyboard.
Remove this directory and the `[patch.crates-io]` entry when winit 0.31 (which
has IME support on Android) is adopted by eframe.
