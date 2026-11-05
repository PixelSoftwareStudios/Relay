use mdns_sd::{IfKind, IfPredicate, ServiceDaemon};

use super::DiscoveryError;

/// Name prefixes of interfaces that are tunnels, VPNs, bridges or container networks.
/// They are excluded unless named explicitly in `allowed`.
const EXCLUDED_PREFIXES: &[&str] = &[
	"utun",
	"tun",
	"tap",
	"wg",
	"ppp",
	"ipsec",
	"docker",
	"br-",
	"veth",
	"virbr",
	"vmnet",
	"vboxnet",
	"tailscale",
	"zt",
	"awdl",
	"llw",
	"anpi",
];

/// Which interfaces take part in discovery.
#[derive(Debug, Clone, Default)]
pub struct InterfacePolicy {
	/// When non-empty, only these interface names are used (and may include tunnel names the
	/// user deliberately chose).
	pub allowed: Vec<String>,
	/// Admit loopback. Never enabled for real advertisement; exists for development.
	pub include_loopback: bool,
}

impl InterfacePolicy {
	pub(super) fn apply(&self, daemon: &ServiceDaemon) -> Result<(), DiscoveryError> {
		if self.allowed.is_empty() {
			let include_loopback = self.include_loopback;
			daemon.disable_interface(IfKind::Predicate(IfPredicate::new(move |intf| {
				let excluded = EXCLUDED_PREFIXES.iter().any(|p| intf.name.starts_with(p));
				excluded || intf.is_p2p() || (intf.is_loopback() && !include_loopback)
			})))?;
			if self.include_loopback {
				daemon.enable_interface(IfKind::LoopbackV4)?;
			}
		} else {
			daemon.disable_interface(IfKind::All)?;
			for name in &self.allowed {
				daemon.enable_interface(IfKind::Name(name.clone()))?;
			}
		}
		Ok(())
	}

	/// Whether an interface name may be used. With `allowed` set only those names qualify;
	/// otherwise tunnel, VPN, bridge and container names are excluded.
	pub fn admits_name(&self, name: &str) -> bool {
		if self.allowed.is_empty() {
			!EXCLUDED_PREFIXES.iter().any(|p| name.starts_with(p))
		} else {
			self.allowed.iter().any(|a| a == name)
		}
	}

	/// Names of the interfaces currently eligible, for display to the user.
	pub fn describe(&self) -> String {
		if self.allowed.is_empty() {
			"all non-loopback, non-tunnel interfaces".to_owned()
		} else {
			self.allowed.join(", ")
		}
	}
}
