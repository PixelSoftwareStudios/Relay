//! A single-call local loopback used to exercise every stage without a network.

use crate::capture::{CaptureChain, CaptureShared};
use crate::codec::{CallDecoder, CallEncoder, EncoderConfig, MAX_PACKET_BYTES};
use crate::device::{
	AudioBackend, DeviceSelection, Direction, Recovery, Resolution, StreamHandle, StreamHealth,
	StreamRequest, recovery_for,
};
use crate::dsp::{NoiseSuppressor, OffSuppressor};
use crate::error::{AudioError, Result};
use crate::format::{BlockTiming, FRAME_SAMPLES};
use crate::mixer::{CallSlot, Mixer};
use crate::playback::{PlaybackShared, PlaybackSink, playback_sink};
use std::sync::Arc;

/// Decides whether the packet with this sequence number is "lost".
pub type LossFn = Box<dyn FnMut(u64) -> bool + Send>;

pub struct LoopbackConfig {
	pub input: DeviceSelection,
	pub output: DeviceSelection,
	pub request: StreamRequest,
	pub encoder: EncoderConfig,
	pub suppressor: Box<dyn NoiseSuppressor>,
	pub loss: Option<LossFn>,
}

impl LoopbackConfig {
	pub fn new(input: DeviceSelection, output: DeviceSelection) -> Self {
		Self {
			input,
			output,
			request: StreamRequest::default(),
			encoder: EncoderConfig::default(),
			suppressor: Box::new(OffSuppressor::default()),
			loss: None,
		}
	}
}

/// Totals since the pipeline opened.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StepStats {
	pub frames_captured: u64,
	pub frames_encoded: u64,
	pub frames_decoded: u64,
	pub frames_concealed: u64,
	pub frames_unaligned: u64,
	pub encoded_bytes: u64,
	pub last_timing: BlockTiming,
}

pub struct LoopbackPipeline {
	chain: CaptureChain,
	capture_shared: Arc<CaptureShared>,
	capture_health: StreamHealth,
	playback_shared: Arc<PlaybackShared>,
	playback_health: StreamHealth,
	suppressor: Box<dyn NoiseSuppressor>,
	encoder: CallEncoder,
	decoder: CallDecoder,
	mixer: Mixer,
	slot: CallSlot,
	sink: PlaybackSink,
	loss: Option<LossFn>,
	packet: Vec<u8>,
	denoised: Vec<f32>,
	decoded: Vec<f32>,
	mixed: Vec<f32>,
	sequence: u64,
	steps: u64,
	stats: StepStats,
	_streams: [Box<dyn StreamHandle>; 2],
}

impl LoopbackPipeline {
	/// Resolve devices, open and start both streams. Allocates everything.
	pub fn open(backend: &mut dyn AudioBackend, config: LoopbackConfig) -> Result<Self> {
		let enumeration = backend.enumerate()?;
		let resolve = |direction, selection: &DeviceSelection| match enumeration
			.resolve(direction, selection)
		{
			Resolution::Device(handle) => Ok(handle),
			Resolution::NoDefault => Err(AudioError::DeviceNotFound),
			Resolution::PinnedMissing => Err(AudioError::SelectedDeviceMissing),
		};
		let input = resolve(Direction::Input, &config.input)?;
		let output = resolve(Direction::Output, &config.output)?;
		let capture = backend.open_capture(input, &config.request)?;
		let playback = backend.open_playback(output, &config.request)?;

		let mut suppressor = config.suppressor;
		suppressor
			.prepare(&crate::dsp::DspLimits {
				max_block_samples: FRAME_SAMPLES,
				max_scratch_bytes: suppressor.descriptor().max_scratch_bytes,
			})
			.map_err(|e| AudioError::Backend(e.to_string()))?;
		let mut mixer = Mixer::new(FRAME_SAMPLES);
		let slot = mixer
			.open_slot()
			.ok_or(AudioError::InvalidArgument("no mixer slot"))?;
		let sink = playback_sink(playback.writer, playback.config.sample_rate)?;
		capture.handle.play()?;
		playback.handle.play()?;
		Ok(Self {
			chain: CaptureChain::new(capture.reader, capture.config.sample_rate)?,
			capture_shared: capture.shared,
			capture_health: capture.health,
			playback_shared: playback.shared,
			playback_health: playback.health,
			suppressor,
			encoder: CallEncoder::new(config.encoder)?,
			decoder: CallDecoder::new()?,
			mixer,
			slot,
			sink,
			loss: config.loss,
			packet: vec![0; MAX_PACKET_BYTES],
			denoised: vec![0.0; FRAME_SAMPLES * 2],
			decoded: vec![0.0; FRAME_SAMPLES],
			mixed: vec![0.0; FRAME_SAMPLES],
			sequence: 0,
			steps: 0,
			stats: StepStats::default(),
			_streams: [capture.handle, playback.handle],
		})
	}

