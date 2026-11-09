//! Detached helper processes that wait for this process to exit and then start the new version.
//! Untrusted values (paths) are passed as separate arguments or environment variables to a fixed
//! script, never spliced into script text.

#[cfg(unix)]
use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::error::{Error, Result};

/// Environment the AppImage runtime and its `AppRun` add for the running image. The relaunched
/// image must not inherit them, or it would believe it runs from the old mount.
pub const APPIMAGE_ENV: [&str; 4] = ["APPIMAGE", "APPDIR", "ARGV0", "OWD"];

/// Waits (at most 5 minutes) until `$1` is gone, then replaces itself with `$2`.
pub const SH_EXEC: &str = r#"n=0
while kill -0 "$1" 2>/dev/null; do
	n=$((n+1))
	[ "$n" -gt 1500 ] && exit 1
	sleep 0.2
done
exec "$2""#;

/// Waits for `$1`, opens a new instance of the bundle `$2`, and only if that worked removes the
/// old bundle `$3` that the swap moved aside.
pub const SH_OPEN: &str = r#"n=0
while kill -0 "$1" 2>/dev/null; do
	n=$((n+1))
	[ "$n" -gt 1500 ] && exit 1
	sleep 0.2
done
/usr/bin/open -n "$2" && rm -rf "$3""#;

#[cfg(unix)]
pub fn spawn_sh(script: &'static str, args: &[&OsStr], scrub_env: &[&str]) -> Result<()> {
	use std::os::unix::process::CommandExt;
	let mut command = Command::new("/bin/sh");
	command
		.arg("-c")
		.arg(script)
		.arg("relay-update-helper")
		.args(args)
		.stdin(Stdio::null())
		.stdout(Stdio::null())
		.stderr(Stdio::null())
		// Own process group: survives the parent and its terminal.
		.process_group(0);
	for name in scrub_env {
		command.env_remove(name);
	}
	command
		.spawn()
		.map(|_| ())
		.map_err(|e| Error::Helper(e.to_string()))
}

#[cfg(unix)]
pub fn relaunch_exec(pid: u32, program: &Path) -> Result<()> {
	spawn_sh(
		SH_EXEC,
		&[pid.to_string().as_ref(), program.as_os_str()],
		&APPIMAGE_ENV,
	)
}

#[cfg(unix)]
pub fn relaunch_open(pid: u32, bundle: &Path, old: &Path) -> Result<()> {
	spawn_sh(
		SH_OPEN,
		&[
			pid.to_string().as_ref(),
			bundle.as_os_str(),
			old.as_os_str(),
		],
		&[],
	)
}

/// Fixed PowerShell program; inputs arrive in `RELAY_UPDATE_*` variables of the child only.
/// UNVERIFIED on real Windows.
pub const POWERSHELL: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
try { (Get-Process -Id ([int]$env:RELAY_UPDATE_PID) -ErrorAction Stop).WaitForExit(300000) | Out-Null } catch {}
if ($env:RELAY_UPDATE_MSI) {
  $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $env:RELAY_UPDATE_MSI).Hash
  if ($hash -eq $env:RELAY_UPDATE_SHA256) {
    $msiexec = Join-Path $env:SystemRoot 'System32\msiexec.exe'
    $arguments = @('/i', ('"' + $env:RELAY_UPDATE_MSI + '"'), '/passive', '/norestart')
    Start-Process -FilePath $msiexec -ArgumentList $arguments -Verb RunAs -Wait | Out-Null
  }
}
Start-Process -FilePath $env:RELAY_UPDATE_LAUNCH
if ($env:RELAY_UPDATE_OLD) {
  foreach ($path in $env:RELAY_UPDATE_OLD.Split('|')) { Remove-Item -LiteralPath $path -Force }
}
if ($env:RELAY_UPDATE_CLEANUP) { Remove-Item -LiteralPath $env:RELAY_UPDATE_CLEANUP -Recurse -Force }
"#;

/// `-EncodedCommand` wants base64 of UTF-16LE. Unlike `-File` it is not blocked by the default
/// "Restricted" execution policy.
pub fn encoded_command(script: &str) -> String {
	const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
	let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
	let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
	for chunk in bytes.chunks(3) {
		let n = chunk
			.iter()
			.enumerate()
			.fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
		for i in 0..4 {
			if i <= chunk.len() {
				out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
			} else {
				out.push('=');
			}
		}
	}
	out
}

#[cfg(windows)]
pub fn powershell_relaunch(
	pid: u32,
	launch: &Path,
	msi: Option<(&Path, &str)>,
	old_files: &[&Path],
	cleanup: Option<&Path>,
) -> Result<()> {
	use std::os::windows::process::CommandExt;
	const DETACHED_PROCESS: u32 = 0x0000_0008;
	const CREATE_NO_WINDOW: u32 = 0x0800_0000;
	let system_root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
	let exe = Path::new(&system_root).join("System32\\WindowsPowerShell\\v1.0\\powershell.exe");
	let joined: Vec<String> = old_files
		.iter()
		.map(|p| p.to_string_lossy().into_owned())
		.collect();
	let mut command = Command::new(exe);
	command
		.args([
			"-NoProfile",
			"-NonInteractive",
			"-WindowStyle",
			"Hidden",
			"-EncodedCommand",
		])
		.arg(encoded_command(POWERSHELL))
		.env("RELAY_UPDATE_PID", pid.to_string())
		.env("RELAY_UPDATE_LAUNCH", launch)
		.env("RELAY_UPDATE_OLD", joined.join("|"))
		.stdin(Stdio::null())
		.stdout(Stdio::null())
		.stderr(Stdio::null())
		.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW);
	if let Some((path, sha256)) = msi {
		command
			.env("RELAY_UPDATE_MSI", path)
			.env("RELAY_UPDATE_SHA256", sha256.to_uppercase());
	}
	if let Some(path) = cleanup {
		command.env("RELAY_UPDATE_CLEANUP", path);
	}
	command
		.spawn()
		.map(|_| ())
		.map_err(|e| Error::Helper(e.to_string()))
}
