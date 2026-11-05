//! Tower layer applied to every inbound request before any handler runs.
//!
//! Order of checks: loopback bearer token (when configured), declared body
//! size, in-flight cap, then client-context validation. Only `GetInfo` and
//! `RegisterClient` may be called without a registered client id.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tonic::Status;
use tonic::body::Body;
use tonic::codegen::http::{HeaderMap, Request, Response};
use tower::{Layer, Service};

use crate::registry::ClientValidator;
use crate::{AUTHORIZATION_HEADER, CLIENT_ID_HEADER, INCARNATION_HEADER};

const PUBLIC_METHODS: [&str; 2] = [
	"/relay.v1.Engine/GetInfo",
	"/relay.v1.Engine/RegisterClient",
];

/// RPCs whose bodies carry passwords or invite secrets. The guard logs only the
/// method path, never any request or response content, for every RPC.
pub const SENSITIVE_METHODS: [&str; 10] = [
	"/relay.v1.Engine/CreateChannel",
	"/relay.v1.Engine/JoinChannel",
	"/relay.v1.Engine/CreateInvite",
	"/relay.v1.Engine/SendSessionMessage",
	"/relay.v1.Engine/SendSessionImage",
	"/relay.v1.Engine/OfferFile",
	"/relay.v1.Engine/RespondOffer",
	"/relay.v1.Engine/GetTranscript",
	"/relay.v1.Engine/SearchSession",
	"/relay.v1.Engine/ExportSession",
];

/// Configuration shared by all guarded requests.
#[derive(Clone)]
pub struct GuardConfig {
	/// Validates client context; typically a [`crate::ClientRegistry`].
	pub validator: Arc<dyn ClientValidator>,
	/// Required `Bearer` token for the loopback-TCP profile; `None` on UDS.
	pub token: Option<Arc<str>>,
	/// Reject requests declaring a larger body than this.
	pub max_message_bytes: usize,
	/// Maximum requests executing at once.
	pub max_in_flight: usize,
}

/// Layer wrapping an engine service with [`GuardConfig`] enforcement.
#[derive(Clone)]
pub struct GuardLayer {
	config: GuardConfig,
	permits: Arc<Semaphore>,
}

impl GuardLayer {
	/// Build the layer from a configuration.
	pub fn new(config: GuardConfig) -> Self {
		let permits = Arc::new(Semaphore::new(config.max_in_flight));
		Self { config, permits }
	}
}

impl<S> Layer<S> for GuardLayer {
	type Service = Guard<S>;

	fn layer(&self, inner: S) -> Self::Service {
		Guard {
			inner,
			config: self.config.clone(),
			permits: Arc::clone(&self.permits),
		}
	}
}

/// Service produced by [`GuardLayer`].
#[derive(Clone)]
pub struct Guard<S> {
	inner: S,
	config: GuardConfig,
	permits: Arc<Semaphore>,
}

type BoxFuture<T, E> = Pin<Box<dyn Future<Output = Result<T, E>> + Send>>;

impl<S> Service<Request<Body>> for Guard<S>
where
	S: Service<Request<Body>, Response = Response<Body>> + Clone + Send + 'static,
	S::Future: Send + 'static,
	S::Error: Send + 'static,
{
	type Response = Response<Body>;
	type Error = S::Error;
	type Future = BoxFuture<Self::Response, Self::Error>;

	fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		self.inner.poll_ready(cx)
	}

	fn call(&mut self, req: Request<Body>) -> Self::Future {
		let clone = self.inner.clone();
		let mut inner = std::mem::replace(&mut self.inner, clone);
		let checked = check_request(&self.config, &self.permits, &req);
		Box::pin(async move {
			match checked {
				Ok(_permit) => inner.call(req).await,
				Err(status) => Ok(status.into_http()),
			}
		})
	}
}

