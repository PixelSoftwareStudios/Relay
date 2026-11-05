# Evidence log

Only measurements from real runs are recorded here. Every entry states the
command, versions and platform. Absent entries mean the run has not happened.

## 2026-11-04: audio stage microbenchmarks (criterion)

- Platform: Apple M1 Pro, macOS 14.7.2, aarch64; rustc 1.99.1; release profile (thin LTO).
- Source: `benches/audio/stages.rs` at commit 37d785c (relay-benches).
- Command: `cargo bench -p relay-benches --bench audio_stages -- --warm-up-time 1 --measurement-time 2`
- Short measurement windows; machine was not otherwise idle-controlled.

| Benchmark (10 ms frame, warm state) | Time (low / mid / high) |
|---|---|
| opus_encode_10ms (32 kbit/s, complexity 5) | 48.3 / 48.9 / 50.0 us |
| opus_decode_10ms | 8.88 / 8.95 / 9.06 us |
| resample_44k1_to_48k_10ms | 11.6 / 11.8 / 12.0 us |
| mixer_4_calls_10ms (incl. limiter) | 4.76 / 4.79 / 4.84 us |
| high_pass_10ms | 2.12 / 2.18 / 2.25 us |
| agc_10ms | 0.994 / 1.013 / 1.036 us |
| suppressor_off_10ms | 25.7 / 25.7 / 25.8 ns |

## 2026-11-04: suppressor comparison on a synthetic corpus

- Platform: Apple M1 Pro, macOS 14.7.2, aarch64; rustc 1.99.1; release profile.
- Candidates: `off` (relay-audio 0.1.0), `sonora` 0.2.0 (BSD-3-Clause, pure Rust:
  NS only / NS+AGC2 / NS+AEC3 profiles), `nnnoiseless` 0.5.2 (BSD-3-Clause,
  pure-Rust RNNoise port, weights embedded in the crate).
- Commands:
  - `cargo run --release -p relay-benches --features sonora,nnnoiseless --bin audio-corpus -- generate <dir>`
  - `cargo run --release -p relay-benches --features sonora,nnnoiseless --bin audio-corpus -- run <dir> --out results.json`
- Corpus: 6 deterministic pairs, 8 s each, 48 kHz mono: a synthetic harmonic
  "speech-like" signal (gliding pitch, 3 Hz syllable envelope) mixed with
  white / brown / mains-hum noise at 0 dB and 10 dB SNR. **This is not speech.**
- Per-frame timing is wall time around `process_into` for one 480-sample
  frame; CPU is process CPU time over the whole file divided by audio duration.
  Values below are means over the 6 files (max is the worst frame seen).

| Backend | Impulse delay | p50 / p99 / max per frame | CPU (cores) |
|---|---|---|---|
| off | 0 samples | 0.04 / 0.83 / 3.4 us | 0.00002 |
| sonora NS | impulse suppressed (none found) | 17.9 / 36.1 / 183 us | 0.0018 |
| sonora NS+AGC | none found | 19.1 / 41.8 / 326 us | 0.0020 |
| sonora NS+AEC (silent render) | none found | 63.2 / 79.9 / 161 us | 0.0065 |
| nnnoiseless | none found | 40.6 / 62.4 / 114 us | 0.0041 |

Cross-correlation delay (noisy input vs output, search +/-960 samples):
off 0; nnnoiseless 476-480 samples (about 10 ms, consistent in every file);
sonora 455-941 samples with values piled against the search limit, which means
the estimate is not trustworthy for that output (see below).

SI-SDR vs clean reference (after alignment, first 0.5 s skipped):

| Backend | Input SI-SDR | Output SI-SDR | Change |
|---|---|---|---|
| off | 0 / 10 dB | identical | 0 |
| nnnoiseless | about 0 / 10 dB | 5.7 to 10.2 / 11.4 to 13.2 dB | +1.4 to +10.4 dB |
| sonora (all profiles) | about 0 / 10 dB | about -4 to -24 dB | negative |

**Interpretation limits.** Passing the clean synthetic signal alone through
sonora NS also yields about -22 dB SI-SDR, i.e. WebRTC NS treats this
stationary harmonic signal as noise and removes it. The sonora rows therefore
reflect a mismatch between the probe and the algorithm, not a verdict on its
quality on speech, and its delay estimate is unreliable here. A real speech
corpus is required before any quality or delay claim. nnnoiseless is also
not guaranteed to behave like speech-trained use on this probe. STOI was not
computed: no vetted Rust STOI implementation was found (crates.io search
returned none), so the field stays `null`.

## 2026-11-04: steady-state allocations of evaluation candidates

- Test: `cargo test -p relay-audio --features eval-sonora,eval-nnnoiseless --test eval -- --nocapture`
  (thread-local counting global allocator, white-noise frames, after warm-up).

| Candidate | Allocations |
|---|---|
| nnnoiseless | 0 over 100 frames (asserted) |
| sonora NS | 100 over 100 frames (1 per frame) |
| sonora NS+AGC | 800 over 100 frames (8 per frame) |
| sonora NS+AEC | 2025 over 50 capture+render frame pairs (about 40 per pair) |

Sonora therefore does not meet the zero-allocation callback target as shipped;
it would need to run on a media worker, not in a device callback, or be fixed
upstream.

## 2026-11-04: C++ WebRTC wrapper build probe

- `webrtc-audio-processing` 2.1.0 with feature `bundled`, in a scratch project
  on the same Mac: `cargo build` succeeded in about 62 s using the installed
  meson and ninja (autoconf/automake absent and not needed). Not integrated,
  because sonora covers NS, AEC3 and AGC2 in pure Rust. No timings taken.

## Not run

- Real speech corpus comparison, real-device mouth-to-ear latency, long-run
  hardware tests, p99 under CPU load, and STOI.
