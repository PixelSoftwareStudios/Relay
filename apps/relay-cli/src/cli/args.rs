//! clap definitions for the `relay` command line.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Low-latency LAN voice calling.
#[derive(Debug, Parser)]
#[command(name = "relay", version, about, propagate_version = true)]
pub struct Cli {
	/// Options shared by every command.
	#[command(flatten)]
	pub global: GlobalArgs,
	/// What to do.
	#[command(subcommand)]
	pub command: Command,
}

/// Options accepted before or after any subcommand.
#[derive(Debug, Clone, Args)]
pub struct GlobalArgs {
	/// Print machine-readable JSON instead of text.
	#[arg(long, global = true)]
	pub json: bool,
	/// Engine Unix socket path (default: `$RELAY_SOCKET`, else the per-user runtime socket).
	#[arg(long, global = true, value_name = "PATH")]
	pub socket: Option<PathBuf>,
	/// Engine loopback TCP profile file, instead of a Unix socket (or `$RELAY_TCP_PROFILE`).
	#[arg(long, global = true, value_name = "PATH", conflicts_with = "socket")]
	pub tcp_profile: Option<PathBuf>,
	/// Take call control from another client when it holds it.
	#[arg(long, global = true)]
	pub takeover: bool,
	/// How long to wait for the engine to finish an operation.
	#[arg(long, global = true, default_value_t = 5000, value_name = "MS")]
	pub timeout_ms: u64,
}

/// Top-level commands.
#[derive(Debug, Subcommand)]
pub enum Command {
	/// Run the engine in this terminal.
	Daemon(DaemonArgs),
	/// Show engine, audio and call status.
	Status,
	/// List peers discovered on the LAN.
	Peers,
	/// Call a peer by name, id prefix or `ip:port`.
	Call {
		/// Peer name, id prefix, or manual address.
		peer: String,
	},
	/// Accept an incoming call.
	Accept(CallTarget),
	/// Decline an incoming call.
	Decline(CallTarget),
	/// End a call.
	End(CallTarget),
	/// Mute the microphone, globally or for one call.
	Mute(SwitchArgs),
	/// Deafen playback, globally or for one call.
	Deafen(SwitchArgs),
	/// Set a gain (linear multiplier).
	Gain(GainArgs),
	/// Choose how a call transmits (needs explicit consent to send audio).
	Talk(TalkArgs),
	/// List or select audio devices.
	#[command(subcommand)]
	Devices(DevicesCommand),
	/// Noise suppression.
	#[command(subcommand)]
	Ns(NsCommand),
	/// Your profile as peers see it.
	#[command(subcommand)]
	Profile(ProfileCommand),
	/// Peer trust decisions.
	#[command(subcommand)]
	Trust(TrustCommand),
	/// Measure the microphone level for a few seconds.
	MicTest(MicTestArgs),
	/// Check the setup and optionally write a redacted support bundle.
	Doctor(DoctorArgs),
	/// Interactive terminal UI.
	Tui,
}

/// Options of `relay daemon`.
#[derive(Debug, Clone, Args)]
pub struct DaemonArgs {
	/// Serve on loopback TCP with a token profile instead of a Unix socket.
	#[arg(long)]
	pub tcp: bool,
}

/// Selects a call; omitted means the only active call.
#[derive(Debug, Clone, Args)]
pub struct CallTarget {
	/// Call id (hex prefix) or peer name.
	pub call: Option<String>,
}

/// Two-state switch argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Switch {
	/// Enable.
	On,
	/// Disable.
	Off,
}

impl Switch {
	/// The boolean this switch stands for.
	pub fn enabled(self) -> bool {
		self == Self::On
	}
}

/// Arguments of `mute` and `deafen`.
#[derive(Debug, Clone, Args)]
pub struct SwitchArgs {
	/// New state.
	#[arg(value_enum, default_value_t = Switch::On)]
	pub state: Switch,
	/// Apply to this call only (id prefix or peer name) instead of globally.
	#[arg(long, value_name = "CALL")]
	pub call: Option<String>,
}

/// Which gain `relay gain` changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum GainKind {
	/// Microphone gain.
	Mic,
	/// Playback gain.
	Playback,
	/// Receive gain of one call (needs `--call` or a single active call).
	Receive,
}

/// Arguments of `relay gain`.
#[derive(Debug, Clone, Args)]
pub struct GainArgs {
	/// Gain to change.
	#[arg(value_enum)]
	pub kind: GainKind,
	/// Linear gain, finite and not negative (1.0 is unity).
	pub value: f32,
	/// Call for `receive`.
	#[arg(long, value_name = "CALL")]
	pub call: Option<String>,
}

/// Transmit behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TalkMode {
	/// Open microphone.
	On,
	/// Receive only; the microphone stays closed.
	Off,
	/// Push-to-talk.
	Ptt,
}

/// Arguments of `relay talk`.
#[derive(Debug, Clone, Args)]
pub struct TalkArgs {
	/// New transmit mode.
	#[arg(value_enum)]
	pub mode: TalkMode,
	/// Call id prefix or peer name.
	#[arg(long, value_name = "CALL")]
	pub call: Option<String>,
	/// Explicitly allow this call to use the microphone (required for on/ptt).
	#[arg(long)]
	pub consent: bool,
}

/// `relay devices ...`
#[derive(Debug, Subcommand)]
pub enum DevicesCommand {
	/// List input and output devices.
	List,
	/// Select the input and/or output device (id or name prefix).
	Select {
		/// Microphone.
		#[arg(long)]
		input: Option<String>,
		/// Speakers or headphones.
		#[arg(long)]
		output: Option<String>,
	},
}

/// `relay ns ...`
#[derive(Debug, Subcommand)]
pub enum NsCommand {
	/// Select a noise suppression backend by qualified id (`off` disables).
	Set {
		/// Qualified backend id.
		backend: String,
	},
}

/// `relay profile ...`
#[derive(Debug, Subcommand)]
pub enum ProfileCommand {
	/// Change the display name.
	Set {
		/// Name shown to peers.
		#[arg(long)]
		name: String,
	},
}

/// `relay trust ...`
#[derive(Debug, Subcommand)]
pub enum TrustCommand {
	/// List peers with their trust state.
	List,
	/// Mark a peer verified after comparing its fingerprint out of band.
	Verify {
		/// Peer name or id prefix.
		peer: String,
		/// Public key SHA-256 (hex) that you compared with the other person.
		#[arg(long, value_name = "HEX")]
		fingerprint: String,
	},
	/// Forget a peer's trust decision.
	Forget {
		/// Peer name or id prefix.
		peer: String,
	},
}

/// Arguments of `relay mic-test`.
#[derive(Debug, Clone, Args)]
pub struct MicTestArgs {
	/// How long to listen.
	#[arg(long, default_value_t = 5)]
	pub seconds: u64,
	/// Play the microphone back locally (may cause feedback).
	#[arg(long)]
	pub local_playback: bool,
	/// Acknowledge the feedback warning of `--local-playback`.
	#[arg(long, requires = "local_playback")]
	pub accept_feedback: bool,
}

/// Arguments of `relay doctor`.
#[derive(Debug, Clone, Args)]
pub struct DoctorArgs {
	/// Write a redacted support bundle to this file.
	#[arg(long, value_name = "FILE")]
	pub bundle: Option<PathBuf>,
}
