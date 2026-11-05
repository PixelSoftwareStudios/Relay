# Evidence log

Only measurements from real runs are recorded here. Every entry states the
command, versions and platform. Absent entries mean the run has not happened.

> The 2026-11-04 synthetic-corpus entries below predate the real corpus, the improved delay estimator and the in-repo STOI; read them as machinery checks only.

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
computed at the time: no vetted Rust STOI crate was found, so the field stayed `null` (an in-repo STOI/eSTOI was added later).

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

- Real-device mouth-to-ear latency, long-run hardware tests, p99 under CPU load, PESQ/DNSMOS, and validation of the in-repo STOI against a reference implementation.

## 2026-11-05: real-speech comparison (LibriSpeech + DEMAND)

> **Partly superseded by the follow-up section below.** The plain SI-SDR figures for sonora in this section are invalid (sonora applies a frequency-dependent delay, which a single-lag SI-SDR cannot absorb); the sonora interpretation bullet about an unexplained anomaly is resolved there. STOI/eSTOI, CPU and per-frame timing figures stand, apart from the per-frame maxima explained below.

### Setup (facts)

- Platform: Apple M1 Pro (10 cores), macOS 14.7.2, aarch64; rustc 1.99.1; release profile (thin LTO, codegen-units 1).
- Code: relay-benches at commit 2f88bb7 (HEAD 028a683 when the results were read).
- Candidates: `off`; `sonora` 0.2.0 NS-only, NS+AGC2, NS+AEC3 (silent render reference); `nnnoiseless` 0.5.2.
- Source data (local, gitignored, read-only): LibriSpeech dev-clean FLAC (CC BY 4.0) and DEMAND 16 kHz scenes DKITCHEN, DLIVING, STRAFFIC (CC BY 4.0), channel `ch01.wav` of each. Archive SHA-256 values match `benches/corpus/SHA256SUMS` (verified with `shasum -a 256 -c`):
  - 76f87d090650617fca0cac8f88b9416e0ebf80350acb97b343a85fa903728ab3  librispeech/dev-clean.tar.gz
  - 9d68e46d2709847c59580770e2f8aa160607ebff7475788174aa1080332cc845  demand/DKITCHEN_16k.zip
  - 2b1726fe06e41551ce2397f2aaf3e4fb692c912d81914b04708df0bbb5252338  demand/DLIVING_16k.zip
  - 2df9b444afb3b789df92d96cb6f58a50653cdbbbe7e7e74fcd9de2af6385fbea  demand/STRAFFIC_16k.zip
- Commands:
  1. `cargo run --release -p relay-benches --features sonora,nnnoiseless --bin audio-corpus -- build-real benches/corpus benches/corpus/built`
  2. `target/release/audio-corpus run benches/corpus/built --out results.json`
  3. `target/release/audio-corpus worker <backend> 30` for each of sonora-ns, sonora-ns-agc, sonora-ns-aec, nnnoiseless
