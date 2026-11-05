/// Largest avatar image the API will carry: 1 MiB, for uploads and canonical WebP files alike.
pub const AVATAR_MAX_BYTES: usize = 1024 * 1024;

/// Resource bounds applied by the transport and middleware.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiLimits {
	/// Largest accepted or produced gRPC message in bytes.
	pub max_message_bytes: usize,
	/// Simultaneously registered local clients.
	pub max_clients: usize,
	/// Requests being processed at once across all clients.
	pub max_in_flight: usize,
	/// Concurrently open server streams across all clients.
	pub max_streams: usize,
	/// Events queued per lifecycle subscriber before it must resync.
	pub subscriber_queue: usize,
}

impl ApiLimits {
	/// Defaults from the resource budget: messages of 1 MiB plus room for the envelope of
	/// a maximal avatar, 32 clients, 256-event queues.
	pub const DEFAULT: Self = Self {
		max_message_bytes: AVATAR_MAX_BYTES + 64 * 1024,
		max_clients: 32,
		max_in_flight: 64,
		max_streams: 96,
		subscriber_queue: 256,
	};
}

impl Default for ApiLimits {
	fn default() -> Self {
		Self::DEFAULT
	}
}
