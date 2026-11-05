//! `RelayClient`: connect, register, snapshot/watch and operation polling.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use tonic::metadata::MetadataValue;
use tonic::service::Interceptor;
use tonic::service::interceptor::InterceptedService;
use tonic::transport::{Channel, Endpoint as TonicEndpoint};
use tonic::{Request, Status};

use crate::pb::engine_client::EngineClient;
use crate::pb::{
	ClientContext, ClientSession, Empty, EngineInfo, GetOperationRequest, Operation,
	OperationState, RegisterClientRequest, Snapshot, WatchStateRequest,
};
use crate::watch::{StateTracker, StateWatch, WatchOptions};
use crate::{AUTHORIZATION_HEADER, ApiError, CLIENT_ID_HEADER, INCARNATION_HEADER, TcpProfile};

/// Where the engine listens.
#[derive(Debug, Clone)]
pub enum Endpoint {
	/// Owner-only Unix socket path.
	#[cfg(unix)]
	Unix(PathBuf),
	/// Loopback TCP described by a private profile file.
	LoopbackTcp {
		/// Path of the profile written by the engine.
		profile_path: PathBuf,
	},
}

#[derive(Clone, Default)]
struct Shared {
	context: Arc<RwLock<Option<ClientContext>>>,
	token: Option<Arc<str>>,
}

/// Interceptor attaching the token and registered client context to calls.
#[derive(Clone)]
pub struct ContextInterceptor(Shared);

impl Interceptor for ContextInterceptor {
	fn call(&mut self, mut req: Request<()>) -> Result<Request<()>, Status> {
		let meta = req.metadata_mut();
		if let Some(token) = &self.0.token {
			let value = MetadataValue::try_from(format!("Bearer {token}"))
				.map_err(|_| Status::internal("bad token"))?;
			meta.insert(AUTHORIZATION_HEADER, value);
		}
		let ctx = self.0.context.read().unwrap_or_else(|e| e.into_inner());
		if let Some(ctx) = ctx.as_ref() {
			let id = hex::encode(&ctx.client_id)
				.parse()
				.map_err(|_| Status::internal("bad client id"))?;
			meta.insert(CLIENT_ID_HEADER, id);
			let inc = hex::encode(&ctx.engine_incarnation)
				.parse()
				.map_err(|_| Status::internal("bad incarnation"))?;
			meta.insert(INCARNATION_HEADER, inc);
		}
		Ok(req)
	}
}

type Inner = EngineClient<InterceptedService<Channel, ContextInterceptor>>;

/// High-level client for the local engine API.
pub struct RelayClient {
	inner: Inner,
	shared: Shared,
	info: EngineInfo,
}

impl RelayClient {
	/// Connect to an endpoint and fetch `GetInfo`.
	pub async fn connect(endpoint: &Endpoint) -> Result<Self, ApiError> {
		let mut shared = Shared::default();
		let channel = match endpoint {
			#[cfg(unix)]
			Endpoint::Unix(path) => connect_unix(path.clone()).await?,
			Endpoint::LoopbackTcp { profile_path } => {
				let profile = TcpProfile::load(profile_path)?;
				shared.token = Some(Arc::from(profile.token.as_str()));
				TonicEndpoint::from_shared(format!("http://{}", profile.addr))
					.map_err(|_| ApiError::BadProfile("unusable address"))?
					.connect()
					.await?
			}
		};
		let mut inner = EngineClient::with_interceptor(channel, ContextInterceptor(shared.clone()))
			.max_decoding_message_size(crate::ApiLimits::DEFAULT.max_message_bytes)
			.max_encoding_message_size(crate::ApiLimits::DEFAULT.max_message_bytes);
		let info = inner.get_info(Empty {}).await?.into_inner();
		Ok(Self {
			inner,
			shared,
			info,
		})
	}

	/// Engine information captured at connect time.
	pub fn info(&self) -> &EngineInfo {
		&self.info
	}

	/// Raw generated client for RPCs without a wrapper.
	pub fn raw(&mut self) -> &mut Inner {
		&mut self.inner
	}

	/// The registered context, if any.
	pub fn context(&self) -> Option<ClientContext> {
		self.shared
			.context
			.read()
			.unwrap_or_else(|e| e.into_inner())
			.clone()
	}

