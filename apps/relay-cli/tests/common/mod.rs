//! Test doubles: a scripted `Control` and an in-process engine over a real socket.

#![allow(dead_code, unused_imports)]

pub mod chat;
pub mod chat_watch;
pub mod control;
#[cfg(unix)]
pub mod engine;
pub mod fixtures;

pub use control::FakeControl;

/// Run a command line against a `Control` and return its stdout.
pub async fn run_with<
	C: relay_cli::client::Control + relay_cli::client::Subscribe + relay_cli::client::Manage,
>(
	client: &mut C,
	args: &[&str],
	json: bool,
) -> Result<String, relay_cli::cli::CliError> {
	use clap::Parser;
	let mut argv = vec!["relay"];
	if json {
		argv.push("--json");
	}
	argv.extend_from_slice(args);
	let cli = relay_cli::cli::Cli::try_parse_from(argv).expect("valid arguments");
	let mut buffer = Vec::new();
	let mut out = relay_cli::commands::Out::new(json, &mut buffer);
	relay_cli::commands::execute(client, cli.command, &mut out).await?;
	Ok(String::from_utf8(buffer).expect("utf8 output"))
}

/// Secrets handed out in order, as if typed or piped; records what was asked.
#[derive(Default)]
pub struct ScriptedSecrets {
	pub answers: std::collections::VecDeque<String>,
	pub asked: Vec<relay_cli::secret::SecretInput>,
}

impl ScriptedSecrets {
	pub fn new(answers: &[&str]) -> Self {
		Self {
			answers: answers.iter().map(|a| (*a).to_owned()).collect(),
			asked: Vec::new(),
		}
	}
}

impl relay_cli::secret::SecretSource for ScriptedSecrets {
	fn read(
		&mut self,
		input: relay_cli::secret::SecretInput,
	) -> Result<relay_cli::secret::Secret, relay_cli::cli::CliError> {
		self.asked.push(input);
		self.answers
			.pop_front()
			.map(relay_cli::secret::Secret::new)
			.ok_or_else(|| relay_cli::cli::CliError::Usage("no secret was entered".into()))
	}
}

/// A clipboard that remembers, or refuses.
#[derive(Default)]
pub struct MemoryClipboard {
	pub text: Option<String>,
	pub fail: bool,
}

impl relay_cli::commands::clipboard::Clipboard for MemoryClipboard {
	fn copy(&mut self, text: &str) -> Result<(), String> {
		if self.fail {
			return Err("no clipboard in this session".into());
		}
		self.text = Some(text.to_owned());
		Ok(())
	}
}

/// Like [`run_with`] with scripted secrets and clipboard.
pub async fn run_scripted<
	C: relay_cli::client::Control + relay_cli::client::Subscribe + relay_cli::client::Manage,
>(
	client: &mut C,
	args: &[&str],
	secrets: &mut ScriptedSecrets,
	clipboard: &mut MemoryClipboard,
) -> Result<String, relay_cli::cli::CliError> {
	use clap::Parser;
	let mut argv = vec!["relay"];
	argv.extend_from_slice(args);
	let cli = relay_cli::cli::Cli::try_parse_from(argv).expect("valid arguments");
	let mut buffer = Vec::new();
	let mut out = relay_cli::commands::Out::new(false, &mut buffer)
		.with_secrets(secrets)
		.with_clipboard(clipboard);
	relay_cli::commands::execute(client, cli.command, &mut out).await?;
	Ok(String::from_utf8(buffer).expect("utf8 output"))
}

/// What a `relay messages` run produced, kept even when it failed.
pub struct Ran {
	pub out: String,
	pub clipboard: Option<String>,
	pub result: Result<(), relay_cli::cli::CliError>,
}

impl Ran {
	/// The output of a run that must have succeeded.
	pub fn ok(self) -> String {
		self.result.expect("command succeeds");
		self.out
	}

	/// The error of a run that must have failed.
	pub fn err(self) -> relay_cli::cli::CliError {
		self.result.expect_err("command fails")
	}
}

/// Run `relay messages ...` against a `Messaging` client.
/// The zone every test renders message times in: UTC+02:00, whatever the host uses.
pub fn zone() -> relay_io::localtime::LocalZone {
	relay_io::localtime::LocalZone::fixed(2 * 3600).expect("valid offset")
}

pub async fn run_messages<M: relay_cli::client::Messaging>(
	client: &mut M,
	args: &[&str],
	json: bool,
) -> Ran {
	run_messages_with(client, args, json, "").await
}

/// Like [`run_messages`] with scripted stdin.
pub async fn run_messages_with<M: relay_cli::client::Messaging>(
	client: &mut M,
	args: &[&str],
	json: bool,
	stdin: &str,
) -> Ran {
	use clap::Parser;
	let mut argv = vec!["relay", "messages"];
	if json {
		argv.insert(1, "--json");
	}
	argv.extend_from_slice(args);
	let cli = relay_cli::cli::Cli::try_parse_from(argv).expect("valid arguments");
	let relay_cli::cli::Command::Messages(parsed) = cli.command else {
		panic!("not a messages command");
	};
	let mut buffer = Vec::new();
	let mut input = std::io::Cursor::new(stdin.as_bytes().to_vec());
	let mut clipboard = MemoryClipboard::default();
	let result = {
		let mut out = relay_cli::commands::Out::new(json, &mut buffer)
			.with_zone(zone())
			.with_input(&mut input)
			.with_clipboard(&mut clipboard);
		relay_cli::commands::messages::run(client, parsed, &mut out).await
	};
	Ran {
		out: String::from_utf8(buffer).expect("utf8 output"),
		clipboard: clipboard.text,
		result,
	}
}