fn check_request(
	config: &GuardConfig,
	permits: &Arc<Semaphore>,
	req: &Request<Body>,
) -> Result<OwnedSemaphorePermit, Status> {
	let headers = req.headers();
	tracing::debug!(
		method = req.uri().path(),
		sensitive = SENSITIVE_METHODS.contains(&req.uri().path()),
		"local api request"
	);
	if let Some(token) = &config.token {
		check_token(headers, token)?;
	}
	if let Some(len) = declared_length(headers)
		&& len > config.max_message_bytes as u64
	{
		return Err(Status::resource_exhausted("message too large"));
	}
	let permit = Arc::clone(permits)
		.try_acquire_owned()
		.map_err(|_| Status::resource_exhausted("too many requests in flight"))?;
	if !PUBLIC_METHODS.contains(&req.uri().path()) {
		let client = hex_header::<16>(headers, CLIENT_ID_HEADER)?
			.ok_or_else(|| Status::unauthenticated("missing client context"))?;
		let incarnation = hex_bytes(headers, INCARNATION_HEADER)?;
		config.validator.validate(&client, incarnation.as_deref())?;
	}
	Ok(permit)
}

fn check_token(headers: &HeaderMap, token: &str) -> Result<(), Status> {
	let supplied = headers
		.get(AUTHORIZATION_HEADER)
		.and_then(|v| v.to_str().ok())
		.and_then(|v| v.strip_prefix("Bearer "))
		.ok_or_else(|| Status::unauthenticated("missing bearer token"))?;
	if constant_time_eq(supplied.as_bytes(), token.as_bytes()) {
		Ok(())
	} else {
		Err(Status::unauthenticated("invalid bearer token"))
	}
}

pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
	let mut diff = a.len() ^ b.len();
	for (i, byte) in a.iter().enumerate() {
		diff |= usize::from(*byte ^ b.get(i).copied().unwrap_or(0));
	}
	diff == 0
}

/// The 5-byte gRPC frame prefix is not counted; this is only an early reject
/// for clients that send a length. Tonic's decode limit remains authoritative.
fn declared_length(headers: &HeaderMap) -> Option<u64> {
	headers.get("content-length")?.to_str().ok()?.parse().ok()
}

fn hex_bytes(headers: &HeaderMap, name: &str) -> Result<Option<Vec<u8>>, Status> {
	let Some(value) = headers.get(name) else {
		return Ok(None);
	};
	value
		.to_str()
		.ok()
		.and_then(|s| hex::decode(s).ok())
		.map(Some)
		.ok_or_else(|| Status::invalid_argument("malformed client context"))
}

fn hex_header<const N: usize>(headers: &HeaderMap, name: &str) -> Result<Option<[u8; N]>, Status> {
	match hex_bytes(headers, name)? {
		None => Ok(None),
		Some(bytes) => <[u8; N]>::try_from(bytes)
			.map(Some)
			.map_err(|_| Status::invalid_argument("malformed client context")),
	}
}

/// Caps concurrently open server streams. Engines acquire a [`StreamGuard`]
/// at the start of every streaming handler and move it into the stream task.
#[derive(Debug, Clone)]
pub struct StreamLimiter {
	slots: Arc<Semaphore>,
}

/// Holds one stream slot until dropped.
#[derive(Debug)]
pub struct StreamGuard {
	_permit: OwnedSemaphorePermit,
}

impl StreamLimiter {
	/// Allow at most `max_streams` concurrent streams.
	pub fn new(max_streams: usize) -> Self {
		Self {
			slots: Arc::new(Semaphore::new(max_streams)),
		}
	}

	/// Claim a slot or fail with `RESOURCE_EXHAUSTED`.
	pub fn acquire(&self) -> Result<StreamGuard, Status> {
		Arc::clone(&self.slots)
			.try_acquire_owned()
			.map(|_permit| StreamGuard { _permit })
			.map_err(|_| Status::resource_exhausted("too many open streams"))
	}
}
