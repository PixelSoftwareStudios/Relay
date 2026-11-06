# Audio crate (`relay-audio`)

Living status of the audio layer. Update it when behaviour or gates change.

## Domain

Internal audio is mono `f32` at 48 kHz in 480-sample (10 ms) frames. Device
formats (`f32`, `i16`, `u16`, `i32`) are normalised on entry and saturated on
exit; non-finite samples become silence. Capture channel selection is explicit
(`ChannelSelect::Channel(n)` or opt-in `Average`); mono is duplicated to the
device channels only at the output edge.

## Layout

| Module | Role |
|---|---|
| `device` | cpal enumeration with stable keys, `AudioBackend` trait, deterministic `FakeBackend`, stream health and recovery policy |
| `capture` / `playback` | real-time callbacks, SPSC rings (`rtrb`), reader/writer halves, frame assembler, capture chain |
| `convert` | sample formats, channel select/fan-out, `StreamResampler` (rubato sinc, drift clamp +/-300 ppm; `new` is always adjustable even at 48 kHz, `new_fixed` bypasses equal rates and refuses drift) |
| `codec` | per-call Opus encoder/decoder (10 ms, mono, PLC and FEC hooks) |
| `mixer` | per-call and master gain (capped at 2.0), sample-wise limiter, preallocated scratch |
| `dsp` | `NoiseSuppressor` contract, `OffSuppressor`, model manifest validation, benchmark harness |
| `tones` | embedded ring/ringback/blip sounds and the callback-safe `TonePlayer` (see Tones) |
| `pipeline` | single-call loopback wiring every stage together |
| `dsp::DspChain` | fixed-order chain: high-pass, AEC slot, NS, AGC, limiter |
| `dsp::eval` | evaluation-only candidates (features `eval-sonora`, `eval-nnnoiseless`) |

## Real-time rules

Callbacks (`CaptureCallback::process`, `PlaybackCallback::process`) use only
atomics, fixed stack buffers and the wait-free rings. They never allocate,
lock, log or do I/O. Overflow and underflow are counted, never grown through:
the capture ring drops what does not fit and publishes the gap so sample
indices stay exact; playback underflow outputs silence and never replays.
`tests/allocations.rs` runs a counting global allocator over the callbacks,
resampler, codec, mixer, capture chain and a full fake-device loopback step and
asserts zero steady-state allocations.

Worker threads are promoted by `priority::promote_current_thread`, implemented in-tree from OS
documentation (behaviour informed by reviewing the `audio_thread_priority` crate's approach; no code
taken, no MPL obligations). cpal's `realtime` feature is off because it depends on that MPL-2.0
crate; the thread cpal creates for device callbacks is promoted by `priority::promote_callback_thread`
from its first callback (one branch per callback afterwards; a few syscalls once, plus a reason
string if refused; never D-Bus, never waits). Tiers: macOS `THREAD_TIME_CONSTRAINT_POLICY` (period =
buffer, computation = period/4, constraint = period/2, clamped to 0.5-50 ms) then QoS
user-interactive; Windows MMCSS "Pro Audio" then `THREAD_PRIORITY_TIME_CRITICAL`; Linux/Android
`SCHED_RR` at `min(RLIMIT_RTPRIO, 10)`, then RealtimeKit (cargo feature `rtkit`, off by default,
worker threads only), then nice -11. Dropping the guard restores the thread. A refusal is recorded
in `StreamHealth::realtime_denied` (callback thread) or logged (workers) and never pauses the stream.
Policy and timing math are safe code behind the `OsPriority` trait and unit-tested with a fake OS.
Safe versus unsafe: `rustix` (safe) does the `RLIMIT_RTPRIO` check, thread id and nice values;
`unsafe` is confined to `priority/{macos,windows,linux}.rs`: the mach `thread_policy_set` and
QoS calls, the MMCSS and `SetThreadPriority` calls, and one `pthread_setschedparam` (rustix does not
expose `sched_setscheduler`). Each block carries a SAFETY comment. The playback callback publishes its
largest buffer (`PlaybackShared::max_callback_frames`) with a relaxed `fetch_max`.