	/// Reuse a context issued earlier (for example cached across a reconnect).
	/// The engine still validates it on every request.
	pub fn adopt_context(&mut self, context: ClientContext) {
		*self
			.shared
			.context
			.write()
			.unwrap_or_else(|e| e.into_inner()) = Some(context);
	}

	fn require_context(&self) -> Result<ClientContext, ApiError> {
		self.context().ok_or(ApiError::NotRegistered)
	}

	/// Register this client. The id is an identifier, not authentication.
	pub async fn register(
		&mut self,
		display_name: &str,
		client_version: &str,
		capabilities: &[&str],
	) -> Result<ClientSession, ApiError> {
		let session = self
			.inner
			.register_client(RegisterClientRequest {
				display_name: display_name.to_owned(),
				client_version: client_version.to_owned(),
				api_major: self.info.api_major,
				requested_capabilities: capabilities.iter().map(|c| (*c).to_owned()).collect(),
			})
			.await?
			.into_inner();
		let context = session
			.context
			.clone()
			.ok_or(ApiError::InvalidResponse("session without context"))?;
		*self
			.shared
			.context
			.write()
			.unwrap_or_else(|e| e.into_inner()) = Some(context);
		Ok(session)
	}

	/// Fetch the current snapshot.
	pub async fn snapshot(&mut self) -> Result<Snapshot, ApiError> {
		let ctx = self.require_context()?;
		Ok(self.inner.get_snapshot(ctx).await?.into_inner())
	}

	/// Open a watch. Without `after_revision` the first item is a snapshot.
	pub async fn watch(&mut self, options: WatchOptions) -> Result<StateWatch, ApiError> {
		let client = self.require_context()?;
		let tracker = match options.after_revision {
			Some(rev) => {
				StateTracker::resumed(options.is_strict(), client.engine_incarnation.clone(), rev)
			}
			None => StateTracker::new(options.is_strict()),
		};
		let stream = self
			.inner
			.watch_state(WatchStateRequest {
				client: Some(client),
				after_revision: options.after_revision,
				call_filter: options.call_filter,
				include_peers: options.include_peers,
			})
			.await?
			.into_inner();
		Ok(StateWatch::new(stream, tracker))
	}

	/// Fetch a snapshot and resume a watch right after it. A gap between the
	/// two surfaces as [`ApiError::RevisionGap`] or a `Resync` item; callers
	/// recover by calling [`Self::watch`] with default options.
	pub async fn snapshot_and_watch(&mut self) -> Result<(Snapshot, StateWatch), ApiError> {
		let snapshot = self.snapshot().await?;
		let watch = self
			.watch(WatchOptions {
				after_revision: Some(snapshot.revision),
				..WatchOptions::default()
			})
			.await?;
		Ok((snapshot, watch))
	}

	/// Look up an operation once.
	pub async fn get_operation(&mut self, operation_id: &[u8]) -> Result<Operation, ApiError> {
		let client = self.require_context()?;
		Ok(self
			.inner
			.get_operation(GetOperationRequest {
				client: Some(client),
				operation_id: operation_id.to_vec(),
			})
			.await?
			.into_inner())
	}

	/// Poll until the operation leaves `ACCEPTED`, or `timeout` passes.
	pub async fn wait_operation(
		&mut self,
		operation_id: &[u8],
		interval: Duration,
		timeout: Duration,
	) -> Result<Operation, ApiError> {
		let deadline = tokio::time::Instant::now() + timeout;
		loop {
			let op = self.get_operation(operation_id).await?;
			if op.state != OperationState::Accepted as i32 {
				return Ok(op);
			}
			if tokio::time::Instant::now() + interval > deadline {
				return Err(ApiError::OperationTimeout);
			}
			tokio::time::sleep(interval).await;
		}
	}
}

#[cfg(unix)]
async fn connect_unix(path: PathBuf) -> Result<Channel, ApiError> {
	use hyper_util::rt::TokioIo;
	use tokio::net::UnixStream;

	// The URI is a placeholder; the connector ignores it.
	let channel = TonicEndpoint::from_static("http://relay.local")
		.connect_with_connector(tower::service_fn(move |_| {
			let path = path.clone();
			async move { Ok::<_, std::io::Error>(TokioIo::new(UnixStream::connect(path).await?)) }
		}))
		.await?;
	Ok(channel)
}
