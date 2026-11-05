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
