//! The service that is on the socket from the first moment, before the engine exists.
//!
//! The daemon binds its endpoint and serves an [`EngineSwitch`] at once; opening the
//! system's secure storage (which can wait minutes for a password prompt) happens
//! afterwards. Until the real engine is installed the switch answers `GetInfo` with the
//! start-up state and refuses every other call as `UNAVAILABLE`. The three start-up
//! calls (`RetryKeychain`, `UseFileStore`, `ResetIdentity`) always go to the
//! [`StartupControl`], whatever the state.

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll};

use tonic::Status;
use tonic::body::Body;
use tonic::codegen::http::{Request, Response};
use tonic::server::NamedService;
use tower::Service;

use crate::pb::engine_server::{Engine, EngineServer};
use crate::pb::{Empty, EngineInfo};
use crate::registry::ClientValidator;

const GET_INFO: &str = "/relay.v1.Engine/GetInfo";
const RETRY: &str = "/relay.v1.Engine/RetryKeychain";
const USE_FILE: &str = "/relay.v1.Engine/UseFileStore";
const RESET: &str = "/relay.v1.Engine/ResetIdentity";

/// Calls that need no registered client: a client cannot register before the engine runs.
pub(crate) const STARTUP_METHODS: [&str; 3] = [RETRY, USE_FILE, RESET];

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// What the daemon does for the start-up calls.
#[tonic::async_trait]
pub trait StartupControl: Send + Sync + 'static {
	/// The information `GetInfo` returns while the engine is not installed.
	fn info(&self) -> EngineInfo;
	/// Read the secure storage again after a failure.
	async fn retry(&self) -> Result<EngineInfo, Status>;
	/// Switch to the file-backed store, then retry.
	async fn use_file_store(&self) -> Result<EngineInfo, Status>;
	/// Delete the stored identity; the daemon stops afterwards.
	async fn reset_identity(&self) -> Result<EngineInfo, Status>;
}

/// Validates clients once the engine's registry exists; before that nobody is known.
#[derive(Default)]
pub struct LateValidator {
	inner: OnceLock<Arc<dyn ClientValidator>>,
}

impl LateValidator {
	/// Starts answering with `validator`.
	pub fn install(&self, validator: Arc<dyn ClientValidator>) {
		let _ = self.inner.set(validator);
	}
}

impl ClientValidator for LateValidator {
	fn validate(&self, client: &crate::ClientId, incarnation: Option<&[u8]>) -> Result<(), Status> {
		match self.inner.get() {
			Some(validator) => validator.validate(client, incarnation),
			None => Err(Status::unavailable("STARTING")),
		}
	}
}

/// Where the real engine is put once it is ready.
pub struct EngineSlot<E>(Arc<OnceLock<EngineServer<E>>>);

impl<E> Clone for EngineSlot<E> {
	fn clone(&self) -> Self {
		Self(Arc::clone(&self.0))
	}
}

impl<E: Engine> EngineSlot<E> {
	/// From now on every call except the start-up ones goes to `server`.
	pub fn install(&self, server: EngineServer<E>) {
		let _ = self.0.set(server);
	}
}

/// The `relay.v1.Engine` service before and after the engine exists.
pub struct EngineSwitch<E> {
	control: Arc<dyn StartupControl>,
	real: Arc<OnceLock<EngineServer<E>>>,
}

impl<E> Clone for EngineSwitch<E> {
	fn clone(&self) -> Self {
		Self {
			control: Arc::clone(&self.control),
			real: Arc::clone(&self.real),
		}
	}
}

impl<E: Engine> EngineSwitch<E> {
	/// A switch with no engine yet, and the slot to install it into.
	pub fn new(control: Arc<dyn StartupControl>) -> (Self, EngineSlot<E>) {
		let real = Arc::new(OnceLock::new());
		(
			Self {
				control,
				real: Arc::clone(&real),
			},
			EngineSlot(real),
		)
	}
}

impl<E> NamedService for EngineSwitch<E> {
	const NAME: &'static str = "relay.v1.Engine";
}

/// One unary call answered by `answer`.
struct Answer(Option<BoxFuture<Result<EngineInfo, Status>>>);

impl tonic::server::UnaryService<Empty> for Answer {
	type Response = EngineInfo;
	type Future = BoxFuture<Result<tonic::Response<EngineInfo>, Status>>;

	fn call(&mut self, _: tonic::Request<Empty>) -> Self::Future {
		let answer = self.0.take();
		Box::pin(async move {
			match answer {
				Some(answer) => answer.await.map(tonic::Response::new),
				None => Err(Status::internal("answered twice")),
			}
		})
	}
}

fn answer(
	request: Request<Body>,
	reply: BoxFuture<Result<EngineInfo, Status>>,
) -> BoxFuture<Result<Response<Body>, Infallible>> {
	Box::pin(async move {
		let mut grpc = tonic::server::Grpc::new(tonic_prost::ProstCodec::default());
		Ok(grpc.unary(Answer(Some(reply)), request).await)
	})
}

impl<E: Engine> Service<Request<Body>> for EngineSwitch<E> {
	type Response = Response<Body>;
	type Error = Infallible;
	type Future = BoxFuture<Result<Response<Body>, Infallible>>;

	fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
		Poll::Ready(Ok(()))
	}

	fn call(&mut self, request: Request<Body>) -> Self::Future {
		let control = Arc::clone(&self.control);
		let path = request.uri().path().to_owned();
		match path.as_str() {
			RETRY => answer(request, Box::pin(async move { control.retry().await })),
			USE_FILE => answer(
				request,
				Box::pin(async move { control.use_file_store().await }),
			),
			RESET => answer(
				request,
				Box::pin(async move { control.reset_identity().await }),
			),
			_ => match self.real.get() {
				Some(server) => {
					let mut server = server.clone();
					Box::pin(async move { server.call(request).await })
				}
				None if path == GET_INFO => {
					answer(request, Box::pin(async move { Ok(control.info()) }))
				}
				None => Box::pin(async { Ok(Status::unavailable("STARTING").into_http()) }),
			},
		}
	}
}
