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
	/// Do not start the daemon when none is running (default: commands that
	/// need it start `relayd` and say so). Also `RELAY_NO_AUTOSTART=1`.
	#[arg(long, global = true)]
	pub no_autostart: bool,
	/// The `relayd` to start instead of the one next to `relay` or on PATH.
	#[arg(long, global = true, hide = true, value_name = "PATH")]
	pub daemon_bin: Option<PathBuf>,
	/// Where a daemon started by this command writes its log.
	#[arg(long, global = true, hide = true, value_name = "PATH")]
	pub daemon_log: Option<PathBuf>,
	/// How long to wait for the engine to finish an operation.
	#[arg(long, global = true, default_value_t = 5000, value_name = "MS")]
	pub timeout_ms: u64,
}

/// Top-level commands.
#[derive(Debug, Subcommand)]
pub enum Command {
	/// Start, stop and inspect the background engine (`relayd`).
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
	Tui(TuiArgs),
	/// Print the connection string others use to call this device.
	Address(AddressArgs),
	/// Global push-to-talk hotkey: run it, check its status, or change the key.
	Hotkey(HotkeyArgs),
	/// Who may call you and how you appear on the network.
	#[command(subcommand)]
	Privacy(PrivacyCommand),
	/// Private channels.
	#[command(subcommand)]
	Channel(ChannelCommand),
	/// Read, change, export or import settings.
	#[command(subcommand)]
	Config(ConfigCommand),
	/// Your avatar.
	#[command(subcommand)]
	Avatar(AvatarCommand),
}

impl Command {
	/// Whether the command registers a global hotkey, which on macOS must
	/// happen on the main thread while tokio runs elsewhere.
	pub fn uses_hotkeys(&self) -> bool {
		match self {
			Self::Tui(args) => !args.no_hotkey,
			Self::Hotkey(args) => args.command.is_none(),
			_ => false,
		}
	}

	/// The `--binding` given for this run, if any.
	pub fn binding_override(&self) -> Option<&str> {
		match self {
			Self::Tui(args) => args.binding.as_deref(),
			Self::Hotkey(args) => args.binding.as_deref(),
			_ => None,
		}
	}
}

/// Options of `relay tui`.
#[derive(Debug, Clone, Args)]
pub struct TuiArgs {
	/// Do not register the global push-to-talk hotkey.
	#[arg(long)]
	pub no_hotkey: bool,
	/// Push-to-talk accelerator for this run, instead of the saved one.
	#[arg(long, value_name = "KEYS", conflicts_with = "no_hotkey")]
	pub binding: Option<String>,
}

/// Options of `relay address`.
#[derive(Debug, Clone, Args)]
pub struct AddressArgs {
	/// Also copy the connection string to the clipboard.
	#[arg(long)]
	pub copy: bool,
}

/// `relay hotkey [status|set]`; without a subcommand it runs the hotkey.
#[derive(Debug, Clone, Args)]
pub struct HotkeyArgs {
	/// Push-to-talk accelerator for this run, instead of the saved one.
	#[arg(long, value_name = "KEYS")]
	pub binding: Option<String>,
	/// What to do instead of running.
	#[command(subcommand)]
	pub command: Option<HotkeyCommand>,
}

/// `relay hotkey ...`
#[derive(Debug, Clone, Subcommand)]
pub enum HotkeyCommand {
	/// Backend, permissions and the saved binding.
	Status,
	/// Save the push-to-talk accelerator (needs the settings lock).
	Set {
		/// For example `Ctrl+Alt+Space`; empty disables.
		accelerator: String,
	},
}

/// Inbound policy argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InboundArg {
	/// Any trusted peer may ring you.
	Anyone,
	/// Only members of a shared channel may ring you.
	SharedChannel,
}

/// Advertisement argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum AdvertArg {
	/// Name is shown in discovery.
	Visible,
	/// Name is not advertised.
	Hidden,
}

/// `relay privacy ...`
#[derive(Debug, Subcommand)]
pub enum PrivacyCommand {
	/// Change inbound policy and/or advertisement (at least one).
	Set {
		/// Who may ring this device.
		#[arg(long, value_enum)]
		inbound: Option<InboundArg>,
		/// Whether this device is advertised.
		#[arg(long, value_enum)]
		advert: Option<AdvertArg>,
	},
}

