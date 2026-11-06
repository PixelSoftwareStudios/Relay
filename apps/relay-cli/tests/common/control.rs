//! A scripted `Control`/`Subscribe` that records what it was asked to do.

use std::collections::{BTreeMap, VecDeque};

use relay_api::pb::*;
use relay_api::{ApiError, WatchItem};
use relay_cli::client::*;

use super::fixtures;

/// Runs when a session watch opens: what the engine did while the stream was down.
pub type WatchSetup = Box<dyn FnOnce(&mut super::chat::ChatState) + Send>;

/// Items a scripted stream yields, then `Ok(None)`.
pub struct Script<T>(pub VecDeque<Result<T, ApiError>>);

impl<T: Send> Feed for Script<T> {
	type Item = T;
	async fn next(&mut self) -> Result<Option<T>, ApiError> {
		self.0.pop_front().transpose()
	}
}

pub struct FakeControl {
	pub info: EngineInfo,
	pub snapshot: Snapshot,
	pub log: Vec<String>,
	pub result: Operation,
	/// One entry per `open_watch` call; an exhausted list makes it fail.
	pub watches: VecDeque<Vec<Result<WatchItem, ApiError>>>,
	pub telemetry: Vec<TelemetrySnapshot>,
	/// What `open_call_stats` yields, then the stream ends.
	pub call_stats: Vec<CallStats>,
	/// Secrets that reached the engine side (never part of `log`).
	pub secrets: Vec<String>,
	/// When set, `create_invite` answers with a FAILED operation and no token.
	pub invite_failure: Option<Problem>,
	/// Session messaging semantics shared with the socket engine.
	pub chat: super::chat::ChatState,
	/// One entry per `open_session_watch`; an exhausted list opens a watch holding only the list.
	pub on_watch_open: VecDeque<WatchSetup>,
	pub session_watches: VecDeque<Vec<Result<relay_api::SessionItem, ApiError>>>,
	/// What `get_settings` answers.
	pub settings: BTreeMap<String, String>,
	/// Scripted per-key answers of `set_settings` (default: applied live).
	pub setting_results: BTreeMap<String, SettingResult>,
	/// What the call history holds, newest first.
	pub history: Vec<CallHistoryEntry>,
}

impl FakeControl {
	pub fn new() -> Self {
		Self {
			info: EngineInfo {
				application_version: "9.9.9".into(),
				api_major: 1,
				..EngineInfo::default()
			},
			snapshot: fixtures::snapshot(),
			log: Vec::new(),
			result: Operation {
				operation_id: vec![9; 16],
				state: OperationState::Applied as i32,
				revision: 11,
				..Operation::default()
			},
			watches: VecDeque::new(),
			telemetry: Vec::new(),
			call_stats: Vec::new(),
			secrets: Vec::new(),
			invite_failure: None,
			chat: super::chat::ChatState::new(),
			on_watch_open: VecDeque::new(),
			session_watches: VecDeque::new(),
			settings: BTreeMap::new(),
			setting_results: BTreeMap::new(),
			history: Vec::new(),
		}
	}

	fn record(&mut self, line: impl Into<String>) -> Result<Operation, ApiError> {
		self.log.push(line.into());
		Ok(self.result.clone())
	}
}

