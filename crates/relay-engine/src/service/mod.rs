//! The control service: one actor owning the domain state, and the local API
//! implementation that talks to it.

mod actor;
mod actor_ops;
mod actor_settings;
mod clients;
mod convert;
mod directory;
mod error;
pub mod fakes;
mod feed;
mod gates;
mod handle;
mod ids;
mod ledger;
pub mod ports;
pub mod presence;
mod rpc;
mod runtime;
mod settings_keys;
pub mod telemetry;
mod tones;

pub use clients::Capability;
pub use error::ServiceError;
pub use gates::{GateReader, GateTable};
pub use handle::{ControlHandle, ServiceOp};
pub use ids::{CounterEntropy, Entropy, Ids, OsEntropy};
pub use ports::{
	AdvertMode, CallMeter, ConnectionDetails, DeviceControl, DeviceDirection, DeviceInfo,
	ForgottenPeer, InboundPolicy, LocalProfile, MediaControl, MediaTelemetry, PeerControl,
	PeerInfo, PeerSelector, PeerTrust, PortError, Ports, SettingsChange, SettingsStore,
	StoredSettings, TrustRequest,
};
pub use rpc::EngineService;
pub use runtime::{Service, ServiceBuilder, ServiceConfig};
pub use settings_keys::{KeyOutcome, KeyResult, is_live, is_messaging, is_sensitive};
pub use tones::{RingPrefs, RingUpdate, ToneCommand, ToneDriver, ToneInput};
