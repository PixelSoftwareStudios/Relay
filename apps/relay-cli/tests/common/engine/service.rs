//! The `Engine` service of the test engine: records mutations, serves fixed state.

use std::sync::Arc;

use relay_api::pb::engine_server::Engine;
use relay_api::pb::state_envelope::Body;
use relay_api::pb::*;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use super::{FakeEngine, Resp, ctx_client, unsupported};
use crate::common::{chat_watch, fixtures};

#[tonic::async_trait]
impl Engine for FakeEngine {
	async fn get_info(&self, _: Request<Empty>) -> Resp<EngineInfo> {
		Ok(Response::new(EngineInfo {
			engine_incarnation: self.registry.incarnation().to_vec(),
			application_version: "test-engine".into(),
			api_major: 1,
			api_minor: 0,
			capabilities: vec!["observe".into(), "call-control".into()],
			limits: None,
			build_revision: "test".into(),
			api_revision: relay_api::API_REVISION - 1,
			startup: None,
		}))
	}

	async fn retry_keychain(&self, _: Request<Empty>) -> Resp<EngineInfo> {
		unsupported()
	}

	async fn use_file_store(&self, _: Request<Empty>) -> Resp<EngineInfo> {
		unsupported()
	}

	async fn reset_identity(&self, _: Request<Empty>) -> Resp<EngineInfo> {
		unsupported()
	}

	async fn register_client(&self, r: Request<RegisterClientRequest>) -> Resp<ClientSession> {
		let requested = r.into_inner().requested_capabilities;
		let wants_messages = requested.iter().any(|c| c == "messages");
		self.registered.lock().unwrap().push(requested);
		let id = self.registry.register()?;
		if wants_messages {
			self.messaging_clients.lock().unwrap().push(id.to_vec());
		}
		Ok(Response::new(ClientSession {
			context: Some(ClientContext {
				client_id: id.to_vec(),
				engine_incarnation: self.registry.incarnation().to_vec(),
			}),
			granted_capabilities: vec![
				if wants_messages {
					"messages"
				} else {
					"observe"
				}
				.into(),
			],
			revision: 10,
			daemon_build_revision: "test".into(),
			daemon_api_revision: relay_api::API_REVISION - 1,
		}))
	}

	async fn get_snapshot(&self, _: Request<ClientContext>) -> Resp<Snapshot> {
		Ok(Response::new(self.snapshot.lock().unwrap().clone()))
	}

	type WatchStateStream = ReceiverStream<Result<StateEnvelope, Status>>;
	async fn watch_state(&self, _: Request<WatchStateRequest>) -> Resp<Self::WatchStateStream> {
		let snapshot = self.snapshot.lock().unwrap().clone();
		let (tx, rx) = mpsc::channel(4);
		let envelope = StateEnvelope {
			engine_incarnation: self.registry.incarnation().to_vec(),
			revision: snapshot.revision,
			body: Some(Body::Snapshot(snapshot)),
		};
		tokio::spawn(async move {
			let _ = tx.send(Ok(envelope)).await;
			tx.closed().await;
		});
		Ok(Response::new(ReceiverStream::new(rx)))
	}

	type WatchTelemetryStream = ReceiverStream<Result<TelemetrySnapshot, Status>>;
	async fn watch_telemetry(
		&self,
		_: Request<TelemetryRequest>,
	) -> Resp<Self::WatchTelemetryStream> {
		let (tx, rx) = mpsc::channel(8);
		tokio::spawn(async move {
			for (i, db) in [-40.0_f32, -12.5, -30.0].into_iter().enumerate() {
				let sample = TelemetrySnapshot {
					sample_sequence: i as u64,
					microphone_dbfs: Some(db),
					..TelemetrySnapshot::default()
				};
				if tx.send(Ok(sample)).await.is_err() {
					return;
				}
			}
			tx.closed().await;
		});
		Ok(Response::new(ReceiverStream::new(rx)))
	}

	async fn get_operation(&self, _: Request<GetOperationRequest>) -> Resp<Operation> {
		Err(Status::not_found("unknown operation"))
	}