impl Control for FakeControl {
	fn info(&self) -> &EngineInfo {
		&self.info
	}
	async fn snapshot(&mut self) -> Result<Snapshot, ApiError> {
		Ok(self.snapshot.clone())
	}
	async fn controller_generation(&mut self, call_id: &[u8]) -> Result<u64, ApiError> {
		self.log.push(format!("lease {}", hex::encode(call_id)));
		Ok(5)
	}
	async fn start_call(&mut self, target: CallTarget) -> Result<Operation, ApiError> {
		self.record(format!("start_call {target:?}"))
	}
	async fn call_action(
		&mut self,
		action: CallAction,
		call_id: &[u8],
	) -> Result<Operation, ApiError> {
		self.record(format!("{action:?} {}", hex::encode(call_id)))
	}
	async fn set_transmit_mode(
		&mut self,
		call_id: &[u8],
		mode: TransmitMode,
	) -> Result<Operation, ApiError> {
		self.record(format!("route {} {mode:?}", hex::encode(call_id)))
	}
	async fn set_call_controls(
		&mut self,
		call_id: &[u8],
		c: CallControls,
	) -> Result<Operation, ApiError> {
		self.record(format!("call_controls {} {c:?}", hex::encode(call_id)))
	}
	async fn set_global_controls(&mut self, c: GlobalControls) -> Result<Operation, ApiError> {
		self.record(format!("global_controls {c:?}"))
	}
	async fn select_devices(&mut self, input: &str, output: &str) -> Result<Operation, ApiError> {
		self.record(format!("select_devices {input} {output}"))
	}
	async fn set_noise_suppression(&mut self, backend: &str) -> Result<Operation, ApiError> {
		self.record(format!("ns {backend}"))
	}
	async fn set_profile(&mut self, name: &str) -> Result<Operation, ApiError> {
		self.record(format!("profile {name}"))
	}
	async fn start_mic_test(
		&mut self,
		local: bool,
		confirmed: bool,
	) -> Result<Operation, ApiError> {
		self.record(format!(
			"mic_test_start local={local} confirmed={confirmed}"
		))
	}
	async fn stop_mic_test(&mut self) -> Result<Operation, ApiError> {
		self.record("mic_test_stop")
	}
	async fn connect_and_trust(&mut self, connection_string: &str) -> Result<Operation, ApiError> {
		self.record(format!("connect_and_trust {connection_string}"))
	}
	async fn decide_trust(&mut self, d: TrustDecision) -> Result<Operation, ApiError> {
		self.record(format!(
			"trust {} {:?} oob={} key={}",
			hex::encode(&d.peer_id),
			d.requested,
			d.out_of_band_verified,
			hex::encode(&d.public_key_sha256)
		))
	}
	async fn ptt(&mut self, request: PttRequest) -> Result<(), ApiError> {
		self.log.push(format!(
			"ptt {:?} gen={}",
			request.action, request.route_generation
		));
		Ok(())
	}
}

impl Subscribe for FakeControl {
	type Watch = Script<WatchItem>;
	type Telemetry = Script<TelemetrySnapshot>;
	type CallStats = Script<CallStats>;

	async fn open_watch(&mut self) -> Result<Self::Watch, ApiError> {
		let items = self.watches.pop_front().ok_or(ApiError::StreamClosed)?;
		Ok(Script(items.into()))
	}

	async fn open_call_stats(&mut self, call_id: &[u8]) -> Result<Self::CallStats, ApiError> {
		self.log
			.push(format!("call_stats {}", hex::encode(call_id)));
		Ok(Script(
			self.call_stats.clone().into_iter().map(Ok).collect(),
		))
	}

	async fn open_telemetry(&mut self, _: u32) -> Result<Self::Telemetry, ApiError> {
		Ok(Script(self.telemetry.drain(..).map(Ok).collect()))
	}
}