/// `relay channel ...`
#[derive(Debug, Subcommand)]
pub enum ChannelCommand {
	/// Create a channel owned by this device.
	Create(ChannelCreate),
	/// Join a channel by name or id.
	Join(ChannelJoin),
	/// Leave a channel and delete its local secrets.
	Leave {
		/// Channel name or id prefix.
		channel: String,
	},
	/// List your channels.
	List,
	/// Mint an invite token (printed once; treat it as a secret).
	Invite {
		/// Channel name or id prefix.
		channel: String,
		/// Lifetime of the token.
		#[arg(long, default_value_t = 3600)]
		ttl_seconds: u32,
		/// How many times it may be used.
		#[arg(long, default_value_t = 1)]
		uses: u32,
	},
	/// Revoke a member by identity fingerprint.
	Revoke {
		/// Channel name or id prefix.
		channel: String,
		/// Member fingerprint (64 hex).
		fingerprint: String,
	},
}

/// Arguments of `relay channel create`.
#[derive(Debug, Clone, Args)]
pub struct ChannelCreate {
	/// Channel name.
	pub name: String,
	/// Do not advertise the channel name.
	#[arg(long)]
	pub hidden: bool,
	/// Joining needs an invite token.
	#[arg(long)]
	pub invite_only: bool,
	/// Protect with a password, read from a hidden prompt.
	#[arg(long, conflicts_with = "password_stdin")]
	pub password: bool,
	/// Protect with a password, read from one line of stdin.
	#[arg(long)]
	pub password_stdin: bool,
}

/// Arguments of `relay channel join`.
#[derive(Debug, Clone, Args)]
pub struct ChannelJoin {
	/// Channel name, or its 32-hex id.
	pub channel: String,
	/// Password from a hidden prompt.
	#[arg(long, group = "credential")]
	pub password: bool,
	/// Password from one line of stdin.
	#[arg(long, group = "credential")]
	pub password_stdin: bool,
	/// Invite token from a hidden prompt.
	#[arg(long, group = "credential")]
	pub invite: bool,
	/// Invite token from one line of stdin.
	#[arg(long, group = "credential")]
	pub invite_stdin: bool,
}

/// `relay config ...`
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
	/// Print one setting, or all of them.
	Get {
		/// Dotted key such as `audio.microphone_gain`.
		key: Option<String>,
	},
	/// Change one setting (through the engine when it is running).
	Set {
		/// Dotted key.
		key: String,
		/// New value.
		value: String,
	},
	/// Write all settings as TOML to stdout or a file.
	Export {
		/// Destination file.
		file: Option<PathBuf>,
	},
	/// Replace all settings from an exported TOML file (engine must be stopped).
	Import {
		/// Source file.
		file: PathBuf,
	},
}

/// `relay avatar ...`
#[derive(Debug, Subcommand)]
pub enum AvatarCommand {
	/// Upload a PNG or JPEG (at most 1 MiB).
	Set {
		/// Image file.
		file: PathBuf,
	},
	/// Remove the avatar.
	Clear,
}

/// Options of `relay daemon`.
#[derive(Debug, Clone, Args)]
pub struct DaemonArgs {
	#[command(subcommand)]
	pub action: DaemonAction,
}

/// What to do with the daemon. The CLI never embeds the engine: it launches
/// and talks to the separate `relayd` binary.
#[derive(Debug, Clone, Subcommand)]
pub enum DaemonAction {
	/// Start `relayd` in the background (does nothing if it already runs).
	Start(DaemonLaunch),
	/// Ask the running daemon to stop: it ends calls, releases the microphone
	/// and removes its endpoint.
	Stop {
		/// Seconds to wait for the endpoint to disappear.
		#[arg(long, default_value_t = 10, value_name = "SECONDS")]
		wait: u64,
	},
	/// Stop the daemon if it runs, then start it again.
	Restart(DaemonLaunch),
	/// Show whether the daemon runs and which engine it is.
	Status,
	/// Print the end of the daemon's log file.
	Logs {
		/// Number of lines to show.
		#[arg(short = 'n', long, default_value_t = 40)]
		lines: usize,
		/// Read this log file instead of the default one.
		#[arg(long, value_name = "PATH")]
		file: Option<PathBuf>,
	},
	/// Run `relayd` in this terminal until it stops (Ctrl-C).
	Run(DaemonLaunch),
}

/// How to find and start `relayd`.
#[derive(Debug, Clone, Args)]
pub struct DaemonLaunch {
	/// Path to `relayd` (default: `RELAY_DAEMON_BIN`, next to `relay`, then PATH).
	#[arg(long, value_name = "PATH")]
	pub binary: Option<PathBuf>,
	/// Append the daemon's output to this file (default: the per-user log).
	#[arg(long, value_name = "PATH")]
	pub log_file: Option<PathBuf>,
	/// Seconds to wait for the daemon to accept connections.
	#[arg(long, default_value_t = 10, value_name = "SECONDS")]
	pub wait: u64,
	/// Further arguments for `relayd`, after `--`.
	#[arg(last = true)]
	pub relayd_args: Vec<String>,
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