## Timeline and mute

Capture indices count source samples including muted or lost spans. A mute or
overrun starts a new generation in `CaptureChain`; the reader discards queued
audio while muted so pre-mute audio cannot reappear. Conversion delay is
reported (`delay_samples`) rather than hidden by shifting timestamps.

## Devices

Selection is `FollowDefault` or `Pinned(StableDeviceKey)`; the engine starts both
directions on `FollowDefault` (cpal default input/output; the Android backend reports its
routed device as default), so no user choice is needed. A missing pinned
device resolves to `PinnedMissing` in `Enumeration::resolve`; the engine's device thread then opens
the default for that stream and reports the pin as missing in state, and never picks any other
device. Default changes are noticed by the device thread's 2 s re-scan and at every open; an open
stream is never moved, so a new default applies from the next stream open. Any
lost or invalidated stream maps to `Recovery::PauseAndPrompt`. Handles are
scoped to one enumeration and stale ones are rejected. Real enumeration works
on the development Mac (`cargo test -p relay-audio --test device -- --ignored`).

## Noise suppression

Only the Off baseline exists. `rnnoise` and `dtln` cargo features exist but
`create_suppressor` returns `DspError::BackendUnavailable` until a qualified
artifact and runtime integration exist; no model is vendored. Manifests are
validated against schema version 1 (`ModelManifest::from_json`), including the
extra requirements for `qualified` status. The model file hash itself is not
verified here (no hashing dependency yet).

The `Harness` records real `Instant` timings only; fields it cannot observe
stay `null`. `measure_delay_samples` measures wrapper delay with an impulse.

## DSP chain

`DspChain` runs high-pass -> AEC -> NS -> AGC -> limiter on 480-sample frames in
place. `DspPreferences` defaults enable only the high-pass filter. Each
component declares what it `provides` (`StageKind`); the chain refuses two
providers of the same stage, so a backend that already bundles AGC conflicts
with enabling AGC separately. The AEC slot is a trait with a render-reference
input; only `NoOpAec` exists, and enabling echo cancellation without a real
canceller is an error. The AGC is capped (+12/-12 dB), gated below -50 dBFS so
room noise is not amplified, and relaxes to unity during gated passages.

## Tones

Original sounds generated by `assets/sound/generate.rs` (Apache-2.0, see
`assets/sound/README.md`): `Ring` (E5/A5 cadence, 2.0 s loop), `Ringback`
(dual-tone double burst, 3.0 s loop), `CallEnd` and `Connected` (one-shot
blips). Raw i16 mono 48 kHz, about 200 KB embedded with `include_bytes!`,
decoded once in `TonePlayer::new()`.

API (`relay_audio::tones`): `TonePlayer::new() -> (TonePlayer, ToneControl)`.
`ToneControl` (single producer, rtrb queue of 32) has `start(sound, looping,
volume)`, `stop()`, `set_volume(0.0..=1.0)` and `set_muted(bool)`; invalid
volumes are refused and a full queue returns `ToneError::QueueFull`. The
player's `render_into(&mut [f32])` drains commands and ADDS the tone to the
buffer with no allocation, locks or logging. Start, stop, volume and mute
changes ramp over 10 ms (`FADE_SAMPLES`). Starting what already plays only
updates its volume; starting another sound fades the old one out first; stop
while idle is a no-op; one-shots end by themselves. Muting keeps the position.

Mixing: `Mixer::add_tones(&mut TonePlayer)` is the tone slot. Call it after
every `add` and before `finish`; tones therefore follow master gain and the
limiter, and are part of the playback signal, so they ARE part of the AEC
render reference (feed the reference from the final output). Tones never feed
an encoder or the capture path. Global deafen = `set_muted(true)` (silent after
the ramp).

### Engine integration (wired)