impl Manage for FakeControl {
	async fn connection_info(&mut self) -> Result<ConnectionInfo, ApiError> {
		Ok(ConnectionInfo {
			local_addresses: vec!["192.168.1.20".into()],
			listen_port: 47_800,
			fingerprint: vec![0x0f; 32],
			connection_string: format!("relay://192.168.1.20:47800#{}", "0f".repeat(32)),
		})
	}
	async fn start_call_string(&mut self, s: &str) -> Result<Operation, ApiError> {
		self.record(format!("start_call_string {s}"))
	}
	async fn set_privacy(
		&mut self,
		inbound: Option<InboundPolicy>,
		advert: Option<AdvertMode>,
	) -> Result<Operation, ApiError> {
		self.record(format!("privacy {inbound:?} {advert:?}"))
	}
	async fn list_channels(&mut self) -> Result<Vec<ChannelInfo>, ApiError> {
		Ok(self.snapshot.channels.clone())
	}
	async fn create_channel(
		&mut self,
		name: &str,
		policy: ChannelPolicy,
		password: Option<&relay_cli::secret::Secret>,
	) -> Result<Operation, ApiError> {
		if let Some(p) = password {
			self.secrets.push(p.expose().to_owned());
		}
		self.record(format!(
			"create_channel {name} listed={} password={} invite_only={}",
			policy.listed,
			password.is_some(),
			policy.invite_only
		))
	}
	async fn join_channel(
		&mut self,
		target: ChannelRef,
		credential: Credential,
	) -> Result<Operation, ApiError> {
		let kind = match &credential {
			Credential::None => "none",
			Credential::Password(p) => {
				self.secrets.push(p.expose().to_owned());
				"password"
			}
			Credential::Invite(i) => {
				self.secrets.push(i.expose().to_owned());
				"invite"
			}
		};
		self.record(format!("join_channel {target:?} {kind}"))
	}
	async fn leave_channel(&mut self, id: &[u8]) -> Result<Operation, ApiError> {
		self.record(format!("leave_channel {}", hex::encode(id)))
	}
	async fn create_invite(
		&mut self,
		id: &[u8],
		ttl: u32,
		uses: u32,
	) -> Result<CreateInviteResponse, ApiError> {
		self.log
			.push(format!("create_invite {} {ttl} {uses}", hex::encode(id)));
		if let Some(problem) = self.invite_failure.clone() {
			return Ok(CreateInviteResponse {
				invite: String::new(),
				expires_unix: 0,
				operation: Some(Operation {
					state: OperationState::Failed as i32,
					problem: Some(problem),
					..Operation::default()
				}),
			});
		}
		Ok(CreateInviteResponse {
			invite: "RLYI1-test-token".into(),
			expires_unix: 1_900_000_000,
			operation: None,
		})
	}
	async fn revoke_member(
		&mut self,
		id: &[u8],
		fingerprint: &[u8],
	) -> Result<Operation, ApiError> {
		self.record(format!(
			"revoke {} {}",
			hex::encode(id),
			hex::encode(fingerprint)
		))
	}
	async fn upload_avatar(
		&mut self,
		bytes: Vec<u8>,
		media_type: &str,
	) -> Result<Operation, ApiError> {
		self.record(format!("avatar {} {media_type}", bytes.len()))
	}
	async fn clear_avatar(&mut self) -> Result<Operation, ApiError> {
		self.record("clear_avatar")
	}
	async fn forget_peer(&mut self, peer_id: &[u8], query: &str) -> Result<Operation, ApiError> {
		self.record(format!("forget {} {query}", hex::encode(peer_id)))
	}
	async fn set_favorites(&mut self, peer_ids: &[Vec<u8>]) -> Result<Operation, ApiError> {
		let ids: Vec<String> = peer_ids.iter().map(|id| hex::encode(&id[..1])).collect();
		self.record(format!("favorites {}", ids.join(",")))
	}
	async fn call_history(
		&mut self,
		limit: u32,
		before_id: Option<u64>,
	) -> Result<CallHistoryPage, ApiError> {
		self.log.push(format!("call_history {limit} {before_id:?}"));
		let entries: Vec<_> = self
			.history
			.iter()
			.filter(|e| before_id.is_none_or(|b| e.id < b))
			.take(limit as usize)
			.cloned()
			.collect();
		Ok(CallHistoryPage {
			entries,
			more: false,
			revision: 1,
		})
	}
	async fn clear_call_history(&mut self) -> Result<Operation, ApiError> {
		self.history.clear();
		self.record("clear_call_history")
	}
	async fn get_settings(&mut self) -> Result<BTreeMap<String, String>, ApiError> {
		Ok(self.settings.clone())
	}
	async fn set_settings(
		&mut self,
		values: Vec<(String, String)>,
	) -> Result<Vec<SettingResult>, ApiError> {
		self.log.push(format!("set_settings {values:?}"));
		let mut results: Vec<SettingResult> = values
			.into_iter()
			.map(|(key, _)| {
				self.setting_results
					.get(&key)
					.cloned()
					.unwrap_or(SettingResult {
						outcome: SettingOutcome::AppliedLive as i32,
						reason: String::new(),
						key,
					})
			})
			.collect();
		results.sort_by(|a, b| a.key.cmp(&b.key));
		Ok(results)
	}
}