	pub fn stats(&self) -> StepStats {
		self.stats
	}

	pub fn capture_shared(&self) -> &CaptureShared {
		&self.capture_shared
	}

	pub fn playback_shared(&self) -> &PlaybackShared {
		&self.playback_shared
	}

	pub fn mixer_mut(&mut self) -> &mut Mixer {
		&mut self.mixer
	}

	pub fn slot(&self) -> CallSlot {
		self.slot
	}

	/// Recovery signal: any lost stream pauses and asks the user.
	pub fn recovery(&self) -> Recovery {
		let capture = recovery_for(Direction::Input, self.capture_health.state());
		let playback = recovery_for(Direction::Output, self.playback_health.state());
		if capture == Recovery::PauseAndPrompt || playback == Recovery::PauseAndPrompt {
			Recovery::PauseAndPrompt
		} else {
			Recovery::Continue
		}
	}

	/// Run every stage over whatever has been captured. Steady state is
	/// allocation-free.
	pub fn step(&mut self) -> Result<StepStats> {
		if self.recovery() == Recovery::PauseAndPrompt {
			return Ok(self.stats);
		}
		let Self {
			chain,
			suppressor,
			encoder,
			decoder,
			mixer,
			slot,
			sink,
			loss,
			packet,
			denoised,
			decoded,
			mixed,
			sequence,
			stats,
			..
		} = self;
		let mut failure = None;
		chain.poll(|frame, timing| {
			if failure.is_some() {
				return;
			}
			if let Err(e) = frame_through(
				frame, timing, suppressor, encoder, decoder, mixer, *slot, sink, loss, packet,
				denoised, decoded, mixed, sequence, stats,
			) {
				failure = Some(e);
			}
		})?;
		if let Some(e) = failure {
			return Err(e);
		}
		self.steps += 1;
		if self.steps.is_multiple_of(50) {
			self.sink.update_drift()?;
		}
		Ok(self.stats)
	}
}

#[allow(clippy::too_many_arguments)]
fn frame_through(
	frame: &[f32],
	timing: BlockTiming,
	suppressor: &mut Box<dyn NoiseSuppressor>,
	encoder: &mut CallEncoder,
	decoder: &mut CallDecoder,
	mixer: &mut Mixer,
	slot: CallSlot,
	sink: &mut PlaybackSink,
	loss: &mut Option<LossFn>,
	packet: &mut [u8],
	denoised: &mut [f32],
	decoded: &mut [f32],
	mixed: &mut [f32],
	sequence: &mut u64,
	stats: &mut StepStats,
) -> Result<()> {
	stats.frames_captured += 1;
	stats.last_timing = timing;
	let progress = suppressor
		.process_into(frame, denoised, timing)
		.map_err(|e| AudioError::Backend(e.to_string()))?;
	// No hidden aggregation: a backend that cannot yield a whole 10 ms frame
	// per frame needs its own adapter in front of this pipeline.
	if progress.produced != FRAME_SAMPLES {
		stats.frames_unaligned += 1;
		return Ok(());
	}
	let len = encoder.encode(&denoised[..FRAME_SAMPLES], packet)?;
	stats.frames_encoded += 1;
	stats.encoded_bytes += len as u64;
	let seq = *sequence;
	*sequence += 1;
	if loss.as_mut().is_some_and(|lost| lost(seq)) {
		decoder.conceal(decoded)?;
		stats.frames_concealed += 1;
	} else {
		decoder.decode(&packet[..len], decoded)?;
		stats.frames_decoded += 1;
	}
	mixer.begin();
	mixer.add(slot, decoded)?;
	mixer.finish(mixed)?;
	sink.write(mixed)
}