`MediaSystem::start` builds one `TonePlayer` and installs it in the render worker
(`RenderCmd::SetTones`); the `ToneControl` stays behind a mutex in the media
system and is driven through `MediaControl::tone(ToneCommand)` (`Start`, `Stop`,
`Volume`, `Muted`). Every quantum the worker runs `begin(); add(call...);
add_tones(&mut player); finish(out)`, and keeps ticking while a tone plays even
with no call (RT-18). A `Start` opens the playback stream (like a call or the
microphone test does), a looping tone keeps it open, and after a `Stop` (200 ms)
or a one-shot (1.5 s) the stream closes again unless a call or the test needs it.
Tones have no path to capture, encoders or send gates; the tests
`media_tone_playback` assert that a local ring sends zero packets and that the
peer hears only the microphone.

#### Start-up of a ring (no stutter at the start)

The first part of each ring used to stutter. Causes, all fixed:

- The playout FIFO targets 20 ms, but the retarget to the device's callback size
  only ran at the first drift period (500 ms). A device with a 1024-frame callback
  therefore underran for the first half second. The sink now takes the callback
  size the backend announced at open (`ActualConfig::callback_frames`) and retargets
  before the first write (`RenderSink::with_callback_hint`).
- The stream was started before the worker owned it and before any data existed,
  so the first callbacks pulled an empty FIFO. The device thread now fills the FIFO
  with silence up to the target (`RenderSink::prefill`), hands the worker its
  output (`RenderCmd::Attach`) and only then calls `play()`.
