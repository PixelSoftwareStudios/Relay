//! Deterministic fakes for exercising the domain without I/O.

mod clock;
mod device;
mod ids;
mod network;
mod node;
mod watcher;

pub use clock::FakeClock;
pub use device::FakeDevice;
pub use ids::{SeqIds, device_key};
pub use network::FakeNetwork;
pub use node::{Node, pump};
pub use watcher::FakeWatcher;
