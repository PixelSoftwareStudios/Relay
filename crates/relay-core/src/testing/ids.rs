use crate::id::{CallId, ClientId, DeviceKeyId, Incarnation, OperationId, SessionId};

/// Deterministic identifier source standing in for the entropy adapter.
#[derive(Debug, Clone)]
pub struct SeqIds {
	next: u128,
}

impl Default for SeqIds {
	fn default() -> Self {
		Self { next: 1 }
	}
}

impl SeqIds {
	/// Starts counting at one.
	pub fn new() -> Self {
		Self::default()
	}

	/// Starts counting at one inside a namespace, so two fake engines never
	/// mint the same identifier.
	pub fn namespaced(namespace: u8) -> Self {
		Self { next: (u128::from(namespace) << 64) | 1 }
	}

	fn bump(&mut self) -> u128 {
		let value = self.next;
		self.next += 1;
		value
	}

	/// A fresh call identifier.
	pub fn call(&mut self) -> CallId {
		CallId::from_u128(self.bump())
	}

	/// A fresh session identifier.
	pub fn session(&mut self) -> SessionId {
		SessionId::from_u128(self.bump())
	}

	/// A fresh operation identifier.
	pub fn operation(&mut self) -> OperationId {
		OperationId::from_u128(self.bump())
	}

	/// A fresh client identifier.
	pub fn client(&mut self) -> ClientId {
		ClientId::from_u128(self.bump())
	}

	/// A fresh incarnation identifier.
	pub fn incarnation(&mut self) -> Incarnation {
		Incarnation::from_u128(self.bump())
	}
}

/// A device key whose bytes are all `byte`; handy for readable fixtures.
pub fn device_key(byte: u8) -> DeviceKeyId {
	DeviceKeyId::from_bytes([byte; 32])
}
