# 13 Transcription providers and interface languages

## Goal

`transcribe-file`, imports and meetings can run through ElevenLabs
speech-to-text, Google Cloud Speech-to-Text, or any speech-to-text model on
OpenRouter instead of the local whisper model, picked in Preferences (or
`transcribe-file --provider` for one run) with the credentials entered
there, and the whole UI reads in English or Indonesian, switched in
Preferences. Local whisper stays the default: without an explicit choice no
audio leaves the Mac. The OpenRouter model is `openrouter_model` in
config.toml (`openai/whisper-1` unless set to another transcription-capable
slug); OpenRouter returns no speaker tags, so its words transcribe without
speaker attribution.

## Done when

- [x] Preferences has a Transcription provider row (Local, ElevenLabs,
      Google, OpenRouter), key fields with where-to-get instructions, and a
      privacy line per provider.
- [~] `transcribe-file` and a two-track meeting transcribe end to end through
      each provider, with You/Remote and Speaker N attribution intact.
- [~] `momr` in Indonesian: every user-visible string through the locales
      module, language switch in Preferences, no English leaks in a UI walk.
- [x] README privacy section names per-provider data handling.
- [x] `cargo test` covers request building, response parsing, chunk math and
      locale completeness (every key in both languages).

Implemented 2026-09-25: `provider.rs` (Keychain keys, chunked ElevenLabs,
Google and OpenRouter passes with word-level segments, speaker numbering
like the local path where the provider returns tags), routing in
`transcribe()`/`transcribe_single()`, `transcribe-file --provider` flag for
one run without touching config, Preferences picker + password rows +
per-provider privacy, `locales.rs` (English + Indonesian,
completeness-tested) with the whole UI converted including menus, dialogs,
toasts, stages, agent messages, CLI help and the menu-bar item. Omnilingual
stays out (Python fairseq2 stack, no shippable runtime). Live provider runs
need the user's own keys, so the end-to-end cloud path is verified to the
key gate (correct "no key" error per provider) rather than past it.
## Prerequisites

Plans 07 (Preferences dialog) and 05 (config keys).

## Background

`transcribe.rs` `transcribe()` runs each side through whisper regions and
interleaves `Segment`s; `transcribe_single()` does one track with diarized
speakers. A provider replaces the per-side/per-track engine: chunked audio in
(ffmpeg segments with offsets), word/timestamp JSON out, mapped back to
`Segment`s with the same speaker labels. `ureq` (already a dependency) grows
the `multipart` and `json` features — no new crates.

## Providers

| | ElevenLabs | Google Cloud STT |
|---|---|---|
| Endpoint | `POST https://api.elevenlabs.io/v1/speech-to-text`, multipart `file` + `model_id=scribe_v1` (+ `language_code`, `diarize=true`), header `xi-api-key` | `POST https://speech.googleapis.com/v1/speech:recognize?key=…`, JSON `{config, audio}` |
| Words | `words[]` `{text, start, end, speaker_id?}` | `results[].alternatives[].words[]` `{word, startTime, endTime, speakerTag?}` |
| Speakers | `speaker_id` → Speaker N by first appearance | `speakerTag` → Speaker N; needs `diarizationConfig` on |
| Limits | ~10 min per request: chunk longer audio | sync ≤ 60 s audio: 55 s chunks, offsets added back |
| Key | Dashboard → profile → API Keys (`https://elevenlabs.io/app/settings/api-keys`) | Console → project → enable Speech-to-Text API → Credentials → API key, restricted to the API |
| Audio | 48 kHz mono WAV per chunk | 16 kHz mono LINEAR16 base64 per chunk (own base64: no new crate) |

Keys live in the macOS Keychain (`security` CLI, service `momr-<provider>`),
never in config files; the config only names the provider. Missing key,
bad key and quota errors surface as the provider's message in the UI, and a
failed chunk aborts the run like a failed whisper pass.

Omnilingual ASR stays out: it is a Python fairseq2 research stack (300M
parameters at the smallest, GPU-speed only) with no local runtime this app
can ship. Revisit if an ONNX/CoreML export appears.

## Interface languages

`src/locales.rs`: `pub enum Lang { En, Id }`, `pub fn t(key) -> &'static str`
per language from two exhaustive tables, `settings.json` `ui_language`.
Every user-visible literal in `ui.rs` (labels, buttons, banners, dialogs,
toasts, menu items, Preferences) goes through it; format arguments stay
positional (`{}`) with matching order in both languages. The completeness
test asserts both tables hold exactly the same keys. Transcript *content*
(stored names, markdown) is untouched — only the chrome translates.

## Steps

1. `provider.rs`: `Provider` enum + config/keychain access, ElevenLabs and
   Google request builders (pure, tested), response → `Segment`s mapping
   (pure, tested), ffmpeg chunking with offsets, base64 (tested vectors).
2. Route `transcribe()`, `transcribe_single()` and the CLIs through the
   configured provider; progress and abort per chunk; provider errors as UI
   text.
3. Preferences: provider picker, key entries (password rows), per-provider
   instructions with links, UI language row; privacy notes.
4. `locales.rs` + the `ui.rs` extraction; completeness test.
5. README privacy + Preferences docs; tick the board.

## Risks and notes

- API field names drift: parse defensively (`serde_json::Value`, required
  `text`/`transcript` only) and report the provider's error body verbatim.
- Long meetings multiply cost: show the chunk count in the progress line so
  a cloud run never surprises.
- `say` fixtures stay the transcription tests; provider tests use checked-in
  response JSON, never the network.

## Status

- [x] Step 1 `provider.rs`
- [x] Step 2 routing
- [x] Step 3 Preferences
- [x] Step 4 locales
- [x] Step 5 docs

