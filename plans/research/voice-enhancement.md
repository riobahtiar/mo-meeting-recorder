# Research: voice enhancement

Researched 2026-09-26 for a user toggle, "voice enhancement": take out wind, traffic, keyboard, hum and room noise, and make voices clearer and fuller, closer to a good studio microphone, on the mic track and ideally on the computer-audio track too. Latency is not critical, so both a live filter in the capture helper and a post-process at Stop were considered.

Facts carry a link to their primary source. "Measured here" marks numbers from the small benchmark in [Local benchmark](#local-benchmark), run on this machine (Apple M1 Pro, macOS 27.0 SDK, Homebrew ffmpeg 9.0.2). Anything I could not confirm from a primary source is marked **unverified**.

## Summary

**Recommendation: Apple's `AUSoundIsolation` audio unit as a post-process at Stop, followed by a short ffmpeg voice chain in `export.rs`. It needs no new crate, no model download and no bundle growth.**

1. **Denoise with `AUSoundIsolation`** (`kAudioUnitSubType_AUSoundIsolation`, `'vois'`). It is a system audio unit on macOS 13 and later, with a standard voice model (`kAUSoundIsolationSoundType_Voice`, macOS 13) and a "high quality voice" model (`kAUSoundIsolationSoundType_HighQualityVoice`, macOS 15), plus a wet/dry mix parameter (MacOSX27.0.sdk `AudioToolbox/AUComponent.h` and `AudioUnitParameters.h`; https://developer.apple.com/documentation/audiotoolbox/kaudiounitsubtype_ausoundisolation). It runs in the Swift helper, where Apple APIs belong (AGENTS.md). The helper would get a new subcommand, for example `momr-audio enhance <in.raw> <out.raw>`. That subcommand reads the s16le 48 kHz stereo staging track, renders it offline through `AVAudioEngine` manual rendering, and writes the same format back. Measured here, it processed a 94.5 s clip in about 1.4 s of wall time and 1.3 to 1.4 s of CPU time (RTF about 0.015). Signal quality came out close to the best neural candidates, and whisper large-v3-turbo accuracy did not change.
2. **Then shape the voice with ffmpeg filters** that ship in the Homebrew build the app already bundles. The chain is `highpass=f=80` (rumble, wind, handling noise), a gentle `equalizer` cut around 200 to 300 Hz (mud) and a lift around 3 to 4 kHz (presence), `deesser`, and a gentle `acompressor`. It ends with the existing `volume` gain from `speech_gain_db` and `alimiter`. These filters exist in the bundled build (measured here: `ffmpeg -filters`) and cost under 0.3 s for 94.5 s of audio. `speech_gain_db` has to be computed on the enhanced track, because its 95th-percentile level includes the noise the enhancer took out.
3. **Keep the original tracks.** `.tracks/` keeps the raw (unenhanced) Opus tracks, and transcription keeps reading the raw audio by default. Enhancement changes only the file the user listens to. That matches the primary evidence that denoising can hurt modern ASR (see [Risks](#risks)). It also keeps the feature reversible, and it leaves the on-disk formats untouched (AGENTS.md: formats are interfaces).

Why post-process and not live: the helper's mic path is `AVAudioEngine`, so the unit could be inserted live, but a live filter is destructive unless the helper writes both versions. The computer-audio path is a Core Audio process tap feeding an IOProc, not an engine graph, so it would need its own plumbing. Offline, one code path handles both tracks, a failed or cancelled enhance falls back to the raw track, and the user can turn the toggle on after the meeting and export again.

**Fallback (if `AUSoundIsolation` proves unreliable on some Macs, Intel above all, or its sound is not good enough): DPDFNet `dpdfnet2_48khz_hr` through the `ort` already in `momr-core`.** It is a 48 kHz model from CEVA, Apache-2.0 for code and weights, about 10 MB of ONNX (https://github.com/ceva-ip/DPDFNet, https://huggingface.co/Ceva-IP/DPDFNet). The graph takes one STFT frame plus a flat state vector and returns the enhanced frame and the new state. ERB features and normalisation live inside the graph, so the host only needs an STFT/ISTFT (960-point Vorbis window, 480 hop), and `realfft` is already a dependency of `momr-core`. The cost is one model file, fetched like the Nemotron files from a pinned Hugging Face revision, plus a checksum check that the current downloader lacks (see [Risks](#risks)). Measured here, it ran at RTF about 0.16 on one thread (Python harness, one `session.run` per 10 ms frame), gave the highest SI-SDR of all candidates, and left whisper accuracy unchanged.

**Minimal fallback, no ML code at all:** ffmpeg `arnndn` with an RNNoise-nu model file (`sh.rnnn`, 297 KB, from https://github.com/GregorR/rnnoise-models). It is a single filter string in `export.rs`, but it is clearly weaker: SI-SDR +0.5 dB at moderate noise, against +7 to +8 dB for the neural models above. It is a 2018-era model.

**Not recommended:** voice-processing I/O (`setVoiceProcessingEnabled`). It is a voice-chat pipeline with echo cancellation, AGC and ducking of other audio, and it is not a recording enhancer. The user's mic mode (Voice Isolation, Wide Spectrum) cannot be set by an app. DeepFilterNet is stale, its weights licence is unanswered, and its Rust path pins a vulnerable `tract`. Generative "studio" restorers (Resemble Enhance, VoiceFixer, AudioSR, MossFormer2) are PyTorch-only, large and slow on CPU, and they can resynthesise speech that was not said. Proprietary or cloud options (NVIDIA Maxine, Krisp, Adobe Podcast) fail on platform, licence or privacy.

A caveat on "studio microphone": nothing local and non-generative adds what a better microphone captures, such as missing bandwidth or room tone removed at the source. Denoising plus EQ, de-essing and compression is the honest ceiling for a meeting record. Generative bandwidth extension could go further, but for minutes and quotes a model that invents plausible speech is a correctness risk, not a feature.

## Comparison

Quality numbers are from each project's own published tables unless marked "measured here". They are not comparable across rows unless the same table is cited. SI-SDR "measured here" is the gain over the noisy input at about 13 dB SNR, on the invented demo meeting (see [Local benchmark](#local-benchmark)).

| Candidate | What it does | 48 kHz? | Quality | Runtime | Integration | Licence (code / weights) | Last release / commit | Security and supply chain |
|---|---|---|---|---|---|---|---|---|
| **Apple AUSoundIsolation** | Neural voice isolation (denoise) | Yes (ran at 48 kHz mono here) | Measured here: +7.1 dB SI-SDR (Voice), +6.6 dB (HQ Voice) | Measured here: RTF 0.015, about 1.5% of one core | Swift helper, offline `AVAudioEngine` | Apple system component | Ships with macOS 13+; HQ model macOS 15+ | Nothing downloaded; OS-updated. Intel behaviour **unverified** |
| Apple voice-processing I/O | AEC, noise suppression, AGC for voice chat | **Unverified** | Not measured | Live only | Swift helper, live | Apple system component | macOS 10.15+ | Ducks other audio; mic mode is the user's |
| **DPDFNet 48 kHz HR** | Neural denoise (DeepFilterNet2 + dual-path RNN) | Yes | Paper (16 kHz VB+DEMAND): PESQ 2.68–2.71, SI-SNR 23.6–24.1; measured here: +8.0 dB SI-SDR | Measured here: RTF 0.16 (1 thread, per-frame ORT) | `ort` + `realfft` in core; 10 MB model | Apache-2.0 / Apache-2.0 (HF card) | v0.6.0 2026-07-05; commit 2026-09-21 | HF pinned revision + SHA-256 available; one maintainer |
| DeepFilterNet3 | Neural denoise (deep filtering) | Yes | ClearVoice table (48 kHz VB+DEMAND): PESQ 3.03; DPDFNet paper (16 kHz): PESQ 2.80; measured here: +7.8 dB SI-SDR | Paper: RTF 0.19 single-thread notebook; measured here: RTF 0.05 (release binary) | 3 ONNX graphs + host-side ERB/norm, or `deep_filter` + `tract` | MIT/Apache-2.0 / weights **unclear** (open questions #709, #712) | v0.5.6 2023-08-31; commit 2024-10-17 | libDF needs tract < 0.21.7, which is in the range of GHSA-h668-6x6g-f8r5 |
| ffmpeg `arnndn` (RNNoise-nu model) | RNN denoise | 48 kHz only | Measured here: +0.5 dB SI-SDR (moderate noise), +4.5 dB (heavy) | Measured here: RTF 0.008–0.011 | Filter string in `export.rs`; 0.3 MB model file | ffmpeg (filter BSD-style) / models "not subject to copyright" | ffmpeg 9.0.2 2026-09-17; models 2018 | OOB fix 2026-07/08, included in 9.0.2 |
| ffmpeg `afftdn` / `afwtdn` / `anlmdn` | Classic spectral / wavelet / non-local-means denoise | Yes | Measured here: about +0.2 dB SI-SDR at the settings tried | RTF 0.003 (`afftdn`), 0.08 (`anlmdn`) | Filter string | ffmpeg | as above | as above |
| ffmpeg EQ / dynamics (`highpass`, `equalizer`, `deesser`, `acompressor`, `speechnorm`, `dynaudnorm`, `loudnorm`) | Tone and level shaping, no denoise | Yes | n/a | RTF < 0.003 (`loudnorm` 0.02) | Filter string | ffmpeg | as above | as above |
| RNNoise 0.2 (C library) | RNN denoise | 48 kHz | DPDFNet paper: PESQ 2.22; GTCRN table: PESQ 2.29 | Very low (0.04 GMAC/s per GTCRN table) | New C build or crate | BSD-3 / trained on public data | v0.2 2024-04-15; commit 2025-02-22 | Model from media.xiph.org, SHA-256 checked by script |
| WebRTC APM (tonarino crate) | NS, AGC, high-pass, AEC | Yes (band-split) | Not measured | Low, real-time | New crate + meson/ninja C++ build | BSD-3 | crate v2.1.0 2026-05-13; upstream v2.1 2025-01-22 | Adds a C++ build to the tree |
| SpeexDSP preprocess | Spectral denoise, AGC | Yes | Not measured; older classic DSP | Very low | New C library | BSD-style (Xiph) | 1.2.1 2022-06-17; commit 2025-07-05 | Mature, low churn |
| GTCRN | Tiny neural denoise | **No, 16 kHz** | README (VB+DEMAND 16 kHz): PESQ 2.87 | README: RTF 0.07 on i5-12400 | ONNX via `ort`, 0.35–0.54 MB | MIT / no separate weights licence | No releases; commit 2026-08-03 | sherpa-onnx mirror |
| NSNet2 (Microsoft DNS baseline) | Neural denoise | 16 kHz; 48 kHz on a side branch | Not published in the repo | Not stated | ONNX via `ort`, 10.7 / 24.7 MB | MIT code / weights **unverified** | Branch last commit 2021-11-29 | Unmaintained baseline |
| MossFormer2_SE_48K (ClearerVoice) | Neural denoise | Yes | ClearVoice table (48 kHz VB+DEMAND): PESQ 3.15 | Not stated; 221 MB checkpoint | PyTorch only | Apache-2.0 / Apache-2.0 | tag v0.1.2; commit 2025-08-14 | Large download |
| Resemble Enhance | Denoise + generative enhancer (bandwidth, restoration) | 44.1 kHz | ClearVoice table: PESQ 2.84 | PyTorch; GPU suggested | PyTorch only | MIT / HF repo gated | 0.0.1 2023-12-14; commit 2024-12-03 | Generative |
| VoiceFixer | General restoration, neural vocoder | 44.1 kHz | Not measured | PyTorch | PyTorch only | MIT | v0.0.12 2021-10-07; commit 2025-02-17 | Generative |
| AudioSR | Diffusion super-resolution | Output 48 kHz | Not measured | Diffusion; slow | PyTorch only | MIT | commit 2025-08-27 | Generative |
| Meta Denoiser | Neural denoise | 16 kHz | Not measured | PyTorch | PyTorch only | **CC-BY-NC-4.0** | Archived; last push 2023-03-14 | Non-commercial |
| NVIDIA Maxine AFX | Denoise, dereverb, super-res, "Studio Voice" | — | — | NVIDIA GPU | **No macOS** | Proprietary | — | — |
| Krisp SDK | Noise and voice cancellation | — | — | — | Commercial SDK | Commercial licence | — | Closed binary |
| Adobe Podcast Enhance | Cloud enhancement | — | — | Cloud | Upload | Proprietary service | — | Audio leaves the Mac |

## Candidates

### 1. Apple

#### AUSoundIsolation (recommended)

- The macOS 27.0 SDK declares `kAudioUnitSubType_AUSoundIsolation = 'vois'` as "an audio unit that can be used to isolate a specified sound type", available on macOS 13.0 and iOS 16.0 (`AudioToolbox.framework/Headers/AUComponent.h` in MacOSX27.0.sdk; https://developer.apple.com/documentation/audiotoolbox/kaudiounitsubtype_ausoundisolation lists macOS 13.0, not beta).
- It has two parameters. `kAUSoundIsolationParam_WetDryMixPercent` runs from 0 to 100 with a default of 100, and `kAUSoundIsolationParam_SoundToIsolate` picks the model: `kAUSoundIsolationSoundType_Voice` ("standard voice isolation model (default)", macOS 13) or `kAUSoundIsolationSoundType_HighQualityVoice` ("high quality voice isolation model", macOS 15) (`AudioUnitParameters.h`, same SDK). The package targets macOS 14 (`helpers/momr-audio/Package.swift`), so the standard model is always there and the high-quality one needs a runtime check.
- Measured here: it runs offline through `AVAudioEngine.enableManualRenderingMode(.offline, …)` at 48 kHz mono. Both models took 1.3 to 1.5 s for 94.5 s of audio, with 1.3 to 1.4 s of user CPU time. At about 13 dB SNR, SI-SDR rose from 13.2 dB to 20.3 dB (Voice) and 19.7 dB (HQ Voice). At about 4 dB SNR it rose from 4.3 dB to 14.3 dB and 13.7 dB.
- Measured here: the unit reports `kAudioUnitProperty_Latency` as 0, but its output is delayed by 2704 samples (56 ms) with the Voice model and 4440 samples (92.5 ms) with HQ Voice, the same in both runs. The enhance step must measure or trim this, or the two tracks fall out of sync in stereo and mono exports. Whether the delay depends on the render block size is **unverified**.
- Measured here: on this synthetic voice the standard model beat the HQ model on both SI-SDR and whisper WER. That is one TTS voice, not evidence about real voices; the choice should be tested on real recordings.
- Apple silicon versus Intel: I found no Apple statement on whether the unit runs, or how fast, on Intel Macs (**unverified**). An Audacity report says the unit showed as incompatible in its plug-in scan on both Intel and Apple silicon, which is a host-scan problem and not a runtime test (https://github.com/audacity/audacity/issues/4231). The helper must treat "component not found or render failed" as "no enhancement" and export the raw track.
- Whether the unit uses the Neural Engine or GPU is **unverified**. The low CPU time is consistent with either an efficient CPU model or offloading.

#### Voice-processing I/O (`setVoiceProcessingEnabled`, AUVoiceIO)

- `AVAudioIONode.setVoiceProcessingEnabled(_:)` exists since macOS 10.15. The header says that when enabled "the input node does signal processing on the incoming audio (taking out any of the audio that is played from the device at a given time from the incoming audio)". It also says voice processing requires both input and output nodes in that mode, works only when the engine renders to the device (not in manual rendering mode), and can be switched only while the engine is stopped (`AVFAudio.framework/Headers/AVAudioIONode.h`, MacOSX27.0.sdk; https://developer.apple.com/documentation/avfaudio/avaudioionode/setvoiceprocessingenabled(_:)).
- Apple describes the processing as "echo cancellation, noise suppression, automatic gain control, among others to enhance the voice chat audio", tuned per device (WWDC23 "What's new in voice processing", https://developer.apple.com/videos/play/wwdc2023/10235/).
- It ducks other audio. macOS 14 added `voiceProcessingOtherAudioDuckingConfiguration` to set how much (header above; WWDC23 session). For a recorder that also records the computer audio that is a side effect to manage, not a feature.
- It is live-only (no manual rendering), so it cannot post-process the tap track or a saved meeting.
- Its echo cancellation could remove the remote side's voice leaking from speakers into the mic, which `transcribe.rs` currently works around with `own_speech_regions`. That is a separate feature worth its own research; it is not voice enhancement.
- What sample rate and channel count the voice-processed input delivers on macOS is **unverified**.

#### Mic modes (Voice Isolation, Wide Spectrum)

- `AVCaptureDevice.preferredMicrophoneMode` and `activeMicrophoneMode` are class properties marked `readonly`. The first is "the microphone mode selected by the user in Control Center"; the second may differ when the app's audio route does not support the preferred mode (`AVFoundation.framework/Headers/AVCaptureDevice.h`, MacOSX27.0.sdk; https://developer.apple.com/documentation/avfoundation/avcapturedevice/preferredmicrophonemode).
- An app can only open the system picker: `AVCaptureDevice.showSystemUserInterface(.microphoneModes)` "brings up the system user interface and deep links to the appropriate module" (same header).
- Apple says choosing its voice-processing APIs "grants users full control over the mic mode settings for your app, including Standard, Voice Isolation, and Wide Spectrum" (WWDC23 session 10235). So mic modes come with voice-processing I/O and share its drawbacks above. On macOS 14 and later the user picks the mode from the menu bar during a call (https://support.apple.com/en-us/105117).
- Conclusion: the app cannot switch Voice Isolation on by itself; it could only show the picker. Not a basis for a toggle.

#### Newer Apple audio APIs (checked through the macOS 27.0 SDK)

- `kAudioUnitSubType_AUAudioMix` (`'amix'`, macOS 26) "supports AudioMix separate-and-remix functionality", with styles such as Cinematic and Studio. It needs `kAUAudioMixProperty_SpatialAudioMixMetadata`, "remix metadata from the file asset", and outputs first-order ambisonics plus a mono foreground by default (`AUComponent.h`, `AudioUnitParameters.h`, `AudioUnitProperties.h`, MacOSX27.0.sdk). It works on spatial-audio recordings that carry that metadata, not on a plain mic or a tap stream.
- A search of the AudioToolbox, AVFAudio, AVFoundation, CoreAudio and SoundAnalysis headers in the macOS 27.0 SDK for "isolation", "noise suppression", "speech enhance" and "dereverb" found only AUSoundIsolation, the mic-mode API and voice processing (measured here with `grep`). Sound Analysis classifies sounds and does not modify audio.

### 2. ffmpeg built-in filters

The bundled Homebrew ffmpeg 9.0.2 is configured without `--disable-filter` flags, and `ffmpeg -filters` lists `arnndn`, `afftdn`, `afwtdn`, `anlmdn`, `highpass`, `lowpass`, `adeclick`, `deesser`, `acompressor`, `speechnorm`, `dynaudnorm`, `loudnorm`, `equalizer`, `adynamicequalizer`, `agate`, `aexciter` and `dialoguenhance` (measured here). Homebrew's formula notes that `ffmpeg-full` carries extra libraries (`brew info ffmpeg`). That formula's dependency list includes `whisper.cpp`, `speex` and `rubberband`, and the plain `ffmpeg` bottle has none of them (`brew info ffmpeg-full`; https://github.com/Homebrew/homebrew-core/blob/HEAD/Formula/f/ffmpeg-full.rb).

- **`arnndn`**: "Reduce noise from speech using Recurrent Neural Networks". The `model` option "is always required", and `mix` from -1 to 1 blends filtered and original, a built-in wet/dry (https://ffmpeg.org/ffmpeg-filters.html#arnndn). The source accepts only 48 kHz and reads only text files headed `rnnoise-nu model file version 1` (https://github.com/FFmpeg/FFmpeg/blob/master/libavfilter/af_arnndn.c). The binary models of RNNoise 0.2 therefore do not load, and the usable models are the 2018 RNNoise-nu set (https://github.com/GregorR/rnnoise-models). That repository says that, apart from its tools, "none of this work is creative and thus none of it is subject to copyright", and it has no licence file; last push 2018-09-02. An out-of-bounds read in `arnndn` ("pad the DCT input buffers to the read length… Fixes: out of array access") was fixed on master on 2026-07-31 and on `release/9.0` on 2026-08-02, and that commit is in tag `n9.0.2` (tagged 2026-09-17) (https://github.com/FFmpeg/FFmpeg/commit/e38b5d15bd93585f96c75e31e2dd1e1fe9294d05). A model file is parsed by C code, so it should come from a pinned, checksummed source.
- **`afftdn`**: FFT denoiser with `noise_reduction` (default 12 dB), `noise_floor`, `noise_type` and `track_noise` (https://ffmpeg.org/ffmpeg-filters.html#afftdn). Measured here with `nr=20:nf=-40:tn=1`, it barely moved SI-SDR (+0.2 dB). It suits steady hiss, not traffic or keyboard.
- **`anlmdn`**: non-local-means broadband denoiser; its default `strength` is 0.00001, so it does nothing until tuned (https://ffmpeg.org/ffmpeg-filters.html#anlmdn). Measured here, `s=7` ran at RTF 0.08 and gave no SI-SDR gain; tuning was not explored.
- **`afwtdn`**: wavelet denoiser for broadband noise (https://ffmpeg.org/ffmpeg-filters.html#afwtdn). Measured here: RTF 0.003.
- **`speechnorm`** expands or compresses each half-cycle toward a target peak, a speech-specific leveller (https://ffmpeg.org/ffmpeg-filters.html#speechnorm). `dynaudnorm` and `loudnorm` (EBU R128) are general levellers. The existing single-gain `speech_gain_db` + `alimiter` design keeps the dynamics on purpose (module comment in `export.rs`), so a compressor should stay gentle.
- **`dialoguenhance`** takes stereo in and makes 3.0 surround with dialogue in the centre channel (https://ffmpeg.org/ffmpeg-filters.html#dialoguenhance). It is for film mixes, not a mono voice.
- **ML filters**: `whisper` needs `--enable-whisper` and whisper.cpp (https://ffmpeg.org/ffmpeg-filters.html#whisper), and `dnn_processing` does "image processing with deep neural networks" through TensorFlow or OpenVINO backends (https://ffmpeg.org/ffmpeg-filters.html#dnn_processing). Neither is in the Homebrew build (measured here) and neither enhances speech.

### 3. RNNoise (xiph/rnnoise)

- v0.2 was released on 2024-04-15: "improved training, SSE4.1 and AVX2 optimizations, and run-time CPU detection", and "the distributed models are now trained using only publicly available datasets" (https://github.com/xiph/rnnoise/releases/tag/v0.2). The last commit on the GitHub mirror is 2025-02-22 (https://github.com/xiph/rnnoise/commits). The mirror says the official repository is on Xiph's GitLab.
- BSD-3-Clause; 48 kHz 16-bit mono (https://github.com/xiph/rnnoise).
- The model is fetched from `https://media.xiph.org/rnnoise/models/` and checked against the SHA-256 in `model_version` by `download_model.sh` (https://github.com/xiph/rnnoise/blob/main/download_model.sh).
- Quality is the lowest of the neural options: PESQ 2.29 against GTCRN's 2.87 in the GTCRN README (https://github.com/Xiaobin-Rong/gtcrn), and PESQ 2.22 against DeepFilterNet3's 2.80 in the DPDFNet paper (https://arxiv.org/abs/2512.16420). Both are 16 kHz VoiceBank+DEMAND.
- Rust port: `nnnoiseless` 0.5.2 (2025-12-18), BSD-3-Clause (https://crates.io/crates/nnnoiseless, https://github.com/jneem/nnnoiseless). It would be a new crate for a weak model; ffmpeg's `arnndn` already gives the same class of result for free.
- `werman/noise-suppression-for-voice` wraps RNNoise as VST, LV2, LADSPA, AU and AUv3 plug-ins (v1.21, 2026-05-29), but it is GPL-3.0 (https://github.com/werman/noise-suppression-for-voice), which does not fit an MIT app loading it in-process.

### 4. DeepFilterNet (Rikorose/DeepFilterNet)

- "A Low Complexity Speech Enhancement Framework for Full-Band Audio (48kHz)"; the `deep-filter` binary accepts only 48 kHz WAV (https://github.com/Rikorose/DeepFilterNet). The code is MIT or Apache-2.0: "All code in this repository is dual-licensed" (same README).
- The weights licence is open. Two issues ask whether the MIT/Apache grant covers the checkpoints and ONNX exports, #709 (2026-09-01) and #712 (2026-09-21), and neither has a maintainer answer (https://github.com/Rikorose/DeepFilterNet/issues/709, https://github.com/Rikorose/DeepFilterNet/issues/712). The DFN3 paper says "the framework as well as pretrained weights have been published under an open source license" (https://arxiv.org/abs/2305.08227), without naming it.
- Maintenance: last release v0.5.6 on 2023-08-31, last commit on `main` 2024-10-17 (https://github.com/Rikorose/DeepFilterNet/releases, https://github.com/Rikorose/DeepFilterNet/commits). The crates.io `deep_filter` is 0.2.5 from 2022-07-28 (https://crates.io/crates/deep_filter). A current issue reports that `cargo build --features tract` fails on a fresh checkout because `tract` 0.21.7+ moved to `ndarray` 0.16; the proposed fix pins `tract` below 0.21.7 (https://github.com/Rikorose/DeepFilterNet/issues/703).
- Security: GHSA-h668-6x6g-f8r5 (2026-06-19, medium) is an arbitrary file read through an unsanitised ONNX `external_data` location in `tract-onnx` < 0.21.17, patched in 0.21.17, 0.22.3 and 0.23.2 (https://github.com/advisories/GHSA-h668-6x6g-f8r5). A libDF held below 0.21.7 is inside the vulnerable range. It matters only for untrusted model files, but it also means a second ONNX runtime next to `ort`.
- ONNX: `models/DeepFilterNet3_onnx.tar.gz` (8.0 MB) and a low-latency variant exist in the repository (https://github.com/Rikorose/DeepFilterNet/tree/main/models). DFN3's ONNX export is split into three graphs (encoder, ERB decoder, DF decoder), and ERB feature extraction and normalisation happen in libDF's Rust code, so running it on `ort` means porting that host code. That is from the libDF source layout (**unverified** in detail; I did not port it).
- Runtime: DFN2 paper "real-time factor to 0.04 on a notebook Core-i5 CPU" (https://arxiv.org/abs/2205.05474); DFN3 demo paper "real-time-factor of 0.19 on a single threaded notebook CPU" (https://arxiv.org/abs/2305.08227). Measured here with the v0.5.6 `aarch64-apple-darwin` release binary: 4.4 to 5.0 s for 94.5 s (RTF about 0.05). The release assets carry no published checksums (https://github.com/Rikorose/DeepFilterNet/releases/tag/v0.5.6); the binary I ran had SHA-256 `4601e7f4…611081`.
- The LADSPA plug-in targets PipeWire on Linux (https://github.com/Rikorose/DeepFilterNet/blob/main/ladspa/README.md); there is no macOS plug-in build in v0.5.6.
- Verdict: good sound, but stale, with an unclear weights licence and a Rust path that fights the current `tract`. DPDFNet is its maintained successor-in-spirit and ships as single-file ONNX.

### 5. DPDFNet (ceva-ip/DPDFNet), recommended fallback

- "A causal single-channel speech enhancement model that extends DeepFilterNet2 architecture with dual-path blocks in the encoder" (https://arxiv.org/abs/2512.16420, 2025-12-18). The repository offers "Pretrained 8, 16, and 48 kHz models" with ONNX and TFLite exports (https://github.com/ceva-ip/DPDFNet).
- 48 kHz models: `dpdfnet2_48khz_hr` (2.58 M params, 2.42 GMAC/s, 10.0 MB ONNX) and `dpdfnet8_48khz_hr` (3.63 M, 7.17 GMAC/s, 14.2 MB) (README model profile). `dpdfnet8_48khz_hr` arrived in v0.5.0 (https://github.com/ceva-ip/DPDFNet/releases/tag/v0.5.0).
- Published quality is for the 16 kHz models: on VoiceBank+DEMAND, PESQ 2.68 to 2.71 and SI-SNR 23.6 to 24.1 against DeepFilterNet3's 2.80 and 21.2, and on the DNS4 blind set DNSMOS OVRL 3.39 to 3.40 against DFN3's 3.28 (https://arxiv.org/html/2512.16420). I found no published metrics for the 48 kHz HR models (**unverified**).
- ONNX contract, from the model's own metadata (measured here): inputs `spec [1,1,481,2]` and `state_in [56436]`; outputs `spec_e` and `state_out`; `n_fft 960`, `hop_length 480`, `window_type vorbis`, `center 1`, `pad_mode reflect`; the initial state comes from `erb_norm_init` and `spec_norm_init` in the metadata. The upstream runner does the same (https://github.com/ceva-ip/DPDFNet/blob/main/onnx_model/infer_dpdfnet_onnx.py). The host code is an STFT loop and a state buffer, and `realfft` is already in `momr-core/Cargo.toml`.
- Licence: Apache-2.0 for the repository (https://github.com/ceva-ip/DPDFNet/blob/main/LICENSE) and `license: apache-2.0` on the Hugging Face model card that hosts the weights (https://huggingface.co/Ceva-IP/DPDFNet).
- Maintenance: created 2025-12-17, latest release v0.6.0 on 2026-07-05, last push 2026-09-21, 3 open issues, and one contributor account with all 98 commits (GitHub API, https://github.com/ceva-ip/DPDFNet). It is a corporate project with a bus factor of one.
- Supply chain: the Hugging Face tree lists an LFS SHA-256 per file, for example `onnx/dpdfnet2_48khz_hr.onnx` at 10,493,337 bytes with SHA-256 `7f0575a5…291dc14b`, repo revision `dd6818d00f50c836fed43a6243ebe49116de5964` (https://huggingface.co/api/models/Ceva-IP/DPDFNet/tree/main/onnx). sherpa-onnx republishes a different build of the same model name (10,596,848 bytes, GitHub digest `sha256:0b399f8a…944928`) under its `speech-enhancement-models` release (https://github.com/k2-fsa/sherpa-onnx/releases/tag/speech-enhancement-models). That is the copy I benchmarked, and its hash matched the digest. Pin one source and its hash.
- Runtime, measured here: Python + onnxruntime 1.30, one `run` per 10 ms frame: RTF 0.154 to 0.161 on one thread, 0.144 on four (the model is sequential per frame). For a one-hour meeting that is about 9 to 10 minutes per track on an M1 Pro, which is too slow to add to Stop as-is. Running it live in a background thread during recording (about 16% of one core per track) or in parallel with transcription would hide it. On Intel Macs it will be slower (**unverified**, no Intel machine was available).

### 6. WebRTC Audio Processing

- The tonarino crate wraps "PulseAudio's repackaging of WebRTC's AudioProcessing module" for echo removal, noise removal, AGC and VAD. It links a system library by default or builds the bundled C++ with the `bundled` feature, which needs meson, ninja and pkg-config (https://github.com/tonarino/webrtc-audio-processing). BSD-3-Clause; crate v2.1.0 on 2026-05-13 and last commit 2026-07-16 (https://crates.io/crates/webrtc-audio-processing).
- Upstream is freedesktop's `webrtc-audio-processing`, v2.1 on 2025-01-22, last commit 2025-11-10 ("examples: Use 48 kHz by default") (https://gitlab.freedesktop.org/pulseaudio/webrtc-audio-processing).
- Its noise suppressor is a classic statistical estimator tuned for calls. I did not measure it, so how it compares to the neural models is **unverified**. It would add a crate plus a C++ build system to get less than AUSoundIsolation already gives. Not recommended.

### 7. SpeexDSP preprocessor

- Xiph's SpeexDSP carries a BSD-style notice (https://github.com/xiph/speexdsp/blob/master/COPYING). Last release SpeexDSP-1.2.1 on 2022-06-17, last commit 2025-07-05 (https://github.com/xiph/speexdsp). Its preprocessor offers denoise, AGC and VAD. It is mature, classic DSP, and its quality against neural models is unmeasured here. It would add a C library for little gain. Not recommended.

### 8. Small neural models with ONNX

- **GTCRN**: "only 48.2 K parameters and 33.0 MMACs per second"; PESQ 2.87 on VoiceBank+DEMAND and DNSMOS OVRL 2.70 on the DNS3 blind set; streaming RTF 0.07 on an i5-12400 (https://github.com/Xiaobin-Rong/gtcrn). It is 16 kHz: the ERB module is built with `fs=16000` and `nfft=512` (https://github.com/Xiaobin-Rong/gtcrn/blob/main/gtcrn.py). MIT licence, no separate weights licence, no releases, last commit 2026-08-03. ONNX files are 0.35 and 0.54 MB. Running at 16 kHz would throw away the top of a 48 kHz recording, the opposite of "fuller".
- **NSNet2** (Microsoft DNS-Challenge baseline): no longer on `master`. `nsnet2-20ms-baseline.onnx` (16 kHz, 10.7 MB) is on branch `icassp2021-final`, and `nsnet2-20ms-48k-baseline.onnx` (24.7 MB) is on branch `nsnset_48khz`, last commit 2021-11-29 (https://github.com/microsoft/DNS-Challenge/tree/nsnset_48khz/NSNet2-baseline). The repository has `LICENSE-CODE` (MIT) and `LICENSE` (CC-BY-4.0); which one covers the ONNX weights is not stated (**unverified**). It is an unmaintained baseline.
- **ClearerVoice-Studio (Alibaba)**: `MossFormer2_SE_48K` scores PESQ 3.15 on the 48 kHz VoiceBank+DEMAND table, against DeepFilterNet 3.03 and Resemble Enhance 2.84 in the same table (https://github.com/modelscope/ClearerVoice-Studio/blob/main/clearvoice/README.md). Code Apache-2.0; the Hugging Face weights are `apache-2.0` (https://huggingface.co/alibabasglab/MossFormer2_SE_48K). The checkpoint is a 221.6 MB PyTorch `.pt` with no official ONNX export, and the last commit is 2025-08-14. `FRCRN_SE_16K` is 16 kHz. It has the best published quality but is PyTorch-only and heavy, so it does not fit.
- **sherpa-onnx** (k2-fsa, Apache-2.0) packages GTCRN and DPDFNet ONNX models (https://github.com/k2-fsa/sherpa-onnx/releases/tag/speech-enhancement-models). It is a C++ library; we only need its model files, not the library.

### 9. "Studio" restoration (generative)

- **Resemble Enhance**: "a denoiser… and an enhancer, which further boosts the perceptual audio quality by restoring audio distortions and extending the audio bandwidth", trained at 44.1 kHz; MIT code; PyTorch via pip (https://github.com/resemble-ai/resemble-enhance). Release 0.0.1 on 2023-12-14, last commit 2024-12-03. The Hugging Face model repository did not answer an anonymous API request ("Invalid username or password"), so the weights' terms are **unverified**.
- **VoiceFixer**: "restore human speech regardless how serious its degraded… noise, reveberation, low resolution (2kHz~44.1kHz) and clipping", using a 44.1 kHz neural vocoder; MIT; last release 2021-10-07 (https://github.com/haoheliu/voicefixer). A vocoder resynthesises the voice.
- **AudioSR**: diffusion-based super-resolution for any audio; MIT; the README notes it was trained on low-pass-filtered data and "struggles when encountering unfamiliar cutoff patterns" (https://github.com/haoheliu/versatile_audio_super_resolution).
- **Meta Denoiser**: archived, 16 kHz, licensed "Attribution-NonCommercial 4.0 International" (https://github.com/facebookresearch/denoiser/blob/main/LICENSE). Excluded.
- **NVIDIA Maxine Audio Effects**: offers denoise, dereverb, super-resolution and "Studio Voice", but "the Windows SDK supports x64 systems and NVIDIA N1X systems… The Linux SDK is designed… for server-side deployments", on NVIDIA GPUs (https://docs.nvidia.com/maxine/afx/latest/index.html). No macOS.
- **Krisp SDK**: "a commercial product. Developers will need to obtain a commercial license from Krisp Technologies" (https://sdk-docs.krisp.ai/docs/licensing-information). Closed binary; excluded under the few-dependencies and privacy rules.
- **Adobe Podcast Enhance Speech**: a web service ("AI audio recording and editing, all on the web", https://podcast.adobe.com/en). Audio would leave the Mac, which rules it out as a default. Adobe's help pages returned 403 to my fetch, so limits and terms are **unverified**.

All generative options are Python/PyTorch-only, large, and slow without a GPU. They can also produce fluent speech that differs from what was said. For a meeting record that is disqualifying as a default.

### 10. Zig, C and C++ libraries

Beyond RNNoise, SpeexDSP and WebRTC APM above, I found no reputable, maintained Zig or C speech-enhancement library with published metrics that beats them. `nnnoiseless` (Rust) and the GPL RNNoise plug-in are ports of the same RNNoise model.

## Local benchmark

This is a sanity check, not a listening test: one synthetic voice and synthetic noise, scored against the known clean signal.

- **Material**: the first 266 words of the invented meeting in `demo/script.txt`, voiced with `say` (94.5 s, 48 kHz mono). Two noisy mixes: "heavy" (pink noise + low-passed brown "road" noise + 50 Hz hum, about 4 dB SI-SDR) and "moderate" (the same at lower level, about 13 dB SI-SDR).
- **Scores**: SI-SDR against the clean signal after lag alignment, plus WER against the script text, transcribed by this repository's `target/debug/momr transcribe-file … --provider local` with whisper `tiny` and `large-v3-turbo`. With 282 reference words, one word is 0.35 points of WER.

Moderate noise (about 13 dB):

| Variant | SI-SDR | WER tiny | WER large-v3-turbo |
|---|---|---|---|
| clean | — | 7.4% | 3.5% |
| noisy | 13.2 dB | 10.6% | 3.5% |
| ffmpeg `arnndn` (sh.rnnn) | 13.7 dB | 13.1% | 3.2% |
| ffmpeg `afftdn` | 13.4 dB | 10.6% | 3.5% |
| DeepFilterNet3 (release binary) | 20.9 dB | 8.5% | 3.5% |
| DPDFNet 48 kHz HR | 21.1 dB | 8.9% | 3.9% |
| DPDFNet + voice chain | — | 7.1% | 3.5% |
| DPDFNet, 15% original mixed back | — | 10.3% | 3.5% |
| AUSoundIsolation, Voice | 20.3 dB | 6.4% | 2.8% |
| AUSoundIsolation, HQ Voice | 19.7 dB | 10.6% | 4.6% |

Heavy noise (about 4 dB): every whisper model returned "_No speech was recognized._" on the unprocessed mix. With any neural enhancer, the transcripts came back at 8.5 to 13.5% WER (tiny) and 3.2 to 5.0% (large-v3-turbo). The cause is in this repository, not in whisper. `active_frames` in `crates/momr-core/src/transcribe.rs` keeps only frames louder than four times the 10th-percentile frame energy, and under steady loud noise, speech never clears that bar. That is worth its own look (see [Risks](#risks)).

Runtime for the same 94.5 s (wall time, M1 Pro): `afftdn` 0.27 s, `afwtdn` 0.32 s, `arnndn` 0.74 to 1.07 s, `anlmdn` 7.6 s, the EQ/de-esser/compressor/limiter chain 0.23 s, `loudnorm` 1.9 s, AUSoundIsolation 1.3 to 1.5 s, DeepFilterNet3 4.4 to 5.0 s, DPDFNet (Python, per frame) 13.6 to 15.3 s.

The scripts (Swift offline renderer for AUSoundIsolation, Python DPDFNet runner, SI-SDR and WER scorers) lived in the session scratchpad and are not in the repository.

## Risks

- **Enhancement can hurt transcription.** A 2025 study ran MetricGAN+ denoising in front of Whisper, Parakeet, Gemini Flash 2.0 and Parrotlet on 500 medical recordings under nine noise conditions: "Original noisy audio achieves lower semWER than enhanced audio in all 40 tested configurations", with degradations of 1.1 to 46.6 points (https://arxiv.org/abs/2512.17562). A 2026 study put SAM-Audio separation in front of five Whisper variants: "WER and CER increase in every evaluated model-dataset configuration", for example Whisper base on English going from 10.53% to 21.66% (https://arxiv.org/abs/2603.04710). Earlier work traced the damage to the "artifact component" of enhancement errors and found that "adding a scaled version of the observed signal to the enhanced speech" improves ASR (https://arxiv.org/abs/2201.06685). Here, at moderate noise, large-v3-turbo stayed within two words of the noisy baseline for every enhancer (measured here). *Mitigation*: transcribe the raw tracks by default, and apply enhancement only to the listening export. Keep the wet/dry mix (`kAUSoundIsolationParam_WetDryMixPercent`, `arnndn` `mix`, DPDFNet `attn_limit_db`) below 100% if the enhanced audio ever feeds whisper.
- **The energy gate before whisper fails in steady loud noise** (measured here, above). Running `speech_regions` on an enhanced copy while whisper still hears the raw audio might fix both problems at once. That is an idea to test, not a finding.
- **Artifacts.** Neural suppressors can clip word onsets, make breaths and quiet consonants sound gated, and warble on music or laughter. RNNoise-class models are the most prone (lowest scores above). A "studio" EQ adds sibilance that `deesser` must catch. Generative restorers can change words. *Mitigation*: a moderate default mix, and the raw track always kept.
- **Track alignment.** AUSoundIsolation delays its output by 56 to 93 ms without reporting it (measured here); DeepFilterNet delays by 30 ms unless `--compensate-delay` is passed (measured here; flag from `deep-filter --help`). If one track is enhanced and the other is not, or the two use different models, the stereo and mono exports lose sync. The enhance step must remove its own delay.
- **The computer-audio track.** Remote voices arrive already processed and compressed by the meeting app's own noise suppression and codec. A second suppressor adds little, and it can hurt music or shared-screen audio. Offer the toggle per track, or default it to the mic.
- **Intel Macs.** AUSoundIsolation on Intel is **unverified**. DPDFNet at 2.42 GMAC/s ran at RTF 0.16 on an M1 Pro performance core. On a 2018 to 2020 Intel laptop, expect it to be noticeably slower (**unverified**), possibly beyond what a Stop-time post-process should take for long meetings. Test on an Intel Mac before promising the fallback there.
- **Model download integrity.** `transcribe::download` checks only size (`min_bytes` and Content-Length), not a hash (`crates/momr-core/src/transcribe.rs`). The Nemotron files at least use a pinned Hugging Face revision (`nemotron.rs`). Any new model should add a SHA-256 check. `/usr/bin/shasum -a 256` via `std::process::Command` keeps to the no-new-crate rule. Hugging Face publishes the LFS SHA-256 per file, and GitHub release assets now carry a `digest`.
- **Licences.** DeepFilterNet weights are unclear (open issues). NSNet2 weights are unclear. The RNNoise-nu models have no licence file but disclaim copyright. The GPL RNNoise plug-in cannot be loaded in-process by this MIT app. DPDFNet (Apache-2.0 on code and card) and Apple's system unit are the clean options.
- **Parser attack surface.** ffmpeg `arnndn` parses its model file in C (an OOB read was fixed as recently as 2026-08), and `tract-onnx` had a path-traversal advisory. Only load models from pinned, hashed sources, never from user-supplied paths.

## Sources

Apple
- MacOSX27.0.sdk headers: `AudioToolbox/AUComponent.h`, `AudioToolbox/AudioUnitParameters.h`, `AudioToolbox/AudioUnitProperties.h`, `AVFAudio/AVAudioIONode.h`, `AVFoundation/AVCaptureDevice.h` (Xcode, read locally)
- https://developer.apple.com/documentation/audiotoolbox/kaudiounitsubtype_ausoundisolation
- https://developer.apple.com/documentation/avfaudio/avaudioionode/setvoiceprocessingenabled(_:)
- https://developer.apple.com/documentation/avfoundation/avcapturedevice/preferredmicrophonemode
- https://developer.apple.com/videos/play/wwdc2023/10235/
- https://support.apple.com/en-us/105117
- https://github.com/audacity/audacity/issues/4231

ffmpeg and RNNoise
- https://ffmpeg.org/ffmpeg-filters.html
- https://github.com/FFmpeg/FFmpeg/blob/master/libavfilter/af_arnndn.c
- https://github.com/FFmpeg/FFmpeg/commit/e38b5d15bd93585f96c75e31e2dd1e1fe9294d05
- https://github.com/Homebrew/homebrew-core/blob/HEAD/Formula/f/ffmpeg.rb and `ffmpeg-full.rb`
- https://github.com/GregorR/rnnoise-models
- https://github.com/xiph/rnnoise, https://github.com/xiph/rnnoise/releases/tag/v0.2, https://github.com/xiph/rnnoise/blob/main/download_model.sh
- https://github.com/jneem/nnnoiseless, https://crates.io/crates/nnnoiseless
- https://github.com/werman/noise-suppression-for-voice

DeepFilterNet and DPDFNet
- https://github.com/Rikorose/DeepFilterNet, https://github.com/Rikorose/DeepFilterNet/releases/tag/v0.5.6
- https://github.com/Rikorose/DeepFilterNet/issues/703, /709, /712
- https://crates.io/crates/deep_filter
- https://arxiv.org/abs/2205.05474 (DeepFilterNet2), https://arxiv.org/abs/2305.08227 (DeepFilterNet3 demo)
- https://github.com/advisories/GHSA-h668-6x6g-f8r5
- https://github.com/ceva-ip/DPDFNet, https://github.com/ceva-ip/DPDFNet/releases
- https://arxiv.org/abs/2512.16420, https://arxiv.org/html/2512.16420
- https://huggingface.co/Ceva-IP/DPDFNet
- https://github.com/k2-fsa/sherpa-onnx/releases/tag/speech-enhancement-models

Other DSP and models
- https://github.com/tonarino/webrtc-audio-processing, https://gitlab.freedesktop.org/pulseaudio/webrtc-audio-processing
- https://github.com/xiph/speexdsp
- https://github.com/Xiaobin-Rong/gtcrn
- https://github.com/microsoft/DNS-Challenge (branches `icassp2021-final`, `nsnset_48khz`)
- https://github.com/modelscope/ClearerVoice-Studio, https://huggingface.co/alibabasglab/MossFormer2_SE_48K
- https://github.com/resemble-ai/resemble-enhance
- https://github.com/haoheliu/voicefixer
- https://github.com/haoheliu/versatile_audio_super_resolution
- https://github.com/facebookresearch/denoiser
- https://docs.nvidia.com/maxine/afx/latest/index.html
- https://sdk-docs.krisp.ai/docs/licensing-information
- https://podcast.adobe.com/en

Enhancement and ASR
- https://arxiv.org/abs/2512.17562
- https://arxiv.org/abs/2603.04710
- https://arxiv.org/abs/2201.06685