impl Messaging for FakeControl {
	type Watch = Script<relay_api::SessionItem>;

	async fn list_sessions(&mut self) -> Result<ListSessionsResponse, ApiError> {
		Ok(self.chat.list())
	}
	async fn get_transcript(
		&mut self,
		session: &[u8],
		from: u64,
		limit: u32,
	) -> Result<Transcript, ApiError> {
		self.chat
			.transcript(session, from, limit)
			.ok_or_else(|| ApiError::Status(Box::new(tonic::Status::not_found("unknown session"))))
	}
	async fn open_session_watch(
		&mut self,
		_: Option<&[u8]>,
		after: Option<u64>,
	) -> Result<Self::Watch, ApiError> {
		self.chat.log.push(format!("watch after={after:?}"));
		if let Some(setup) = self.on_watch_open.pop_front() {
			setup(&mut self.chat);
		}
		let items = self.session_watches.pop_front().unwrap_or_else(|| {
			let list = self.chat.list();
			vec![Ok(relay_api::SessionItem::List(SessionList {
				sessions: list.sessions,
				settings: list.settings,
			}))]
		});
		Ok(Script(items.into()))
	}
	async fn send_text(&mut self, s: &[u8], text: &str) -> Result<SessionOpResult, ApiError> {
		Ok(self.chat.send_text(s, text))
	}
	async fn send_image(&mut self, s: &[u8], image: Vec<u8>) -> Result<SessionOpResult, ApiError> {
		Ok(self.chat.send_image(s, image))
	}
	async fn retry_message(&mut self, s: &[u8], _: u64) -> Result<SessionOpResult, ApiError> {
		Ok(self.chat.send_text(s, "retry"))
	}
	async fn offer_file(&mut self, s: &[u8], path: &str) -> Result<SessionOpResult, ApiError> {
		Ok(self.chat.offer_file(s, path))
	}
	async fn respond_offer(
		&mut self,
		s: &[u8],
		offer: &[u8],
		accept: bool,
		dir: &str,
		name: &str,
	) -> Result<SessionOpResult, ApiError> {
		Ok(self.chat.respond(s, offer, accept, dir, name))
	}
	async fn request_preview(&mut self, s: &[u8], o: &[u8]) -> Result<SessionOpResult, ApiError> {
		Ok(self.chat.preview(s, o, "request"))
	}
	async fn approve_preview(
		&mut self,
		s: &[u8],
		o: &[u8],
		approve: bool,
	) -> Result<SessionOpResult, ApiError> {
		Ok(self
			.chat
			.preview(s, o, if approve { "approve" } else { "deny" }))
	}
	async fn cancel_offer(&mut self, s: &[u8], o: &[u8]) -> Result<SessionOpResult, ApiError> {
		Ok(self.chat.cancel(s, o))
	}
	async fn set_permissions(
		&mut self,
		s: &[u8],
		messages: Option<bool>,
		files: Option<bool>,
	) -> Result<SessionOpResult, ApiError> {
		Ok(self.chat.permissions(s, messages, files))
	}
	async fn pin(
		&mut self,
		s: &[u8],
		outgoing: bool,
		seq: u64,
		pinned: bool,
	) -> Result<SessionOpResult, ApiError> {
		Ok(self.chat.pin(s, outgoing, seq, pinned))
	}
	async fn search(&mut self, s: &[u8], query: &str) -> Result<SearchSessionResponse, ApiError> {
		self.chat
			.search(s, query)
			.ok_or_else(|| ApiError::Status(Box::new(tonic::Status::not_found("unknown session"))))
	}
	async fn clear(&mut self, s: &[u8]) -> Result<SessionOpResult, ApiError> {
		Ok(self.chat.clear(s))
	}
	async fn export(
		&mut self,
		s: &[u8],
		_: bool,
		dest: Option<&str>,
	) -> Result<ExportSessionResponse, ApiError> {
		self.chat
			.export(s, dest.unwrap_or_default())
			.ok_or_else(|| ApiError::Status(Box::new(tonic::Status::not_found("unknown session"))))
	}
}