	async fn start_call(&self, r: Request<StartCallRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		let target = match r.target {
			Some(start_call_request::Target::PeerId(id)) => format!("peer={}", hex::encode(id)),
			Some(start_call_request::Target::ManualLocalAddress(a)) => format!("addr={a}"),
			Some(start_call_request::Target::ManualConnectionString(s)) => format!("string={s}"),
			None => "none".into(),
		};
		self.op(r.context.as_ref(), format!("start_call {target}"))
	}
	async fn accept_call(&self, r: Request<CallMutation>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(
			r.context.as_ref(),
			format!("accept gen={}", r.controller_generation),
		)
	}
	async fn decline_call(&self, r: Request<CallMutation>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(r.context.as_ref(), "decline".into())
	}
	async fn end_call(&self, r: Request<CallMutation>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(
			r.context.as_ref(),
			format!("end call={}", hex::encode(&r.call_id)),
		)
	}
	async fn acquire_controller(
		&self,
		r: Request<AcquireControllerRequest>,
	) -> Resp<ControllerLease> {
		let r = r.into_inner();
		self.log
			.lock()
			.unwrap()
			.push(format!("acquire takeover={}", r.request_explicit_takeover));
		Ok(Response::new(ControllerLease {
			call_id: r.call_id,
			generation: 5,
			expires_in_ms: 10_000,
			..ControllerLease::default()
		}))
	}
	async fn release_controller(&self, _: Request<ReleaseControllerRequest>) -> Resp<Operation> {
		unsupported()
	}
	async fn set_transmit_mode(&self, r: Request<SetTransmitModeRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(r.context.as_ref(), format!("route mode={}", r.mode))
	}
	async fn connect_and_trust(&self, r: Request<ConnectAndTrustRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(
			r.context.as_ref(),
			format!("connect_and_trust {}", r.connection_string),
		)
	}
	type WatchCallStatsStream = ReceiverStream<Result<CallStats, Status>>;
	async fn watch_call_stats(
		&self,
		_: Request<WatchCallStatsRequest>,
	) -> Resp<Self::WatchCallStatsStream> {
		unsupported()
	}
	async fn set_call_controls(&self, r: Request<SetCallControlsRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(
			r.context.as_ref(),
			format!(
				"call_controls muted={:?} gain={:?}",
				r.muted, r.receive_gain
			),
		)
	}
	async fn set_global_controls(&self, r: Request<SetGlobalControlsRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(
			r.context.as_ref(),
			format!("global muted={:?} mic={:?}", r.muted, r.microphone_gain),
		)
	}
	async fn select_devices(&self, r: Request<SelectDevicesRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(
			r.context.as_ref(),
			format!("devices {} {}", r.input_device_id, r.output_device_id),
		)
	}
	async fn set_noise_suppression(
		&self,
		r: Request<SetNoiseSuppressionRequest>,
	) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(r.context.as_ref(), format!("ns {}", r.qualified_backend_id))
	}
	async fn start_mic_test(&self, r: Request<StartMicTestRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(r.context.as_ref(), "mic_test_start".into())
	}
	async fn stop_mic_test(&self, r: Request<MutationContext>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(Some(&r), "mic_test_stop".into())
	}
	async fn preview_tone(&self, r: Request<MutationContext>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(Some(&r), "preview_tone".into())
	}
	async fn set_ringtone(&self, r: Request<SetRingtoneRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		let what = if r.reset {
			"reset".to_owned()
		} else {
			format!("{} {}", r.file_name, r.data.len())
		};
		self.op(r.context.as_ref(), format!("set_ringtone {what}"))
	}
	async fn set_profile(&self, r: Request<SetProfileRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(r.context.as_ref(), format!("profile {}", r.display_name))
	}
	async fn upload_avatar(&self, r: Request<UploadAvatarRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(
			r.context.as_ref(),
			format!(
				"avatar {} bytes {}",
				r.encoded_image.len(),
				r.declared_media_type
			),
		)
	}
	async fn decide_peer_trust(&self, r: Request<DecidePeerTrustRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(
			r.context.as_ref(),
			format!("trust {} oob={}", r.requested_trust, r.out_of_band_verified),
		)
	}

	async fn create_channel(&self, r: Request<CreateChannelRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.secrets.lock().unwrap().push(r.password.clone());
		let policy = r.policy.unwrap_or_default();
		self.op(
			r.context.as_ref(),
			format!(
				"create_channel {} listed={} password={} invite_only={}",
				r.name,
				policy.listed,
				!r.password.is_empty(),
				policy.invite_only
			),
		)
	}
	async fn join_channel(&self, r: Request<JoinChannelRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.secrets.lock().unwrap().push(r.password.clone());
		self.secrets.lock().unwrap().push(r.invite.clone());
		let by = if r.channel_id.is_empty() {
			format!("name={}", r.name)
		} else {
			format!("id={}", hex::encode(&r.channel_id))
		};
		self.op(
			r.context.as_ref(),
			format!(
				"join_channel {by} password={} invite={}",
				!r.password.is_empty(),
				!r.invite.is_empty()
			),
		)
	}
	async fn leave_channel(&self, r: Request<LeaveChannelRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(
			r.context.as_ref(),
			format!("leave_channel {}", hex::encode(&r.channel_id)),
		)
	}
	async fn list_channels(&self, _: Request<ListChannelsRequest>) -> Resp<ListChannelsResponse> {
		let channels = self.snapshot.lock().unwrap().channels.clone();
		Ok(Response::new(ListChannelsResponse { channels }))
	}
	async fn create_invite(&self, r: Request<CreateInviteRequest>) -> Resp<CreateInviteResponse> {
		let r = r.into_inner();
		self.log.lock().unwrap().push(format!(
			"create_invite {} ttl={} uses={}",
			hex::encode(&r.channel_id),
			r.ttl_seconds,
			r.uses
		));
		Ok(Response::new(CreateInviteResponse {
			invite: "RLYI1-test-token".into(),
			expires_unix: 1_900_000_000,
			operation: None,
		}))
	}
	async fn revoke_channel_member(
		&self,
		r: Request<RevokeChannelMemberRequest>,
	) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(
			r.context.as_ref(),
			format!(
				"revoke {} {}",
				hex::encode(&r.channel_id),
				hex::encode(&r.fingerprint)
			),
		)
	}
	async fn set_privacy(&self, r: Request<SetPrivacyRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(
			r.context.as_ref(),
			format!(
				"privacy inbound={:?} advert={:?}",
				r.inbound_policy, r.advert_mode
			),
		)
	}
	async fn get_avatar(&self, _: Request<GetAvatarRequest>) -> Resp<AvatarImage> {
		unsupported()
	}
	async fn shutdown(&self, r: Request<MutationContext>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(Some(&r), "shutdown".into())
	}
	async fn get_connection_info(&self, _: Request<ClientContext>) -> Resp<ConnectionInfo> {
		Ok(Response::new(ConnectionInfo {
			local_addresses: vec!["192.168.1.20".into()],
			listen_port: 47_800,
			fingerprint: vec![0x0f; 32],
			connection_string: format!("relay://192.168.1.20:47800#{}", "0f".repeat(32)),
		}))
	}

	// Session messaging: `chat::ChatState` holds the semantics, this layer the capability gate.
	async fn list_sessions(&self, r: Request<ClientContext>) -> Resp<ListSessionsResponse> {
		self.chat_resp(Some(&r.into_inner()), |c| c.list())
	}
	async fn get_transcript(&self, r: Request<GetTranscriptRequest>) -> Resp<Transcript> {
		let r = r.into_inner();
		self.chat(r.client.as_ref(), |c| {
			c.transcript(&r.session_id, r.from_ordinal, r.limit)
		})?
		.ok_or_else(|| Status::not_found("unknown session"))
		.map(Response::new)
	}
	type WatchSessionStream = ReceiverStream<Result<SessionEnvelope, Status>>;
	async fn watch_session(
		&self,
		r: Request<WatchSessionRequest>,
	) -> Resp<Self::WatchSessionStream> {
		let r = r.into_inner();
		self.chat(r.client.as_ref(), |_| ())?;
		let (tx, rx) = mpsc::channel(64);
		let incarnation = self.registry.incarnation().to_vec();
		tokio::spawn(chat_watch::serve_watch(
			Arc::clone(&self.chat),
			incarnation,
			r.after_revision,
			r.session_id,
			tx,
		));
		Ok(Response::new(ReceiverStream::new(rx)))
	}
	async fn send_session_message(
		&self,
		r: Request<SendSessionMessageRequest>,
	) -> Resp<SessionOpResult> {
		let r = r.into_inner();
		self.chat_resp(ctx_client(&r.context), |c| {
			c.send_text(&r.session_id, &r.text)
		})
	}
	async fn send_session_image(
		&self,
		r: Request<SendSessionImageRequest>,
	) -> Resp<SessionOpResult> {
		let r = r.into_inner();
		self.chat_resp(ctx_client(&r.context), |c| {
			c.send_image(&r.session_id, r.image.clone())
		})
	}
	async fn retry_session_message(
		&self,
		_: Request<RetrySessionMessageRequest>,
	) -> Resp<SessionOpResult> {
		unsupported()
	}
	async fn offer_file(&self, r: Request<OfferFileRequest>) -> Resp<SessionOpResult> {
		let r = r.into_inner();
		self.chat_resp(ctx_client(&r.context), |c| {
			c.offer_file(&r.session_id, &r.path)
		})
	}
	async fn respond_offer(&self, r: Request<RespondOfferRequest>) -> Resp<SessionOpResult> {
		let r = r.into_inner();
		self.chat_resp(ctx_client(&r.context), |c| {
			c.respond(
				&r.session_id,
				&r.offer_id,
				r.accept,
				&r.destination_dir,
				&r.file_name,
			)
		})
	}
	async fn request_preview(&self, r: Request<OfferRequest>) -> Resp<SessionOpResult> {
		let r = r.into_inner();
		self.chat_resp(ctx_client(&r.context), |c| {
			c.preview(&r.session_id, &r.offer_id, "request")
		})
	}
	async fn approve_preview(&self, r: Request<ApprovePreviewRequest>) -> Resp<SessionOpResult> {
		let r = r.into_inner();
		let what = if r.approve { "approve" } else { "deny" };
		self.chat_resp(ctx_client(&r.context), |c| {
			c.preview(&r.session_id, &r.offer_id, what)
		})
	}
	async fn cancel_transfer(&self, r: Request<OfferRequest>) -> Resp<SessionOpResult> {
		let r = r.into_inner();
		self.chat_resp(ctx_client(&r.context), |c| {
			c.cancel(&r.session_id, &r.offer_id)
		})
	}
	async fn set_session_permissions(
		&self,
		r: Request<SetSessionPermissionsRequest>,
	) -> Resp<SessionOpResult> {
		let r = r.into_inner();
		self.chat_resp(ctx_client(&r.context), |c| {
			c.permissions(&r.session_id, r.allow_messages, r.allow_files)
		})
	}
	async fn pin_message(&self, r: Request<PinMessageRequest>) -> Resp<SessionOpResult> {
		let r = r.into_inner();
		self.chat_resp(ctx_client(&r.context), |c| {
			c.pin(&r.session_id, r.outgoing, r.seq, r.pinned)
		})
	}
	async fn search_session(
		&self,
		r: Request<SearchSessionRequest>,
	) -> Resp<SearchSessionResponse> {
		let r = r.into_inner();
		self.chat(r.client.as_ref(), |c| c.search(&r.session_id, &r.query))?
			.ok_or_else(|| Status::not_found("unknown session"))
			.map(Response::new)
	}
	async fn clear_session(&self, r: Request<ClearSessionRequest>) -> Resp<SessionOpResult> {
		let r = r.into_inner();
		self.chat_resp(ctx_client(&r.context), |c| c.clear(&r.session_id))
	}
	async fn export_session(
		&self,
		r: Request<ExportSessionRequest>,
	) -> Resp<ExportSessionResponse> {
		let r = r.into_inner();
		self.chat(ctx_client(&r.context), |c| {
			c.export(&r.session_id, &r.destination_path)
		})?
		.ok_or_else(|| Status::not_found("unknown session"))
		.map(Response::new)
	}
	async fn set_messaging_settings(
		&self,
		_: Request<SetMessagingSettingsRequest>,
	) -> Resp<SessionOpResult> {
		unsupported()
	}

	async fn list_call_history(&self, _: Request<ListCallHistoryRequest>) -> Resp<CallHistoryPage> {
		Ok(Response::new(CallHistoryPage::default()))
	}
	async fn clear_call_history(&self, r: Request<MutationContext>) -> Resp<Operation> {
		self.op(Some(&r.into_inner()), "clear_call_history".into())
	}
	async fn clear_avatar(&self, r: Request<MutationContext>) -> Resp<Operation> {
		self.op(Some(&r.into_inner()), "clear_avatar".into())
	}
	async fn set_favorites(&self, r: Request<SetFavoritesRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		{
			let mut snapshot = self.snapshot.lock().unwrap();
			for peer in &mut snapshot.peers {
				let place = r.peer_ids.iter().position(|id| *id == peer.peer_id);
				peer.favorite = place.is_some();
				peer.favorite_position = place.unwrap_or(0) as u32;
			}
		}
		let ids: Vec<String> = r.peer_ids.iter().map(|id| hex::encode(&id[..1])).collect();
		self.op(r.context.as_ref(), format!("favorites {}", ids.join(",")))
	}
	async fn forget_peer(&self, r: Request<ForgetPeerRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		let what = format!("forget {} {}", hex::encode(&r.peer_id), r.query);
		self.op(r.context.as_ref(), what)
	}
	async fn replace_pinned_key(&self, r: Request<ReplacePinnedKeyRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		let what = format!(
			"replace {} {}",
			hex::encode(&r.peer_id),
			r.expected_fingerprint
		);
		self.op(r.context.as_ref(), what)
	}
	async fn get_settings(&self, _: Request<ClientContext>) -> Resp<SettingsDump> {
		let values = self.settings.lock().unwrap().clone();
		Ok(Response::new(SettingsDump {
			values: values.into_iter().collect(),
			// A scripted cache of 3 MiB so `relay limits` has something to show.
			usage: [("limits.avatar_cache_mib".to_owned(), 3 * 1024 * 1024)].into(),
			build_revision: "test".into(),
			api_revision: relay_api::API_REVISION - 1,
		}))
	}
	async fn set_settings(&self, r: Request<SetSettingsRequest>) -> Resp<SetSettingsResponse> {
		let r = r.into_inner();
		let mut current = self.settings.lock().unwrap();
		let mut keys: Vec<_> = r.values.iter().collect();
		keys.sort();
		let line: Vec<String> = keys.iter().map(|(k, v)| format!("{k}={v}")).collect();
		self.log
			.lock()
			.unwrap()
			.push(format!("set_settings {}", line.join(",")));
		if let Some((key, _)) = keys
			.iter()
			.find(|(k, v)| super::is_restricted(k) && current.get(*k) != Some(*v))
		{
			return Err(Status::permission_denied(format!(
				"SETTING_RESTRICTED: `{key}` cannot be changed over the API"
			)));
		}
		let results = keys
			.iter()
			.map(|(key, value)| {
				let (outcome, reason) = if !current.contains_key(*key) {
					(SettingOutcome::Rejected, "unknown setting".to_owned())
				} else if key.starts_with("network.") {
					(
						SettingOutcome::SavedNeedsRestart,
						"read at the next start".to_owned(),
					)
				} else {
					(SettingOutcome::AppliedLive, String::new())
				};
				if outcome != SettingOutcome::Rejected {
					current.insert((*key).clone(), (*value).clone());
				}
				SettingResult {
					key: (*key).clone(),
					outcome: outcome as i32,
					reason,
				}
			})
			.collect();
		Ok(Response::new(SetSettingsResponse {
			operation: Some(Operation {
				state: OperationState::Applied as i32,
				..Operation::default()
			}),
			results,
		}))
	}

	type InputControlStream = ReceiverStream<Result<InputControlAck, Status>>;
	async fn input_control(
		&self,
		request: Request<tonic::Streaming<InputControlMessage>>,
	) -> Resp<Self::InputControlStream> {
		let mut incoming = request.into_inner();
		let (tx, rx) = mpsc::channel(8);
		let inputs = self.inputs.clone();
		tokio::spawn(async move {
			while let Ok(Some(message)) = incoming.message().await {
				inputs.lock().unwrap().push(message.clone());
				let ack = InputControlAck {
					source_id: message.source_id,
					input_sequence: message.input_sequence,
					accepted: true,
					effective_lease_ms: message.requested_lease_ms,
					effective_transmitting: true,
					..InputControlAck::default()
				};
				if tx.send(Ok(ack)).await.is_err() {
					break;
				}
			}
		});
		Ok(Response::new(ReceiverStream::new(rx)))
	}
}
