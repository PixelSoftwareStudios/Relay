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

## Timeline and mute

Capture indices count source samples including muted or lost spans. A mute or
overrun starts a new generation in `CaptureChain`; the reader discards queued
audio while muted so pre-mute audio cannot reappear. Conversion delay is
reported (`delay_samples`) rather than hidden by shifting timestamps.

## Devices

Selection is `FollowDefault` or `Pinned(StableDeviceKey)`. A missing pinned
device resolves to `PinnedMissing`; there is no automatic fallback, and any
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

## Evaluation candidates

`eval-sonora` (pure-Rust WebRTC port: NS, NS+AGC2, NS+AEC3) and
`eval-nnnoiseless` (pure-Rust RNNoise port) are not default features and are
not reachable from `create_suppressor`. Measured behaviour, including that
sonora allocates per frame, is in `docs/evidence.md`. Benchmarks and the corpus
runner live in the `relay-benches` crate (`benches/`).

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
- Unplug/replug, sleep/resume, Bluetooth and USB-clock-drift acceptance cases:
  not run.
- Mouth-to-ear latency, worker p99 under load, and any suppressor qualification
  benchmark: no measurements exist.
- Neural suppressor qualification: no candidate has a qualified manifest.