- Streams used the device's default buffer. Call, tone and capture streams now
  request `LOW_LATENCY_BUFFER_FRAMES` (512 frames, about 10.7 ms at 48 kHz). cpal
  clamps it to the supported range and uses the default when the range is unknown
  or the stream refuses a fixed size (a warning is logged). AAudio runs at the native
  burst with a buffer of two bursts. `ActualConfig` reports what was granted, and
  the engine logs it at `info` ("playback opened"); the backend-reported output
  latency (cpal's playback timestamp minus callback timestamp) is stored in
  `PlaybackShared::reported_latency_us` and logged once per stream.
- An outgoing call sends `ToneCommand::Prewarm` when it starts preparing, so the
  output is open and prefilled (held 3 s) before the ringback begins. An incoming
  ring cannot be earlier than the invite; it starts on the first worker tick after
  the output is attached, with the 10 ms fade-in. Looping stays inside the player:
  there is no stream restart between ring cycles.

The sounds are `include_bytes!` raw PCM decoded once in `TonePlayer::new`; the
callback side only copies and scales (`tones_are_the_embedded_raw_pcm_decoded_once`),
and tone render, mix, FIFO write and callback pull allocate nothing
(`tone_to_speaker_path_is_allocation_free`, counting allocator on the real
`TonePlayer`); the queues are lock-free `rtrb` rings and the callback never logs.
Measured with the fake backend whose output takes 250 ms to open
(`a_ring_on_a_slow_to_open_output_has_no_underruns_at_the_start`): 962 underrun
frames before the fix, 0 after; the first ring sample comes about 39 ms after the
output finished opening (silent prefill plus the fade-in). The fake playback device
now pulls nothing until `play()`, like hardware.

Which tone plays is decided by `service/tones.rs` (`ToneDriver`, pure) from the
control actor, after every message and tick:

- Incoming call `IncomingRinging`, a client attached (open state watch) and
  `audio.ringtone_enabled`: `Start(Ring, looping, audio.ringtone_volume)`.
- Outgoing call `OutgoingRinging` and `audio.ringback_enabled`:
  `Start(Ringback, looping, 0.7)`. A ring wins over a ringback; they never overlap.
- The loop stops on accept, decline, timeout, remote cancel, end, failure, the
  last client leaving, a disabled preference and shutdown.
- `audio.call_sounds_enabled`: `Connected` when a call becomes `Active` and
  `CallEnd` when a call that was active ends; one-shots at the ring volume that
  never interrupt a ring.
- Global deafen: `Muted(true)` and stop; undeafen: `Muted(false)` (a call still
  ringing then rings again).
- Preference changes arrive through `SetGlobalControls` and apply live: a volume
  change is `Volume(v)`, disabling stops the tone.
- `PreviewTone` (settings "Test ring") plays the ring for 2 s on this device's
  output regardless of `ringtone_enabled`; real tones and deafen override it.

## Evaluation candidates

Neural candidates GTCRN, DTLN and FastEnhancer run through `tract-onnx` behind the `neural` feature of `relay-benches` (weights in the gitignored `benches/models`, hash-checked against committed manifests); nothing in `relay-audio` changed for them.

`eval-sonora` (pure-Rust WebRTC port: NS, NS+AGC2, NS+AEC3) and
`eval-nnnoiseless` (pure-Rust RNNoise port) are not default features and are
not reachable from `create_suppressor`. Measured behaviour, including that
sonora allocates per frame, is in `docs/evidence.md`. Benchmarks and the corpus
runner live in the `relay-benches` crate (`benches/`): a deterministic builder
for a LibriSpeech + DEMAND corpus (local, gitignored under `benches/corpus`),
SI-SDR, STOI and eSTOI (in-repo, unvalidated against a reference), delay by
cross-correlation, per-frame timing, CPU time, and a paced worker-thread test.

## Listening test

`audio-corpus listen` is a blind rating test for your own headphones. It needs the built corpus (`benches/corpus/built`, see `docs/evidence.md`) and whichever candidates you build in (`--features sonora,nnnoiseless,neural`).

```
cargo run --release -p relay-benches --features sonora,nnnoiseless,neural --bin audio-corpus -- \
  listen prepare benches/corpus/built --utterances 4 --conditions snr00,snr10,clean --seed 42
```

This renders every chosen utterance through `off` and each backend into `benches/listening-out/session-<seed>/` (gitignored). Clips are named `trialNN_X.wav`; within a trial the letters are shuffled with the recorded seed, so a name never reveals a backend. Each processed clip is delay-aligned to the original and loudness-matched to it (gain capped at 12 dB), so level and lag do not give a clip away; both corrections are recorded in the key. Open the clips in any player, then edit `answers.csv`: one `rating` (1 bad to 5 excellent) per clip, and an `x` in `preferred` on the one clip you liked best in each trial (optional; at most one per trial). Blank means unrated.

```
... --bin audio-corpus -- listen score benches/listening-out/session-42
```

`score` reads `key/key.json` (do not open it before you are done), rejects malformed sheets, prints the mean rating, rated count and picks per backend and condition, and writes `summary.csv`. It reports partial sheets as partial. The test is a rating-and-pick protocol rather than a strict ABX identification test, and a handful of trials gives only a rough ordering.

## Known limits

- The loopback pipeline requires a suppressor that yields whole 10 ms frames;
  other hops need their own adapter in front of it.
- `PlaybackSink` uses the adjustable resampler, so a 48 kHz device pays the sinc group delay (about 64 samples, reported by `delay_samples`) to keep drift correction available.
- The drift controller in `PlaybackSink::update_drift` is an unvalidated
  starting point.
- Fake devices prove logic, not timing.

## Unmet gates (explicitly not run)

- Long-run (hours) capture/playback on real hardware: not run.
- Callback allocation count on real cpal threads: only the fake-device path is
  proven.
- Effect of the thread promotion on any OS (wake jitter, glitches), Windows timer resolution,
  and which callback sizes real Bluetooth/USB/HDMI devices negotiate: not run.
- Unplug/replug, sleep/resume, Bluetooth and USB-clock-drift acceptance cases:
  not run.
- Mouth-to-ear latency, worker p99 under load, and any suppressor qualification
  benchmark: no measurements exist.
- Neural suppressor qualification: no candidate has a qualified manifest.