- Corpus build: seed 0x52454c415931. 8 speakers at evenly spaced sorted indices of the 40 dev-clean speakers, 3 utterances each (5-12 s, evenly spread over the speaker's sorted usable list) = 24 utterances. Each is resampled 16 -> 48 kHz and mixed at -5, 0, 5, 10, 20 dB SNR with a seeded random segment of a DEMAND scene (scene = (utterance + snr index) mod 3), scaled against the whole-utterance RMS, saturating at full scale. A clean-only copy of each utterance (noisy = clean) detects speech damage. Total 144 pairs. The DEMAND noise was 16 kHz band-limited, so noise has no energy above 8 kHz.
- `manifest.json` (written by the builder) lists every file, SHA-256, noise offset and SNR; its own SHA-256 is **20e1693195ede93dccdbab41992d79197dd20d5d3c263f5990b8d37e78399c1c**; two builds produced the same hash (test `builder_is_deterministic_and_complete`).
- Metrics: SI-SDR vs the clean reference after shifting the output by the cross-correlation delay; first 0.5 s skipped. STOI / eSTOI are this repo's implementation from the published algorithms (10 kHz, 15 third-octave bands, 30-frame segments), unit-tested for identical => 1, level invariance, silence removal, monotone fall with SNR; **not validated against a reference implementation**. No PESQ/DNSMOS. Input columns are the unprocessed noisy mix scored the same way.
- Selected utterances (index, speaker, id, SHA-256 of the FLAC):

```
00 1272 1272-128104-0010.flac 2ebbcaa44f14c15fd8afd0e4a71cae494377d30812a0ba05d6be07c55d00bbea
01 1272 1272-141231-0004.flac 34c4339d921ca363faa292996ea12864c9bbdb5a69fa40178e41629f069a36e0
02 1272 1272-141231-0026.flac 78bbbfada3676e97548fe946e252cfffc5fba916542d2c6e39d5ecbb8be76710
03 1988 1988-147956-0017.flac b396d83002bae45a473f7c1753ec7f5c19dff72b5a0c77a7d506a097691fa82b
04 1988 1988-148538-0008.flac b5ef8617d5ed3bd4295dfb700fa4c6d4267bc5095b72101e5a4449e675de3bf1
05 1988 1988-24833-0009.flac 60c2a4766f6845fc9225676bc3ddad8ff7f46dff1cb01ad64dc0359c75db0283
06 2277 2277-149874-0010.flac c81d812b30eab9f374d8384b1f761cda8a115772b7fa2304d2388826c2c0f007
07 2277 2277-149896-0008.flac fff4a9ac06e5a85348e1d12a498da564c7de250917e8733577b86f64cec7198c
08 2277 2277-149897-0017.flac 0c55b36ae9861122d3b4e45b4d8e427bacf9ecbd289536dc6fc0d3eb4d8a2cb7
09 2902 2902-9006-0009.flac 94edb9c03e5cbcfa7ad42d63d4c355517793240a26dfd69e594c5bfa3e91e592
10 2902 2902-9008-0004.flac ac2a7ad8bf87b4593c2a2af83e5ea643804b116d54c9790216f7f46405016724
11 2902 2902-9008-0011.flac 391451534c3fbdc1e94c3bd3027445535422d91c685db00bdbf465d1a4ba907a
12 3576 3576-138058-0015.flac 6a1f6529a1cf9a9b137479558c4a871f52842559f3b6032585e5d73a5d3209e5
13 3576 3576-138058-0025.flac 4e4e3a674633f1aa901ce16dffe2f31f4c2661e006e65724af58f682466e884d
14 3576 3576-138058-0034.flac 3c0a527c977839b84d3501bbb0df466cf998da8213e8602cb0a0eb8dddcb88b9
15 5536 5536-43358-0007.flac b8a3160cbb9ffba51e10397baea6e24032f3a7fe080a88fa281d45959c958094
16 5536 5536-43359-0009.flac cad867e630b5b62363e381609c6b16afc2d025d51853bc45edd83a64a74774f4
17 5536 5536-43363-0010.flac 017083d38c940b67c359f78773aa7c2202dc5257723ba04bd21b4d5e156f3814
18 6313 6313-66125-0025.flac 79246631aadffc3ac58f5728f22e0d0c45f8ad988c2c263c985cf1319b44fbf3
19 6313 6313-66129-0026.flac a9361ad88c6a8e700bb12234df549be94dafaf1041a20cfb5a2ad4cd74cdba05
20 6313 6313-76958-0014.flac 03bcd7eb474689f9b3793baabe699e44a27d347e442a00e864a8720e25190ed1
21 7850 7850-111771-0008.flac 1d9219725dc7a77e7c102a504cef5edd07cefabe397ca82a0d95f2d54e984a37
22 7850 7850-281318-0021.flac 30bfe662d9cad792b40f7e3a0debd17a01888b5c64e0c8ec8b9e1199550a32b3
23 7850 7850-73752-0002.flac 92169c023ffa056d35a447c406e33f1f161b1754d9a706b3b63673f2b1100172
noise kitchen demand/extracted/DKITCHEN_16k/DKITCHEN/ch01.wav 94eda18f9461348d1d070ca69d001e7fa9e51604731509026b4905d2e321561d
noise living demand/extracted/DLIVING_16k/DLIVING/ch01.wav 9567ff039c040359534075d575b469b587b1e3858ef48a93acd3ecba95f57210
noise traffic demand/extracted/STRAFFIC_16k/STRAFFIC/ch01.wav 8fa49dcf3996af655a3ecef8d06f0d9ef217c35410e3adce3d461da797987dbb
```

### Results (facts; means over 24 files per condition)

Timing is per 480-sample frame over all 144 files (about 109416 frames); CPU is process CPU seconds per audio second.

| Backend | p50 | p99 | max | CPU (cores) | delay by xcorr, samples (min / median / max) | allocs/frame |
|---|---|---|---|---|---|---|
| off | 0.0 us | 0.2 us | 49 us | 0.00001 | 0 / 0 / 0 | 0 |
| sonora-ns | 17.8 us | 25.9 us | 2117 us | 0.00181 | 304 / 331 / 508 | 1 |
| sonora-ns-agc | 19.3 us | 40.1 us | 10785 us | 0.00197 | 304 / 331 / 508 | 8 |
| sonora-ns-aec | 62.8 us | 97.2 us | 38914 us | 0.00671 | 688 / 715 / 892 | about 40 (capture+render) |
| rnnoise-nnnoiseless | 41.6 us | 93.4 us | 23160 us | 0.00425 | 479 / 480 / 480 | 0 |

**SI-SDR (dB)**, input / output / change by condition:

| Backend | snrm05 | snr00 | snr05 | snr10 | snr20 | clean |
|---|---|---|---|---|---|---|
| off | -4.7 / -4.7 / +0.0 | 0.2 / 0.3 / +0.0 | 5.3 / 5.3 / +0.0 | 10.3 / 10.3 / +0.0 | 20.2 / 20.2 / +0.0 | n/a / n/a / n/a |
| sonora-ns | -4.7 / -4.2 / +0.4 | 0.2 / -1.1 / -1.3 | 5.3 / -0.8 / -6.0 | 10.3 / -0.8 / -11.1 | 20.2 / -0.7 / -20.9 | n/a / -0.8 / n/a |
| sonora-ns-agc | -4.7 / -4.2 / +0.5 | 0.2 / -1.1 / -1.3 | 5.3 / -0.8 / -6.0 | 10.3 / -0.8 / -11.1 | 20.2 / -0.7 / -20.9 | n/a / -0.8 / n/a |
| sonora-ns-aec | -4.7 / -4.2 / +0.4 | 0.2 / -1.1 / -1.3 | 5.3 / -0.8 / -6.0 | 10.3 / -0.8 / -11.1 | 20.2 / -0.7 / -20.9 | n/a / -0.8 / n/a |
| rnnoise-nnnoiseless | -4.7 / 5.2 / +9.9 | 0.2 / 9.6 / +9.4 | 5.3 / 12.2 / +6.9 | 10.3 / 13.9 / +3.5 | 20.2 / 16.8 / -3.5 | n/a / 17.4 / n/a |

**STOI**, input / output / change by condition:

| Backend | snrm05 | snr00 | snr05 | snr10 | snr20 | clean |
|---|---|---|---|---|---|---|
| off | 0.780 / 0.780 / +0.000 | 0.879 / 0.879 / +0.000 | 0.914 / 0.914 / +0.000 | 0.949 / 0.949 / +0.000 | 0.991 / 0.991 / +0.000 | 1.000 / 1.000 / +0.000 |
| sonora-ns | 0.780 / 0.770 / -0.010 | 0.879 / 0.863 / -0.016 | 0.914 / 0.892 / -0.022 | 0.949 / 0.926 / -0.023 | 0.991 / 0.968 / -0.022 | 1.000 / 0.980 / -0.020 |
| sonora-ns-agc | 0.780 / 0.770 / -0.010 | 0.879 / 0.863 / -0.016 | 0.914 / 0.892 / -0.022 | 0.949 / 0.926 / -0.023 | 0.991 / 0.968 / -0.022 | 1.000 / 0.980 / -0.020 |
| sonora-ns-aec | 0.780 / 0.770 / -0.010 | 0.879 / 0.863 / -0.016 | 0.914 / 0.892 / -0.022 | 0.949 / 0.926 / -0.023 | 0.991 / 0.969 / -0.022 | 1.000 / 0.980 / -0.020 |
| rnnoise-nnnoiseless | 0.780 / 0.789 / +0.008 | 0.879 / 0.879 / +0.000 | 0.914 / 0.910 / -0.004 | 0.949 / 0.945 / -0.004 | 0.991 / 0.982 / -0.009 | 1.000 / 0.992 / -0.008 |

**eSTOI**, input / output / change by condition:

| Backend | snrm05 | snr00 | snr05 | snr10 | snr20 | clean |
|---|---|---|---|---|---|---|
| off | 0.569 / 0.569 / +0.000 | 0.714 / 0.714 / +0.000 | 0.792 / 0.792 / +0.000 | 0.866 / 0.866 / +0.000 | 0.971 / 0.971 / +0.000 | 1.000 / 1.000 / +0.000 |
| sonora-ns | 0.569 / 0.573 / +0.005 | 0.714 / 0.714 / -0.000 | 0.792 / 0.780 / -0.013 | 0.866 / 0.847 / -0.019 | 0.971 / 0.935 / -0.036 | 1.000 / 0.959 / -0.041 |
| sonora-ns-agc | 0.569 / 0.573 / +0.005 | 0.714 / 0.714 / -0.000 | 0.792 / 0.780 / -0.013 | 0.866 / 0.847 / -0.019 | 0.971 / 0.935 / -0.036 | 1.000 / 0.959 / -0.041 |
| sonora-ns-aec | 0.569 / 0.572 / +0.004 | 0.714 / 0.714 / -0.000 | 0.792 / 0.779 / -0.013 | 0.866 / 0.847 / -0.019 | 0.971 / 0.935 / -0.036 | 1.000 / 0.959 / -0.041 |
| rnnoise-nnnoiseless | 0.569 / 0.585 / +0.016 | 0.714 / 0.738 / +0.024 | 0.792 / 0.809 / +0.016 | 0.866 / 0.876 / +0.010 | 0.971 / 0.958 / -0.013 | 1.000 / 0.982 / -0.018 |

### Worker-thread pacing (facts)

`audio-corpus worker <backend> 30`: one worker thread, a frame enqueued every 10 ms of wall clock for 30 s (3000 frames; synthetic 5 dB white-noise mix; AEC fed silence); latency is enqueue to output ready. Normal-priority thread, machine not otherwise controlled.

| Backend | p50 | p99 | max | frames > 5 ms | frames > 10 ms |
|---|---|---|---|---|---|
| off (baseline) | 9 us | 4061 us | 21970 us | 20 | 4 |
| sonora NS | 39 us | 1802 us | 8162 us | 6 | 0 |
| sonora NS+AGC | 42 us | 1452 us | 12352 us | 9 | 1 |
| sonora NS+AEC | 113 us | 2641 us | 18806 us | 8 | 3 |
| nnnoiseless | 66 us | 326 us | 14695 us | 5 | 2 |

### Allocations re-check (facts)

`cargo test --release -p relay-audio --features eval-sonora,eval-nnnoiseless --test eval -- --nocapture`: sonora NS 100 allocations over 100 frames; NS+AGC 800 over 100; NS+AEC 2025 over 50 capture+render pairs; nnnoiseless 0 (asserted). Unchanged from the earlier check.

### Interpretation (not measured facts)

- **nnnoiseless** raises waveform SNR (SI-SDR +9.4, +6.9, +3.5 dB at 0, 5, 10 dB SNR, +9.9 dB at -5 dB) but lowers it at 20 dB (-3.5 dB) and alters clean speech (17.4 dB SI-SDR vs the clean original). STOI is essentially unchanged (within 0.01) and eSTOI moves by about +0.02 at low SNR and -0.02 on clean. On this evidence it removes noise energy without a measurable intelligibility gain, and it is not transparent on clean speech.
- **sonora** (all three profiles) shows STOI 0.02-0.04 lower than the unprocessed input and an SI-SDR near -0.7 dB at every input SNR including clean. SI-SDR that does not depend on input SNR means the output is not waveform-aligned with the clean reference; the cause was not established here (possibilities: fractional or time-varying delay in its band-split pipeline, wrapper configuration, or the NS itself). The delay estimate is now consistent (about 331 samples for NS, about 715 with AEC enabled, versus 480 for nnnoiseless), so a gross misalignment is unlikely but sub-sample alignment was not checked. No conclusion about sonora noise-reduction quality should be drawn from SI-SDR alone; STOI shows no intelligibility gain.
- NS+AGC rows equal NS-only rows because SI-SDR and STOI are gain-invariant; the AGC effect is not visible in these metrics. The AEC profile's extra delay (about 384 samples over NS-only) is a cost of enabling it.
- Compute cost is small for every candidate (p99 <= 97 us per 10 ms frame; <= 0.7% of a core). The single-frame maxima of 2-39 ms in the corpus run exceed the "off" maximum (49 us); cause not investigated (first-call initialisation or preemption are untested guesses).
- On a dedicated normal-priority worker the Off baseline itself misses 10 ms four times in 3000 frames, so deadline misses come from thread scheduling, not candidate compute. A worker for these candidates fits the frame budget by compute but needs real-time thread priority and a jitter buffer margin before it can be trusted; this was not tried.
- Sonora's per-frame allocations remain, so it belongs on a worker thread, never in a device callback.

### Limits

DEMAND noise here is 16 kHz-sampled scenes (no content above 8 kHz); only channel 1 of each scene; dev-clean read speech is not conversational; one machine; no PESQ or DNSMOS; STOI/eSTOI unvalidated against a reference implementation.

## 2026-11-05 (follow-up): sonora SI-SDR anomaly, STOI validation, timing outliers, rerun

Same platform as above (Apple M1 Pro, macOS 14.7.2, rustc 1.99.1, release). Corpus and manifest unchanged (manifest SHA-256 20e1693195ede93dccdbab41992d79197dd20d5d3c263f5990b8d37e78399c1c).

### A. Why sonora's SI-SDR was about -0.7 dB (facts)

Ad-hoc scratch program (not in the repo) feeding clean utterance u05 (1988-24833-0009) through sonora with different configs, aligning by the best integer lag and reporting correlation (corr), energy ratio and plain SI-SDR at that lag:

| Config | best lag | corr | out/in energy | SI-SDR at lag |
|---|---|---|---|---|
| nothing enabled | 0 | 1.0000 | 1.000 | exact copy (bypass) |
| high-pass only | 42 | 0.863 | 0.927 | 4.7 dB |
| NS Low / Moderate / High / VeryHigh (32 kHz internal, the default) | 328 | 0.757 | 0.92-0.94 | 1.3 dB (all four levels) |
| NS Low, 48 kHz internal | 331 | 0.793 | 0.868 | 2.3 dB |
| nnnoiseless | 480 | 0.996 | 0.999 | 21.0 dB |

Per-band cross-spectrum between input and output at the best lag (NS Low, default config): energy per band is preserved within about 1 dB below 6 kHz, but the cross-spectrum **phase is not zero**: +159 deg below 190 Hz, +72 deg at 190-375 Hz, +17 deg at 375-750 Hz, -14 deg at 0.75-1.5 kHz, -70 deg at 1.5-3 kHz, about 170 deg at 3-6 kHz. With the high-pass alone the phase is +103, +48, +11, -10, -48, -133 deg in the same bands. For nnnoiseless the phase is 12.5 deg below 190 Hz and 0-7 deg elsewhere (a pure delay). Changing the lag by one sample changes sonora's SI-SDR by about 0.1 dB, so it is not a sample-alignment error.

Verified cause: sonora's processing (with the NS, and with the high-pass alone) has a frequency-dependent group delay; there is no single lag that aligns the whole spectrum, so plain SI-SDR, which assumes one constant delay, collapses even though energy and (per STOI) intelligibility are preserved. It is not a sign flip, a gain/level change, a frame-size or channel mismatch (a bypass config round-trips exactly), or an NS level that gates everything (all four levels give the same result). nnnoiseless is a pure delay, so its SI-SDR was valid and its constant 480-sample lag is not hiding the same issue. The harness was at fault for using a constant-delay metric on a non-constant-delay processor.

Harness fix: `filtered_sdr_db` (BSS-eval style) lets a 448-tap linear filter (224 taps each side of the aligned position, central 2.5 s) map the clean reference to the output before measuring distortion. It is unit-tested (a known 3-tap response scores above 60 dB where plain SI-SDR stays below 12 dB; unrelated noise scores below 5 dB). It is reported as `fsdr_*` next to the plain `si_sdr_*`. Caveat: allowing a filter makes the metric slightly optimistic for every backend including the unprocessed input (e.g. 9.6 dB for a 10 dB mix); improvements are comparable between backends, absolute values are not comparable to plain SI-SDR. In the `clean` column the unprocessed input has no distortion and is capped at 120 dB, so only the output value is meaningful there.

### B. STOI / eSTOI validation against pystoi (facts)

Reference: pystoi 0.4.1 with numpy 2.5.4, scipy 1.18.1, soundfile 0.14.0, Python 3.14.0, in a virtualenv in the session scratchpad. Ten built corpus pairs (clean vs noisy mix, spread over utterances, SNRs and noise scenes), both implementations at 48 kHz input:

| Pair | pystoi STOI / eSTOI | ours | abs diff |
|---|---|---|---|
| u00 snr10 kitchen | 0.9858 / 0.9307 | 0.9859 / 0.9289 | 0.0001 / 0.0018 |
| u02 snr-5 traffic | 0.7486 / 0.4755 | 0.7483 / 0.4708 | 0.0003 / 0.0047 |
| u05 snr0 kitchen | 0.9535 / 0.8489 | 0.9537 / 0.8519 | 0.0002 / 0.0030 |
| u07 snr10 living | 0.9643 / 0.8743 | 0.9649 / 0.8770 | 0.0005 / 0.0026 |
| u09 snr-5 kitchen | 0.8240 / 0.6924 | 0.8256 / 0.6973 | 0.0017 / 0.0049 |
| u12 snr0 living | 0.8486 / 0.7020 | 0.8510 / 0.7074 | 0.0024 / 0.0053 |
| u14 snr10 traffic | 0.8693 / 0.6931 | 0.8660 / 0.6909 | 0.0033 / 0.0022 |
| u16 snr-5 living | 0.7910 / 0.5703 | 0.7893 / 0.5669 | 0.0017 / 0.0033 |
| u19 snr0 traffic | 0.8111 / 0.5421 | 0.8128 / 0.5416 | 0.0017 / 0.0004 |
| u21 snr10 kitchen | 0.9979 / 0.9903 | 0.9981 / 0.9910 | 0.0002 / 0.0007 |

Maximum absolute difference: **0.0033 (STOI) and 0.0053 (eSTOI)**, below the 0.01 target, so no change was made. Interpretation: the residual is plausibly from the different 48 -> 10 kHz resampler; it was not isolated. Only unprocessed mixes were compared, not processed or delayed outputs.

### C. Per-frame timing outliers (facts, then interpretation)

Facts:
- Single process, one file, three consecutive passes with a warm candidate (`audio-corpus outliers <backend> <wav> plain|qos`, 651 frames per pass): no frame above 1 ms for off, sonora NS, sonora NS+AEC or nnnoiseless, with or without user-interactive QoS. The largest frame in each run was frame 0 of pass 0 (first touch): off 11 us, sonora NS 132 us, sonora NS+AEC 93 us, nnnoiseless 201 us (one run each, other runs lower).
- Full corpus (109,416 frames per backend), plain: frames above 1 ms: off 0, sonora NS 7, NS+AGC 2, NS+AEC 7, nnnoiseless 9; maxima 4 / 13783 / 3379 / 3646 / 11140 us. With the main thread at user-interactive QoS: 0, 27, 1, 83, 154 frames; maxima 12 / 30726 / 2127 / 29035 / 53663 us. p50 is unchanged (17.7-18.1, 19.3, 62.5-63.3, 41.6-41.7 us).
- Paced worker (30 s, 3000 frames), one run each: plain, off 2 frames above 5 ms and 0 above 10 ms; sonora NS 7 and 2; nnnoiseless 2 and 1. With user-interactive QoS: off 5 and 2; sonora NS 11 and 1; nnnoiseless 13 and 4. (An earlier run of the plain worker had off at 20 above 5 ms and 4 above 10 ms; run-to-run variation is large.)

Interpretation: the slow frames are rare (at most 0.008% of frames in the plain run), land at random positions, are not tied to the first frame or to warm-up, occur in the allocation-free nnnoiseless as much as in allocating sonora, and are absent from `off` only because its timed region is about 50 ns long; this is consistent with the operating system preempting the thread now and then (more often the longer the timed call), not with a compute or allocator cost. Requesting user-interactive QoS did not reduce them in this experiment (it looked worse, from single runs). A pinned or time-constraint real-time thread was not tried (macOS has no simple core pinning); until that is tested, assume a normal-priority worker can miss a 10 ms deadline a few times per several thousand frames and size the playout buffer for it.

### D. Rerun of the real-corpus comparison (facts)

Command: `target/release/audio-corpus run benches/corpus/built --out run_plain.json` (and with `--qos on`), built with `--features sonora,nnnoiseless`. Per-frame timing over all 144 files:

| Backend | p50 | p99 | max | frames > 1 ms | CPU (cores) | xcorr delay (min / median / max) |
|---|---|---|---|---|---|---|
| off | 0.0 us | 0.1 us | 4 us | 0 of 109416 | 0.00001 | 0 / 0 / 0 |
| sonora-ns | 17.7 us | 25.6 us | 13783 us | 7 of 109416 | 0.00180 | 304 / 331 / 508 |
| sonora-ns-agc | 19.3 us | 23.9 us | 3379 us | 2 of 109416 | 0.00194 | 304 / 331 / 508 |
| sonora-ns-aec | 62.5 us | 83.8 us | 3646 us | 7 of 109416 | 0.00664 | 688 / 715 / 892 |
| rnnoise-nnnoiseless | 41.6 us | 54.5 us | 11140 us | 9 of 109416 | 0.00419 | 479 / 480 / 480 |

Same run with the main thread at user-interactive QoS (`--qos on`):

| Backend | p50 | p99 | max | frames > 1 ms | CPU (cores) | xcorr delay (min / median / max) |
|---|---|---|---|---|---|---|
| off | 0.0 us | 0.1 us | 12 us | 0 of 109416 | 0.00001 | 0 / 0 / 0 |
| sonora-ns | 18.1 us | 35.2 us | 30726 us | 27 of 109416 | 0.00184 | 304 / 331 / 508 |
| sonora-ns-agc | 19.3 us | 31.5 us | 2127 us | 1 of 109416 | 0.00195 | 304 / 331 / 508 |
| sonora-ns-aec | 63.3 us | 133.3 us | 29035 us | 83 of 109416 | 0.00682 | 688 / 715 / 892 |
| rnnoise-nnnoiseless | 41.7 us | 110.9 us | 53663 us | 154 of 109416 | 0.00429 | 479 / 480 / 480 |

**Filtered SDR (dB), 448-tap linear filter allowed**, unprocessed input / output / change (means of 24 files):

| Backend | snrm05 | snr00 | snr05 | snr10 | snr20 | clean |
|---|---|---|---|---|---|---|
| off | -5.2 / -5.2 / +0.0 | 0.1 / 0.1 / +0.0 | 4.6 / 4.6 / +0.0 | 9.6 / 9.6 / +0.0 | 19.5 / 19.5 / +0.0 | 120.0 / 120.0 / -0.0 |
| sonora-ns | -5.2 / 6.1 / +11.3 | 0.1 / 12.3 / +12.3 | 4.6 / 14.0 / +9.4 | 9.6 / 14.9 / +5.3 | 19.5 / 19.7 / +0.2 | 120.0 / 21.5 / -98.5 |
| sonora-ns-agc | -5.2 / 6.1 / +11.3 | 0.1 / 12.3 / +12.3 | 4.6 / 14.0 / +9.4 | 9.6 / 14.9 / +5.3 | 19.5 / 19.7 / +0.2 | 120.0 / 21.5 / -98.5 |
| sonora-ns-aec | -5.2 / 6.0 / +11.2 | 0.1 / 12.4 / +12.3 | 4.6 / 14.1 / +9.5 | 9.6 / 15.0 / +5.4 | 19.5 / 19.8 / +0.3 | 120.0 / 21.7 / -98.3 |
| rnnoise-nnnoiseless | -5.2 / 6.4 / +11.5 | 0.1 / 11.3 / +11.3 | 4.6 / 13.8 / +9.1 | 9.6 / 15.8 / +6.2 | 19.5 / 21.6 / +2.1 | 120.0 / 24.7 / -95.3 |

**Plain SI-SDR (dB), single constant delay**, unprocessed input / output / change (means of 24 files):

| Backend | snrm05 | snr00 | snr05 | snr10 | snr20 | clean |
|---|---|---|---|---|---|---|
| off | -4.7 / -4.7 / +0.0 | 0.2 / 0.3 / +0.0 | 5.3 / 5.3 / +0.0 | 10.3 / 10.3 / +0.0 | 20.2 / 20.2 / +0.0 | n/a / n/a / n/a |
| sonora-ns | -4.7 / -4.2 / +0.4 | 0.2 / -1.1 / -1.3 | 5.3 / -0.8 / -6.0 | 10.3 / -0.8 / -11.1 | 20.2 / -0.7 / -20.9 | n/a / -0.8 / n/a |
| sonora-ns-agc | -4.7 / -4.2 / +0.5 | 0.2 / -1.1 / -1.3 | 5.3 / -0.8 / -6.0 | 10.3 / -0.8 / -11.1 | 20.2 / -0.7 / -20.9 | n/a / -0.8 / n/a |
| sonora-ns-aec | -4.7 / -4.2 / +0.4 | 0.2 / -1.1 / -1.3 | 5.3 / -0.8 / -6.0 | 10.3 / -0.8 / -11.1 | 20.2 / -0.7 / -20.9 | n/a / -0.8 / n/a |
| rnnoise-nnnoiseless | -4.7 / 5.2 / +9.9 | 0.2 / 9.6 / +9.4 | 5.3 / 12.2 / +6.9 | 10.3 / 13.9 / +3.5 | 20.2 / 16.8 / -3.5 | n/a / 17.4 / n/a |

**STOI**, unprocessed input / output / change (means of 24 files):

| Backend | snrm05 | snr00 | snr05 | snr10 | snr20 | clean |
|---|---|---|---|---|---|---|
| off | 0.780 / 0.780 / +0.000 | 0.879 / 0.879 / +0.000 | 0.914 / 0.914 / +0.000 | 0.949 / 0.949 / +0.000 | 0.991 / 0.991 / +0.000 | 1.000 / 1.000 / +0.000 |
| sonora-ns | 0.780 / 0.770 / -0.010 | 0.879 / 0.863 / -0.016 | 0.914 / 0.892 / -0.022 | 0.949 / 0.926 / -0.023 | 0.991 / 0.968 / -0.022 | 1.000 / 0.980 / -0.020 |
| sonora-ns-agc | 0.780 / 0.770 / -0.010 | 0.879 / 0.863 / -0.016 | 0.914 / 0.892 / -0.022 | 0.949 / 0.926 / -0.023 | 0.991 / 0.968 / -0.022 | 1.000 / 0.980 / -0.020 |
| sonora-ns-aec | 0.780 / 0.770 / -0.010 | 0.879 / 0.863 / -0.016 | 0.914 / 0.892 / -0.022 | 0.949 / 0.926 / -0.023 | 0.991 / 0.969 / -0.022 | 1.000 / 0.980 / -0.020 |
| rnnoise-nnnoiseless | 0.780 / 0.789 / +0.008 | 0.879 / 0.879 / +0.000 | 0.914 / 0.910 / -0.004 | 0.949 / 0.945 / -0.004 | 0.991 / 0.982 / -0.009 | 1.000 / 0.992 / -0.008 |

**eSTOI**, unprocessed input / output / change (means of 24 files):

| Backend | snrm05 | snr00 | snr05 | snr10 | snr20 | clean |
|---|---|---|---|---|---|---|
| off | 0.569 / 0.569 / +0.000 | 0.714 / 0.714 / +0.000 | 0.792 / 0.792 / +0.000 | 0.866 / 0.866 / +0.000 | 0.971 / 0.971 / +0.000 | 1.000 / 1.000 / +0.000 |
| sonora-ns | 0.569 / 0.573 / +0.005 | 0.714 / 0.714 / -0.000 | 0.792 / 0.780 / -0.013 | 0.866 / 0.847 / -0.019 | 0.971 / 0.935 / -0.036 | 1.000 / 0.959 / -0.041 |
| sonora-ns-agc | 0.569 / 0.573 / +0.005 | 0.714 / 0.714 / -0.000 | 0.792 / 0.780 / -0.013 | 0.866 / 0.847 / -0.019 | 0.971 / 0.935 / -0.036 | 1.000 / 0.959 / -0.041 |
| sonora-ns-aec | 0.569 / 0.572 / +0.004 | 0.714 / 0.714 / -0.000 | 0.792 / 0.779 / -0.013 | 0.866 / 0.847 / -0.019 | 0.971 / 0.935 / -0.036 | 1.000 / 0.959 / -0.041 |
| rnnoise-nnnoiseless | 0.569 / 0.585 / +0.016 | 0.714 / 0.738 / +0.024 | 0.792 / 0.809 / +0.016 | 0.866 / 0.876 / +0.010 | 0.971 / 0.958 / -0.013 | 1.000 / 0.982 / -0.018 |

### E. Interpretation of the rerun (not measured facts)

- With a metric that tolerates sonora's frequency-dependent delay, **sonora NS gives a noise-reduction gain of about +11.3, +12.3, +9.4, +5.3 and +0.2 dB filtered SDR at -5, 0, 5, 10 and 20 dB SNR**, roughly the same as nnnoiseless (+11.5, +11.3, +9.1, +6.2, +2.1). The earlier statement that sonora showed no usable gain came from the invalid SI-SDR and should be disregarded.
- On clean speech both degrade the signal (output filtered SDR 21.5 dB for sonora, 24.7 dB for nnnoiseless, versus an exact input), and the STOI of clean speech falls by 0.020 (sonora) and 0.008 (nnnoiseless).
- STOI: sonora lowers it by 0.010 to 0.023 at every SNR; nnnoiseless is within +0.008 to -0.009. eSTOI: sonora +0.005 to -0.036 (worse at high SNR), nnnoiseless +0.024 to -0.013 (better at low SNR). So neither improves intelligibility by these measures, while both reduce noise energy. These are objective proxies on read speech with stationary-ish noise; they do not establish listening quality.
- sonora's NS+AGC and NS+AEC (silent render) numbers equal NS-only because these metrics are gain-invariant and there is no echo in the corpus; the AEC profile mostly adds about 384 samples of delay (715 vs 331) and 3.7x the CPU.
- Per-frame delay is stable per backend now (sonora NS 304-508, median 331; the spread is a property of the frequency-dependent delay and of speech content, not measured further).

## 2026-11-05: coverage-guided fuzzing of untrusted parsers

Toolchain: `rustc 1.101.0-nightly (2d20b42b8 2026-11-04)` (rustup minimal profile), `cargo-fuzz 0.13.2`, libFuzzer with ASAN, macOS aarch64. Command per target, run from `fuzz/`, with the corpus in a scratch directory outside the repository (seeded from `fuzz/corpus/<target>/` where one exists):

`cargo +nightly fuzz run <target> <scratch-corpus> -- -max_total_time=240`

Each target ran once for 241 s, four targets at a time in parallel on a 10-core machine. `exec/s` is the final figure while sharing the machine.

| Target | Executions | cov / features | Final corpus | Peak rss | Crashes |
|---|---|---|---|---|---|
| rtp_parse | 101,863,105 | 43 / 43 | 9 files, 1.1 KB | 446 MB | none |
| record_header | 7,130,615 | 45 / 45 | 8 files, 225 B | 106 MB | none |
| control_envelope | 6,583,700 | 1351 / 4858 | 1549 files, 93 KB | 438 MB | none |
| media_datagram | 6,599,547 | 71 / 71 | 10 files, 1.8 KB | 407 MB | none |
| channel_wire | 7,002,269 | 566 / 2194 | 670 files, 72 KB | 479 MB | none |
| channel_text | 6,575,148 | 525 / 1561 | 437 files, 31 KB | 452 MB | none |
| manual_endpoint | 7,773,127 | 374 / 814 | 288 files, 25 KB | 156 MB | none |
| avatar_decode | 1,169,104 | 3403 / 8790 | 1013 files, 165 KB | 448 MB | none |

Findings and caveats:

- No crash, panic, assertion failure, out-of-memory or timeout in any target; the artifact directory stayed empty.
- The `avatar_decode` target did not compile when first built (it used a removed `Avatar.png` field; the field is `image`). Fixed in commit `8fb3c46`. Before this run the targets had only been type-checked, and this one was not even that.
- The rtp_parse, record_header and media_datagram logs of the first batch were overwritten by a duplicated launch, so those three were rerun cleanly; the table shows the clean reruns.
- Resident memory of about 440 MB in rtp_parse is libFuzzer/ASAN overhead, not a parser leak: it was 441 MB after 2 M executions and 446 MB after 102 M.
- `media_datagram` exercises only rejection of unsealed bytes against one fixed session; its coverage plateaued at 71 edges within seconds, so it cannot reach the authenticated-record paths. `rtp_parse`, `record_header` and `manual_endpoint` also plateaued early, which suggests small input spaces but is not proof of exhaustion.
- `avatar_decode` is the slowest (about 4.9 k exec/s) and still gaining coverage when it stopped; four minutes is far too short for an image decoder.
- Seeds are a handful of hand-made files under `fuzz/corpus/` for five targets; they are guesses at valid inputs and were not checked to parse successfully.

This is a smoke run of 4 minutes per target. It is not a clean bill of health: no dictionary, no long run, no sanitizer other than ASAN, no differential or stateful fuzzing, and the control_envelope, channel_wire, channel_text and avatar_decode corpora were still growing.
