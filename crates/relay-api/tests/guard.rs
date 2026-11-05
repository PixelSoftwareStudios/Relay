#![cfg(unix)]
//! Middleware behavior: client context, size limits, stream bounds.

mod common;

use common::Harness;
use relay_api::pb::{RegisterClientRequest, UploadAvatarRequest};
use relay_api::{ApiLimits, RelayClient, StreamLimiter, WatchOptions};
use tonic::Code;

#[tokio::test]
async fn unregistered_client_context_is_rejected() {
	let h = Harness::start().await;
	let mut client = RelayClient::connect(&h.endpoint()).await.expect("connect");
	// Forge a context the registry never issued.
	let forged = relay_api::pb::ClientContext {
		client_id: vec![0xAB; 16],
		engine_incarnation: h.registry.incarnation().to_vec(),
	};
	let err = client
		.raw()
		.get_snapshot(forged.clone())
		.await
		.expect_err("no header");
	assert_eq!(err.code(), Code::Unauthenticated);
	let mut client = h.client().await;
	h.registry
		.remove(&client.context().unwrap().client_id.try_into().unwrap());
	let err = client.snapshot().await.expect_err("removed");
	assert!(matches!(err, relay_api::ApiError::Status(s) if s.code() == Code::Unauthenticated));
}

#[tokio::test]
async fn stale_incarnation_is_rejected() {
	let h = Harness::start().await;
	let mut client = h.client().await;
	let mut ctx = client.context().unwrap();
	ctx.engine_incarnation = vec![1; 16];
	client.adopt_context(ctx);
	let err = client.snapshot().await.expect_err("stale");
	assert!(matches!(err, relay_api::ApiError::Status(s) if s.code() == Code::FailedPrecondition));
}

#[tokio::test]
async fn server_refuses_oversized_message_from_unlimited_client() {
	use hyper_util::rt::TokioIo;
	use relay_api::pb::engine_client::EngineClient;

	let h = Harness::start().await;
	let ctx = h.client().await.context().unwrap();
	let path = h.socket.clone();
	let channel = tonic::transport::Endpoint::from_static("http://relay.local")
		.connect_with_connector(tower::service_fn(move |_| {
			let path = path.clone();
			async move {
				Ok::<_, std::io::Error>(TokioIo::new(tokio::net::UnixStream::connect(path).await?))
			}
		}))
		.await
		.expect("channel");
	let id = hex::encode(&ctx.client_id);
	let mut raw = EngineClient::with_interceptor(channel, move |mut r: tonic::Request<()>| {
		r.metadata_mut()
			.insert("relay-client-id", id.parse().unwrap());
		Ok(r)
	});
	let limit = ApiLimits::DEFAULT.max_message_bytes;
	let err = raw
		.upload_avatar(UploadAvatarRequest {
			encoded_image: vec![0; limit + 1],
			..UploadAvatarRequest::default()
		})
		.await
		.expect_err("too big");
	assert_eq!(err.code(), Code::OutOfRange);
}

#[tokio::test]
async fn client_limit_is_enforced() {
	let limits = ApiLimits {
		max_clients: 2,
		..ApiLimits::DEFAULT
	};
	let h = Harness::start_with(limits).await;
	let _a = h.client().await;
	let _b = h.client().await;
	let mut c = RelayClient::connect(&h.endpoint()).await.expect("connect");
	let err = c
		.raw()
		.register_client(RegisterClientRequest::default())
		.await
		.expect_err("third client");
	assert_eq!(err.code(), Code::ResourceExhausted);
}

#[tokio::test]
async fn stream_slots_are_bounded() {
	let limiter = StreamLimiter::new(2);
	let a = limiter.acquire().expect("a");
	let _b = limiter.acquire().expect("b");
	assert_eq!(
		limiter.acquire().expect_err("full").code(),
		Code::ResourceExhausted
	);
	drop(a);
	assert!(limiter.acquire().is_ok());
}

#[tokio::test]
async fn watch_streams_are_capped_server_side() {
	let limits = ApiLimits {
		max_streams: 1,
		..ApiLimits::DEFAULT
	};
	let h = Harness::start_with(limits).await;
	let mut a = h.client().await;
	let mut b = h.client().await;
	let _first = a.watch(WatchOptions::default()).await.expect("first");
	let err = b
		.watch(WatchOptions::default())
		.await
		.err()
		.expect("second refused");
	assert!(matches!(err, relay_api::ApiError::Status(s) if s.code() == Code::ResourceExhausted));
}
