//! `relay completions` and `relay man` print non-empty output naming the subcommands.

use relay_cli::cli::exit;

#[tokio::test]
async fn every_shell_gets_a_script_that_knows_the_subcommands() {
	for shell in ["bash", "zsh", "fish", "elvish", "powershell"] {
		let script = relay_cli_run(&["completions", shell]).await.unwrap();
		assert!(!script.trim().is_empty(), "{shell}");
		for word in ["trust", "avatar", "config", "completions", "man"] {
			assert!(script.contains(word), "{shell} script lacks {word}");
		}
	}
	let err = relay_cli_run(&["completions", "tcsh"]).await;
	assert!(err.is_err(), "unknown shell is a usage error");
}

#[tokio::test]
async fn the_man_page_goes_to_stdout_or_a_directory() {
	let page = relay_cli_run(&["man"]).await.unwrap();
	assert!(
		page.contains(".TH relay"),
		"{}",
		&page[..page.len().min(200)]
	);
	for word in ["trust", "avatar", "config"] {
		assert!(page.contains(word), "main page lacks {word}");
	}

	let dir = tempfile::tempdir().unwrap();
	let target = dir.path().join("man");
	let said = relay_cli_run(&["man", "--dir", target.to_str().unwrap()])
		.await
		.unwrap();
	assert!(said.contains("wrote man pages"), "{said}");
	let trust = std::fs::read_to_string(target.join("relay-trust.1")).unwrap();
	assert!(trust.contains("forget"), "{trust}");
	assert!(target.join("relay.1").is_file());
}

/// The binary-level parse, so clap errors map to the usage exit code.
async fn relay_cli_run(args: &[&str]) -> Result<String, relay_cli::cli::CliError> {
	use clap::Parser;
	let mut argv = vec!["relay"];
	argv.extend_from_slice(args);
	let cli = relay_cli::cli::Cli::try_parse_from(argv)
		.map_err(|e| relay_cli::cli::CliError::Usage(e.to_string()))?;
	let mut buffer = Vec::new();
	relay_cli::commands::run(cli, &mut buffer).await?;
	Ok(String::from_utf8(buffer).unwrap())
}

#[tokio::test]
async fn bad_arguments_are_usage_errors() {
	let err = relay_cli_run(&["completions"]).await.unwrap_err();
	assert_eq!(err.exit_code(), exit::USAGE);
}
