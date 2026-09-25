//! Interface language: English or Indonesian. Every user-visible literal in
//! the app goes through `t()`. Both languages sit side by side in one table,
//! so a key without its Indonesian text does not compile, and the key list
//! the tests walk is generated from the same table. Format arguments stay
//! positional (`{}`) with the same count in both languages; `tf()` fills
//! them in one pass.
//!
//! Transcript *content* (speaker labels, the markdown, the language line) is
//! untouched; only the chrome translates, because `transcript.md` is read by
//! scripts. The language is resolved once per launch, since the widgets are
//! built once at startup and a mid-run switch would mix the two: a change in
//! Settings takes effect on the next launch.

use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    English,
    Indonesian,
}

impl Lang {
    /// The settings.json and `MOMR_LANG` code.
    pub fn code(self) -> &'static str {
        match self {
            Lang::English => "en",
            Lang::Indonesian => "id",
        }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        match code {
            "en" => Some(Lang::English),
            "id" => Some(Lang::Indonesian),
            _ => None,
        }
    }

    /// A locale such as `id-ID`, `id_ID.UTF-8` or `en-GB` to a language.
    fn from_locale(locale: &str) -> Lang {
        if locale.trim().trim_matches('"').starts_with("id") {
            Lang::Indonesian
        } else {
            Lang::English
        }
    }
}

/// The launch language, chosen once per process by the shell at startup
/// (it feeds the Settings choice through `init_lang` before any `t()`
/// call). Reads as English until then, so unit tests — which never
/// initialise — pass whatever the developer's settings.
static LANG: OnceLock<Lang> = OnceLock::new();

/// Records the launch language; the shell calls this once at startup.
pub fn init_lang(lang: Lang) {
    let _ = LANG.set(lang);
}

/// The launch language, English until `init_lang` runs.
pub fn current() -> Lang {
    LANG.get().copied().unwrap_or(Lang::English)
}

/// The launch-language chain, pure for tests: the Settings choice wins, else
/// the first macOS preferred language, else `$LANG`.
pub fn resolve_lang(saved: Option<Lang>, apple_first: Option<&str>, lang_env: &str) -> Lang {
    saved
        .or_else(|| apple_first.map(Lang::from_locale))
        .unwrap_or_else(|| Lang::from_locale(lang_env))
}

/// `(\n    "id-ID",\n    "en-US"\n)` to `id-ID`.
pub fn first_apple_language(plist: &str) -> Option<String> {
    plist
        .split(['(', ',', ')', '\n'])
        .map(|item| item.trim().trim_matches('"'))
        .find(|item| !item.is_empty())
        .map(str::to_owned)
}

/// The string for `key` in the current language.
pub fn t(key: &str) -> &'static str {
    t_in(current(), key)
}

pub fn t_in(lang: Lang, key: &str) -> &'static str {
    lookup(lang, key).unwrap_or("missing string")
}

/// `t(key)` with each `{}` replaced by the next of `args`, in one pass, so an
/// argument that itself contains `{}` is never filled in again.
pub fn tf(key: &str, args: &[&str]) -> String {
    fill(t(key), args)
}

fn fill(template: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut args = args.iter();
    let mut rest = template;
    while let Some(at) = rest.find("{}") {
        out.push_str(&rest[..at]);
        out.push_str(args.next().copied().unwrap_or("{}"));
        rest = &rest[at + 2..];
    }
    out.push_str(rest);
    out
}

macro_rules! strings {
    ($(($key:literal, $en:literal, $id:literal)),* $(,)?) => {
        /// Every key, generated from the table.
        #[cfg(test)]
        const KEYS: &[&str] = &[$($key),*];

        fn lookup(lang: Lang, key: &str) -> Option<&'static str> {
            match key {
                $($key => Some(match lang {
                    Lang::English => $en,
                    Lang::Indonesian => $id,
                }),)*
                _ => None,
            }
        }
    };
}

// (key, English, Indonesian)
strings![
    ("menu.new", "New Recording", "Rekaman Baru"),
    ("menu.open", "Open Meeting…", "Buka Rapat…"),
    ("menu.import", "Import Audio File…", "Impor Berkas Audio…"),
    ("menu.reveal", "Reveal in Finder", "Tampilkan di Finder"),
    ("menu.close_window", "Close Window", "Tutup Jendela"),
    ("menu.undo", "Undo", "Urungkan"),
    ("menu.redo", "Redo", "Ulangi"),
    ("menu.cut", "Cut", "Potong"),
    ("menu.copy", "Copy", "Salin"),
    ("menu.paste", "Paste", "Tempel"),
    ("menu.select_all", "Select All", "Pilih Semua"),
    ("menu.copy_transcript", "Copy Transcript", "Salin Transkrip"),
    ("menu.start", "Start Recording", "Mulai Merekam"),
    ("menu.pause_resume", "Pause / Resume", "Jeda / Lanjutkan"),
    ("menu.stop", "Stop Recording", "Hentikan Rekaman"),
    ("menu.compact", "Compact Strip", "Strip Ringkas"),
    ("menu.fullscreen", "Enter Full Screen", "Masuk Layar Penuh"),
    (
        "menu.transcribe_again",
        "Transcribe Again",
        "Transkripsikan Lagi"
    ),
    ("menu.window", "Window", "Jendela"),
    ("menu.help", "MOM Recorder Help", "Bantuan MOM Recorder"),
    ("menu.file", "File", "Berkas"),
    ("menu.edit", "Edit", "Sunting"),
    ("menu.recording", "Recording", "Perekaman"),
    ("menu.view", "View", "Tampilan"),
    ("ready.mic", "You (microphone)", "Kamu (mikrofon)"),
    ("ready.computer", "Computer audio", "Audio komputer"),
    ("ready.hint", "Drop to import", "Jatuhkan untuk mengimpor"),
    (
        "ready.import_hint",
        "Import an audio file, or drop one here",
        "Impor berkas audio, atau jatuhkan ke sini"
    ),
    ("ready.start", "Start recording", "Mulai merekam"),
    ("ready.stop", "Stop recording", "Hentikan perekaman"),
    ("ready.pause", "Pause", "Jeda"),
    ("ready.resume", "Resume", "Lanjutkan"),
    (
        "ready.status_idle",
        "Ready. Press Start recording when the meeting begins.",
        "Siap. Tekan Mulai merekam saat rapat dimulai."
    ),
    (
        "ready.status_recording",
        "Recording. Name, audio file and language can still be changed.",
        "Merekam. Nama, berkas audio, dan bahasa masih bisa diubah."
    ),
    (
        "ready.status_paused",
        "Paused. Nothing is recorded until you resume.",
        "Dijeda. Tidak ada yang direkam sampai dilanjutkan."
    ),
    (
        "ready.status_stopping",
        "Saving the audio…",
        "Menyimpan audio…"
    ),
    (
        "ready.status_transcribing",
        "Transcribing the meeting on this computer…",
        "Mentranskripsikan rapat di komputer ini…"
    ),
    ("ready.language_title", "Language", "Bahasa"),
    (
        "ready.language_subtitle",
        "Used for the transcript after the call",
        "Dipakai untuk transkrip setelah panggilan"
    ),
    ("ready.format_title", "Audio file", "Berkas audio"),
    ("ready.sources_title", "Record", "Rekam"),
    (
        "ready.enhance_title",
        "Voice enhancement",
        "Peningkatan suara"
    ),
    (
        "ready.enhance_subtitle",
        "Less noise and clearer voices in the saved audio; the transcript uses the original",
        "Lebih sedikit bising dan suara lebih jernih di audio tersimpan; transkrip memakai aslinya"
    ),
    (
        "enhance.no_helper",
        "the momr-audio helper is missing",
        "helper momr-audio tidak ada"
    ),
    (
        "enhance.unavailable",
        "voice isolation is not available on this Mac",
        "isolasi suara tidak tersedia di Mac ini"
    ),
    (
        "enhance.failed",
        "Saved without voice enhancement: {}",
        "Disimpan tanpa peningkatan suara: {}"
    ),
    (
        "ready.sources_subtitle",
        "Which side of the call is kept",
        "Sisi panggilan mana yang disimpan"
    ),
    ("ready.not_recorded", "Not recorded", "Tidak direkam"),
    (
        "sources.both",
        "Microphone and computer audio",
        "Mikrofon dan audio komputer"
    ),
    ("sources.mic", "Microphone only", "Hanya mikrofon"),
    (
        "sources.computer",
        "Computer audio only",
        "Hanya audio komputer"
    ),
    (
        "ready.format_subtitle",
        "Can be changed during the call",
        "Bisa diubah selama panggilan"
    ),
    (
        "ready.model_needed",
        "The speech model ({}, {}) is needed to transcribe",
        "Model wicara ({}, {}) diperlukan untuk transkripsi"
    ),
    ("ready.model_download", "Download", "Unduh"),
    (
        "ready.model_downloading",
        "Downloading the speech model…",
        "Mengunduh model wicara…"
    ),
    (
        "ready.model_progress",
        "Downloading the speech model… {:.0}%",
        "Mengunduh model wicara… {:.0}%"
    ),
    ("ready.button_saving", "Saving…", "Menyimpan…"),
    (
        "ready.button_transcribing",
        "Transcribing…",
        "Mentranskripsikan…"
    ),
    ("done.copy", "Copy transcript", "Salin transkrip"),
    (
        "done.no_transcript",
        "No transcript found",
        "Transkrip tidak ditemukan"
    ),
    ("done.new", "New recording", "Rekaman baru"),
    ("done.reveal", "Reveal in Finder", "Tampilkan di Finder"),
    ("done.language_again", "Language", "Bahasa"),
    (
        "done.transcribe_again",
        "Transcribe again",
        "Transkripsikan lagi"
    ),
    (
        "done.transcribe_again_hint",
        "Transcribe the meeting again with the selected language",
        "Transkripsikan lagi rapat ini dengan bahasa yang dipilih"
    ),
    (
        "done.transcribe_again_gone",
        "The separate tracks of this meeting are gone, so it cannot be transcribed again",
        "Trek terpisah rapat ini sudah hilang, jadi tidak bisa ditranskripsikan lagi"
    ),
    ("done.chapters", "Chapters", "Bab"),
    (
        "done.chapters_hint",
        "For meetings of three minutes or more",
        "Untuk rapat tiga menit atau lebih"
    ),
    (
        "done.chapters_added",
        "{} chapters added",
        "{} bab ditambahkan"
    ),
    ("done.chapters_none", "No chapters yet", "Belum ada bab"),
    (
        "done.chapters_writing",
        "Writing chapters with {}…",
        "Menulis bab dengan {}…"
    ),
    ("done.speakers", "Speakers", "Pembicara"),
    ("done.rename_meeting", "Meeting name", "Nama rapat"),
    (
        "done.recovery_title",
        "Unfinished recording found",
        "Rekaman yang belum selesai ditemukan"
    ),
    (
        "done.recovery_body",
        "A recording from {} ({}) was not stopped properly, probably because the app quit. Save it as a meeting?",
        "Rekaman dari {} ({}) tidak dihentikan dengan benar, mungkin karena aplikasi keluar. Simpan sebagai rapat?"
    ),
    ("done.recovery_discard", "Discard", "Buang"),
    ("done.recovery_later", "Later", "Nanti"),
    ("done.recovery_save", "Save", "Simpan"),
    (
        "done.close_recording_title",
        "Still recording",
        "Masih merekam"
    ),
    (
        "done.close_recording_body",
        "Closing stops the meeting. The audio is saved and transcribed first, then the app quits.",
        "Menutup menghentikan rapat. Audio disimpan dan ditranskripsikan dulu, lalu aplikasi keluar."
    ),
    (
        "done.close_recording_stop",
        "Stop and close",
        "Hentikan dan tutup"
    ),
    (
        "done.close_recording_keep",
        "Keep recording",
        "Lanjutkan merekam"
    ),
    (
        "done.close_transcribing_title",
        "Still transcribing",
        "Masih mentranskripsikan"
    ),
    (
        "done.close_transcribing_body",
        "The audio is already saved. The transcript is not finished yet.",
        "Audio sudah tersimpan. Transkripnya belum selesai."
    ),
    (
        "done.close_transcribing_later",
        "Close when done",
        "Tutup jika sudah selesai"
    ),
    ("done.close_transcribing_keep", "Keep open", "Tetap buka"),
    ("import.title", "Import an audio file", "Impor berkas audio"),
    (
        "import.subtitle_speakers",
        "Recognized by their voices",
        "Dikenali dari suaranya"
    ),
    ("import.dialog_title", "Import audio", "Impor audio"),
    ("import.confirm", "Import", "Impor"),
    ("import.cancel", "Cancel", "Batal"),
    ("import.auto_speakers", "Automatic", "Otomatis"),
    (
        "import.not_openable",
        "This is not a meeting the recorder can open",
        "Ini bukan rapat yang bisa dibuka perekam"
    ),
    (
        "banner.audio_permission",
        "Microphone permission was refused — allow it in System Settings › Privacy & Security › Microphone.",
        "Izin mikrofon ditolak — izinkan di Pengaturan Sistem › Privasi & Keamanan › Mikrofon."
    ),
    (
        "banner.audio_tap_permission",
        "System Audio Recording permission was refused — allow it in System Settings › Privacy & Security, then restart the app.",
        "Izin Perekaman Audio Sistem ditolak — izinkan di Pengaturan Sistem › Privasi & Keamanan, lalu buka ulang aplikasi."
    ),
    (
        "banner.audio_tap_unsupported",
        "This macOS cannot tap the system audio.",
        "macOS ini tidak bisa menyadap audio sistem."
    ),
    (
        "banner.audio_install_blackhole",
        "Install BlackHole to record the computer audio instead.",
        "Pasang BlackHole agar audio komputer ikut terekam."
    ),
    (
        "banner.audio_helper_missing",
        "The momr-audio helper was not found — reinstall MOM Recorder.",
        "Helper momr-audio tidak ditemukan — pasang ulang MOM Recorder."
    ),
    (
        "banner.audio_mic_failed",
        "Microphone capture failed (exit {}) — check System Settings › Privacy & Security › Microphone.",
        "Perekaman mikrofon gagal (exit {}) — periksa Pengaturan Sistem › Privasi & Keamanan › Mikrofon."
    ),
    (
        "banner.audio_no_ffmpeg",
        "Could not start microphone capture — is ffmpeg installed?",
        "Tidak bisa mulai perekaman mikrofon — apakah ffmpeg terpasang?"
    ),
    (
        "banner.audio_no_program",
        "Could not start {} — is it installed?",
        "Tidak bisa menjalankan {} — apakah terpasang?"
    ),
    (
        "strip.tooltip",
        "Compact strip (⇧⌘M)",
        "Strip ringkas (⇧⌘M)"
    ),
    (
        "strip.drag_hint",
        "Drag to move, ⇧⌘M to expand",
        "Seret untuk memindah, ⇧⌘M untuk melebarkan"
    ),
    ("prefs.title", "Settings", "Pengaturan"),
    ("prefs.transcription", "Transcription", "Transkripsi"),
    ("prefs.model", "Speech model", "Model wicara"),
    ("prefs.model_present", "On this Mac", "Di Mac ini"),
    (
        "prefs.model_size_gb",
        "About {size} GB, downloads on first use",
        "Sekitar {size} GB, diunduh saat pertama dipakai"
    ),
    (
        "prefs.model_size_mb",
        "About {size} MB, downloads on first use",
        "Sekitar {size} MB, diunduh saat pertama dipakai"
    ),
    ("prefs.language", "Default language", "Bahasa default"),
    (
        "prefs.provider",
        "Transcription provider",
        "Penyedia transkripsi"
    ),
    (
        "prefs.provider_local_note",
        "Nothing leaves this Mac",
        "Tidak ada yang keluar dari Mac ini"
    ),
    (
        "prefs.provider_eleven_note",
        "Sends meeting audio to ElevenLabs when transcribing",
        "Mengirim audio rapat ke ElevenLabs saat transkripsi"
    ),
    (
        "prefs.provider_google_note",
        "Sends meeting audio to Google Cloud when transcribing",
        "Mengirim audio rapat ke Google Cloud saat transkripsi"
    ),
    (
        "prefs.provider_openrouter_note",
        "Sends meeting audio to the chosen OpenRouter model when transcribing",
        "Mengirim audio rapat ke model OpenRouter pilihan saat transkripsi"
    ),
    (
        "prefs.eleven_key",
        "ElevenLabs API key",
        "Kunci API ElevenLabs"
    ),
    ("prefs.google_key", "Google API key", "Kunci API Google"),
    (
        "prefs.openrouter_key",
        "OpenRouter API key",
        "Kunci API OpenRouter"
    ),
    (
        "prefs.key_hint_eleven",
        "Dashboard › profile › API Keys (elevenlabs.io/app/settings/api-keys)",
        "Dasbor › profil › API Keys (elevenlabs.io/app/settings/api-keys)"
    ),
    (
        "prefs.key_hint_google",
        "Console › project › Speech-to-Text API › Credentials, restricted to the API",
        "Konsol › proyek › Speech-to-Text API › Credentials, dibatasi untuk API itu"
    ),
    (
        "prefs.key_hint_openrouter",
        "OpenRouter dashboard › Keys (openrouter.ai/settings/keys)",
        "Dasbor OpenRouter › Keys (openrouter.ai/settings/keys)"
    ),
    (
        "prefs.key_saved",
        "Saved in the Keychain",
        "Tersimpan di Keychain"
    ),
    (
        "prefs.key_saved_toast",
        "API key saved",
        "Kunci API tersimpan"
    ),
    ("prefs.chapters", "Chapters", "Bab"),
    (
        "prefs.chapters_about",
        "A coding agent with every tool switched off writes the chapter titles.",
        "Agen pengode dengan semua perkakas dimatikan yang menulis judul bab."
    ),
    ("prefs.agent", "Agent", "Agen"),
    ("prefs.agent_none", "None", "Tidak ada"),
    ("prefs.recording", "Recording", "Perekaman"),
    (
        "prefs.format",
        "Default audio format",
        "Format audio default"
    ),
    (
        "prefs.name",
        "Your name in transcripts",
        "Namamu di transkrip"
    ),
    ("prefs.meetings", "Meetings folder", "Folder rapat"),
    ("prefs.meetings_choose", "Choose…", "Pilih…"),
    ("prefs.audio", "Audio", "Audio"),
    ("prefs.mic", "Microphone", "Mikrofon"),
    (
        "prefs.mic_inputs",
        "{} input devices, default follows the system",
        "{} perangkat input, default mengikuti sistem"
    ),
    (
        "prefs.mic_none",
        "No input device found",
        "Tidak ada perangkat input"
    ),
    ("prefs.computer", "Computer audio", "Audio komputer"),
    (
        "prefs.computer_tap",
        "Records what the Mac plays",
        "Merekam yang dimainkan Mac"
    ),
    ("prefs.computer_blackhole", "Through {}", "Melalui {}"),
    (
        "prefs.computer_unavailable",
        "Unavailable: install BlackHole",
        "Tidak tersedia: pasang BlackHole"
    ),
    (
        "prefs.blackhole_how",
        "How to set up BlackHole",
        "Cara memasang BlackHole"
    ),
    ("prefs.menubar", "Menu Bar", "Bilah Menu"),
    (
        "prefs.menubar_show",
        "Show recording status",
        "Tampilkan status perekaman"
    ),
    (
        "prefs.menubar_restart",
        "Takes effect on the next launch",
        "Berlaku saat dibuka berikutnya"
    ),
    (
        "prefs.ui_language",
        "Interface language",
        "Bahasa antarmuka"
    ),
    (
        "prefs.ui_language_hint",
        "Bahasa Indonesia or English. Takes effect on the next launch.",
        "Bahasa Indonesia atau English. Berlaku saat dibuka berikutnya."
    ),
    (
        "about.comments",
        "Two-track meeting recorder: your microphone and the computer audio, transcribed on this Mac.",
        "Perekam rapat dua trek: mikrofonmu dan audio komputermu, ditranskripsikan di Mac ini."
    ),
    ("about.transcription_credit", "Transcription", "Transkripsi"),
    (
        "about.based_on",
        "Based on Meeting Recorder by Jankees van Woezik",
        "Berdasarkan Meeting Recorder oleh Jankees van Woezik"
    ),
    (
        "help.finish_first",
        "Finish the current recording first",
        "Selesaikan dulu perekaman yang berjalan"
    ),
    ("help.line_deleted", "Line deleted", "Baris dihapus"),
    ("help.saved", "Saved", "Tersimpan"),
    (
        "help.copied_clipboard",
        "Copied to clipboard",
        "Disalin ke papan klip"
    ),
    (
        "notify.transcribed",
        "Meeting transcribed",
        "Rapat ditranskripsikan"
    ),
    ("row.edit", "Edit this line", "Sunting baris ini"),
    ("row.next_speaker", "Next speaker", "Pembicara berikutnya"),
    ("row.delete", "Delete this line", "Hapus baris ini"),
    ("row.play_from", "Play from {}", "Putar dari {}"),
    ("chapter.play_from", "Play from {}", "Putar dari {}"),
    ("chapters.generate", "Generate", "Buat"),
    ("chapters.redo", "Redo", "Ulangi"),
    ("chapters.made_with", "Made with {}", "Dibuat dengan {}"),
    (
        "chapters.let_divide",
        "Let {} divide the meeting into chapters",
        "Minta {} membagi rapat menjadi bab"
    ),
    (
        "chapters.could_not",
        "{} could not make chapters",
        "{} tidak bisa membuat bab"
    ),
    ("import.filter", "Audio and video", "Audio dan video"),
    ("import.importing", "Importing audio", "Mengimpor audio"),
    (
        "import.no_audio",
        "this file has no audio ffmpeg can read",
        "berkas ini tidak punya audio yang bisa dibaca ffmpeg"
    ),
    (
        "import.no_convert",
        "could not convert the audio",
        "tidak bisa mengonversi audio"
    ),
    (
        "open.title",
        "Open a meeting (.meeting-recorder file)",
        "Buka rapat (berkas .meeting-recorder)"
    ),
    ("open.filter", "Meeting recordings", "Rekaman rapat"),
    (
        "close.cancel_transcription",
        "Cancel transcription",
        "Batalkan transkripsi"
    ),
    ("canvas.not_recording", "Not recording", "Tidak merekam"),
    ("canvas.paused", "PAUSED", "DIJEDA"),
    ("done.meeting_saved", "Meeting saved", "Rapat tersimpan"),
    (
        "done.transcript_ready",
        "Transcript ready",
        "Transkrip siap"
    ),
    ("done.imported_audio", "Imported audio", "Audio impor"),
    (
        "done.recovered_title",
        "Recovered recording",
        "Rekaman pulihan"
    ),
    ("done.fallback_title", "Meeting", "Rapat"),
    (
        "help.stopped_unexpectedly",
        "the import stopped unexpectedly",
        "impor berhenti tiba-tiba"
    ),
    (
        "help.transcription_stopped",
        "transcription stopped unexpectedly",
        "transkripsi berhenti tiba-tiba"
    ),
    (
        "help.no_meeting_folder",
        "no meeting folder",
        "tidak ada folder rapat"
    ),
    (
        "help.write_failed",
        "could not write the transcript: {}",
        "tidak bisa menulis transkrip: {}"
    ),
    (
        "help.could_not_save_transcript",
        "Could not save the transcript",
        "Tidak bisa menyimpan transkrip"
    ),
    (
        "help.model_ready",
        "Speech model ready",
        "Model wicara siap"
    ),
    (
        "help.model_download_failed",
        "Could not download the model: {}",
        "Tidak bisa mengunduh model: {}"
    ),
    (
        "help.download_stopped",
        "the download stopped",
        "unduhan berhenti"
    ),
    (
        "help.not_openable",
        "not a meeting the recorder can open",
        "bukan rapat yang bisa dibuka perekam"
    ),
    (
        "help.no_transcript_yet",
        "This meeting has no transcript yet.",
        "Rapat ini belum punya transkrip."
    ),
    (
        "help.cancelled",
        "Transcription cancelled.",
        "Transkripsi dibatalkan."
    ),
    (
        "help.failed",
        "Transcription failed: {}.",
        "Transkripsi gagal: {}."
    ),
    (
        "help.no_audio",
        "Could not save the audio.",
        "Tidak bisa menyimpan audio."
    ),
    (
        "help.could_not_start",
        "Could not start recording: {}",
        "Tidak bisa mulai merekam: {}"
    ),
    ("help.stages_saving", "Saving audio", "Menyimpan audio"),
    ("help.stages_loading", "Loading audio", "Memuat audio"),
    ("help.stages_done", "Done", "Selesai"),
    (
        "help.folder_exists",
        "A meeting folder with that name already exists",
        "Folder rapat dengan nama itu sudah ada"
    ),
    (
        "help.rename_failed",
        "Could not rename the folder: {}",
        "Tidak bisa mengganti nama folder: {}"
    ),
    (
        "help.every_speaker",
        "Every speaker needs a different name",
        "Setiap pembicara butuh nama berbeda"
    ),
    (
        "speaker.row_mic",
        "Speaker on the microphone",
        "Pembicara di mikrofon"
    ),
    (
        "speaker.row_computer",
        "Speaker on the computer audio",
        "Pembicara di audio komputer"
    ),
    (
        "speaker.row_computer_n",
        "Speaker {} on the computer audio",
        "Pembicara {} di audio komputer"
    ),
    ("speaker.empty_fallback", "Speaker {}", "Pembicara {}"),
    ("stage.loading_model", "Loading model", "Memuat model"),
    ("stage.transcribing", "Transcribing", "Mentranskripsikan"),
    ("stage.loading_audio", "Loading audio", "Memuat audio"),
    (
        "stage.finding_speakers",
        "Finding speakers",
        "Mencari pembicara"
    ),
    ("download.model", "Downloading model", "Mengunduh model"),
    (
        "download.speaker",
        "Downloading the speaker model",
        "Mengunduh model pembicara"
    ),
    ("player.play", "Play", "Putar"),
    ("player.pause", "Pause", "Jeda"),
    ("player.back", "Back 15 seconds", "Mundur 15 detik"),
    ("player.forward", "Forward 15 seconds", "Maju 15 detik"),
    ("player.previous_line", "Previous line", "Baris sebelumnya"),
    ("player.next_line", "Next line", "Baris berikutnya"),
    ("player.speed", "Playback speed", "Kecepatan putar"),
    ("player.volume", "Volume", "Volume"),
    ("player.mute", "Mute", "Bisukan"),
    ("player.unmute", "Unmute", "Bunyikan"),
    ("player.lane_mic", "Microphone", "Mikrofon"),
    ("player.lane_computer", "Computer audio", "Audio komputer"),
    (
        "player.keys",
        "Space plays or pauses, ← and → skip 5 seconds",
        "Spasi memutar atau menjeda, ← dan → lompat 5 detik"
    ),
    ("format.mono", "Mono", "Mono"),
    (
        "format.stereo",
        "Stereo (mic left, computer right)",
        "Stereo (mic kiri, komputer kanan)"
    ),
    ("format.separate", "Separate files", "Berkas terpisah"),
    ("format.short_mono", "Mono", "Mono"),
    ("format.short_stereo", "Stereo", "Stereo"),
    ("format.short_separate", "Separate files", "Berkas terpisah"),
    ("lang.auto", "Auto-detect", "Otomatis"),
    ("lang.en", "English", "Inggris"),
    ("lang.id", "Indonesian", "Indonesia"),
    ("lang.nl", "Dutch", "Belanda"),
    ("lang.de", "German", "Jerman"),
    ("lang.fr", "French", "Prancis"),
    ("lang.es", "Spanish", "Spanyol"),
    ("lang.it", "Italian", "Italia"),
    ("lang.pt", "Portuguese", "Portugis"),
    (
        "provider.name_local",
        "On this Mac (whisper)",
        "Di Mac ini (whisper)"
    ),
    ("provider.name_eleven", "ElevenLabs", "ElevenLabs"),
    (
        "provider.name_google",
        "Google Cloud Speech-to-Text",
        "Google Cloud Speech-to-Text"
    ),
    ("provider.name_openrouter", "OpenRouter", "OpenRouter"),
    ("misc.undo", "Undo", "Urungkan"),
    (
        "cli.usage",
        "Usage: {} [start | stop | pause | compact | watch | transcribe <mic> <computer> [--language xx]]",
        "Pakai: {} [start | stop | pause | compact | watch | transcribe <mic> <computer> [--language xx]]"
    ),
    (
        "cli.no_command",
        "(no command)  open the recorder, ready to record",
        "(tanpa perintah)  buka perekam, siap merekam"
    ),
    (
        "cli.meeting",
        "<meeting>     open a .meeting-recorder file or a meeting folder",
        "<rapat>       buka berkas .meeting-recorder atau folder rapat"
    ),
    (
        "cli.start",
        "start         start recording in the open window (for a keybinding)",
        "start         mulai merekam di jendela yang terbuka (untuk pintasan)"
    ),
    (
        "cli.stop",
        "stop          stop the running recording (for a keybinding)",
        "stop          hentikan perekaman yang berjalan (untuk pintasan)"
    ),
    (
        "cli.compact",
        "compact       switch the recording window between full and compact",
        "compact       alihkan jendela perekaman antara penuh dan ringkas"
    ),
    (
        "cli.pause",
        "pause         pause or resume the running recording",
        "pause         jeda atau lanjutkan rekaman yang berjalan"
    ),
    (
        "cli.watch",
        "watch         stream the recorder state as NDJSON, for a menu bar item or any other client",
        "watch         alirkan status perekam sebagai NDJSON, untuk item bilah menu atau klien lain"
    ),
    (
        "cli.transcribe",
        "transcribe    transcribe two tracks and print the transcript as Markdown",
        "transcribe    transkripsikan dua trek dan cetak transkrip sebagai Markdown"
    ),
    (
        "cli.finish",
        "finish        save a stopped recording from its staging folder: audio, manifest, transcript",
        "finish        simpan rekaman yang dihentikan dari folder stagingnya: audio, manifest, transkrip"
    ),
    (
        "cli.ask",
        "ask           run a prompt over stdin through the default agent, without tools",
        "ask           jalankan prompt lewat stdin melalui agen default, tanpa perkakas"
    ),
    (
        "cli.not_running",
        "the recorder is not running",
        "perekam tidak berjalan"
    ),
    (
        "cli.diarize",
        "Usage: {} diarize <audio> [--speakers N]",
        "Pakai: {} diarize <audio> [--speakers N]"
    ),
    (
        "cli.unknown",
        "unknown command '{}', see --help",
        "perintah '{}' tidak dikenal, lihat --help"
    ),
    (
        "agent.unset",
        "No agent set. Add agent = \"claude\" to {}",
        "Agen belum diatur. Tambahkan agent = \"claude\" ke {}"
    ),
    ("agent.missing", "{} is not installed", "{} belum terpasang"),
    (
        "agent.refused_agy",
        "Antigravity only offers a blanket sandbox, not a way to remove its tools, so the recorder will not send your transcript to it",
        "Antigravity hanya menawarkan sandbox umum, bukan cara mematikan perkakasnya, jadi perekam tidak akan mengirim transkripmu ke sana"
    ),
    (
        "agent.refused_crush",
        "Crush has no flag to run without tools, so the recorder will not send your transcript to it",
        "Crush tidak punya flag untuk berjalan tanpa perkakas, jadi perekam tidak akan mengirim transkripmu ke sana"
    ),
    (
        "agent.refused_unknown",
        "The recorder does not know how to run {} without tools",
        "Perekam tidak tahu cara menjalankan {} tanpa perkakas"
    ),
    (
        "agent.ori_needs",
        "Ori needs Claude Code or Pi installed to work on a transcript",
        "Ori butuh Claude Code atau Pi terpasang agar bisa mengolah transkrip"
    ),
    (
        "agent.ori_harness",
        "Ori needs Claude Code or Pi installed",
        "Ori butuh Claude Code atau Pi terpasang"
    ),
    (
        "agent.opencode_denied",
        "OpenCode did not come back with every tool denied, so the recorder will not send your transcript to it",
        "OpenCode tidak kembali dengan semua perkakas dimatikan, jadi perekam tidak akan mengirim transkripmu ke sana"
    ),
    (
        "agent.grok_setup",
        "Grok has not been set up yet. Run grok once in a terminal, then try again.",
        "Grok belum disiapkan. Jalankan grok sekali di terminal, lalu coba lagi."
    ),
    (
        "agent.too_long",
        "The transcript is too long to send to the agent",
        "Transkrip terlalu panjang untuk dikirim ke agen"
    ),
    (
        "agent.no_workdir",
        "Could not make a working directory: {}",
        "Tidak bisa membuat direktori kerja: {}"
    ),
    (
        "agent.no_start",
        "Could not start {}: {}",
        "Tidak bisa menjalankan {}: {}"
    ),
    (
        "agent.no_answer",
        "{} did not answer within {} seconds",
        "{} tidak menjawab dalam {} detik"
    ),
    (
        "agent.exited",
        "{} exited with status {}",
        "{} keluar dengan status {}"
    ),
    (
        "agent.nothing",
        "{} returned nothing",
        "{} tidak mengembalikan apa-apa"
    ),
    (
        "ask.usage",
        "Usage: {} ask \"<prompt>\" < text | ask --agent",
        "Pakai: {} ask \"<prompt>\" < text | ask --agent"
    ),
    (
        "ask.no_stdin",
        "could not read the text from stdin",
        "tidak bisa membaca teks dari stdin"
    ),
    (
        "help.agent_stopped",
        "the agent stopped unexpectedly",
        "agen berhenti tiba-tiba"
    ),
    (
        "banner.audio_tap_failed",
        "System audio capture failed in Core Audio — retrying.",
        "Perekaman audio sistem gagal di Core Audio — mencoba lagi."
    ),
    (
        "banner.audio_conversion",
        "The audio could not be converted for recording — try another device in System Settings › Sound.",
        "Audio tidak bisa dikonversi untuk direkam — coba perangkat lain di Pengaturan Sistem › Suara."
    ),
    (
        "banner.audio_tap_fell_back",
        "Recording through BlackHole instead, which only hears what is routed to it: send the output through a Multi-Output Device.",
        "Merekam lewat BlackHole sebagai gantinya, yang hanya mendengar audio yang diarahkan ke sana: kirim keluaran lewat Multi-Output Device."
    ),
    (
        "banner.audio_no_mic",
        "No microphone was found — connect one or pick it in System Settings › Sound.",
        "Mikrofon tidak ditemukan — sambungkan atau pilih di Pengaturan Sistem › Suara."
    ),
    (
        "banner.audio_write_failed",
        "Audio could not be written to disk ({}) — part of the recording is lost.",
        "Audio tidak bisa ditulis ke disk ({}) — sebagian rekaman hilang."
    ),
    (
        "cli.transcribe_file",
        "transcribe-file  transcribe one audio file: --speakers N, --language xx, --model name, --provider local|elevenlabs|google|openrouter",
        "transcribe-file  transkripsikan satu berkas audio: --speakers N, --language xx, --model nama, --provider local|elevenlabs|google|openrouter"
    ),
    (
        "cli.diarize_help",
        "diarize       tell the voices in an audio file apart and print who speaks when",
        "diarize       bedakan suara dalam berkas audio dan cetak siapa berbicara kapan"
    ),
    ("cli.done_in", "Done in {}s", "Selesai dalam {} dtk"),
    (
        "provider.unknown_id",
        "config.toml names an unknown provider \"{}\"; use local, elevenlabs, google or openrouter.",
        "config.toml menyebut penyedia yang tidak dikenal \"{}\"; gunakan local, elevenlabs, google, atau openrouter."
    ),
    (
        "provider.key_missing",
        "No API key saved for {}. Add one in Settings › Transcription.",
        "Belum ada kunci API untuk {}. Tambahkan di Pengaturan › Transkripsi."
    ),
    (
        "provider.key_unreadable",
        "Could not read the {} key from the Keychain: {}",
        "Kunci {} tidak bisa dibaca dari Keychain: {}"
    ),
    (
        "provider.key_empty",
        "Paste the whole key on one line; an empty key is not saved.",
        "Tempel seluruh kunci dalam satu baris; kunci kosong tidak disimpan."
    ),
    (
        "provider.key_save_failed",
        "Could not save the API key: {}",
        "Kunci API tidak bisa disimpan: {}"
    ),
    (
        "provider.google_needs_language",
        "Google Cloud cannot detect the language: pick one first (the transcription language, or --language on the command line).",
        "Google Cloud tidak bisa mendeteksi bahasa: pilih dulu bahasanya (bahasa transkripsi, atau --language di baris perintah)."
    ),
    (
        "provider.err_not_json",
        "{} returned text that is not JSON: {}",
        "{} mengembalikan teks yang bukan JSON: {}"
    ),
    ("provider.err_failed", "{} failed: {}", "{} gagal: {}"),
    (
        "provider.err_rejected",
        "{} rejected the API key: {}",
        "{} menolak kunci API: {}"
    ),
    (
        "provider.err_credits",
        "{} is out of credits: {}",
        "Kredit {} habis: {}"
    ),
    (
        "provider.err_quota",
        "{} is out of quota: {}",
        "Kuota {} habis: {}"
    ),
    (
        "provider.err_request",
        "{} request failed: {}",
        "Permintaan ke {} gagal: {}"
    ),
    (
        "provider.err_shape",
        "{} answered without the expected {} field; its API may have changed.",
        "{} menjawab tanpa kolom {} yang diharapkan; API-nya mungkin berubah."
    ),
    (
        "provider.chunk_failed",
        "Stopped at part {} of {}: {}",
        "Berhenti di bagian {} dari {}: {}"
    ),
    (
        "provider.ffmpeg_missing",
        "ffmpeg could not run: {}",
        "ffmpeg tidak bisa dijalankan: {}"
    ),
    (
        "provider.ffmpeg_google",
        "ffmpeg could not prepare the audio for Google",
        "ffmpeg tidak bisa menyiapkan audio untuk Google"
    ),
    (
        "provider.ffmpeg_cut",
        "ffmpeg could not cut the audio for upload",
        "ffmpeg tidak bisa memotong audio untuk diunggah"
    ),
    (
        "provider.workdir_failed",
        "Could not make a working folder: {}",
        "Folder kerja tidak bisa dibuat: {}"
    ),
    (
        "provider.stage",
        "Transcribing with {}",
        "Mentranskripsikan dengan {}"
    ),
    (
        "provider.stage_failed",
        "Could not stage the audio for upload: {}",
        "Audio tidak bisa disiapkan untuk diunggah: {}"
    ),
    (
        "help.socket_failed",
        "The menu bar item and momr stop cannot reach this window ({}).",
        "Item bilah menu dan momr stop tidak bisa menjangkau jendela ini ({})."
    ),
    (
        "player.failed",
        "Playback stopped: {}",
        "Pemutaran berhenti: {}"
    ),
    (
        "prefs.save_failed",
        "The setting was not saved: {}",
        "Pengaturan tidak tersimpan: {}"
    ),
    (
        "prefs.devices_unknown",
        "Could not list the devices: {}",
        "Perangkat tidak bisa didaftar: {}"
    ),
    (
        "ready.status_transcribing_cloud",
        "Uploading the audio to {} and transcribing…",
        "Mengunggah audio ke {} dan mentranskripsikan…"
    ),
    ("speaker.row_import", "Speaker {}", "Pembicara {}"),
    (
        "banner.computer_silent",
        "The computer audio stayed completely silent. If the call should be in it, allow System Audio Recording in System Settings › Privacy & Security.",
        "Audio komputer benar-benar senyap. Jika panggilan seharusnya terekam, izinkan Perekaman Audio Sistem di Pengaturan Sistem › Privasi & Keamanan."
    ),
    (
        "prefs.computer_tap_denied",
        "System Audio Recording is refused for this app",
        "Perekaman Audio Sistem ditolak untuk aplikasi ini"
    ),
    ("prefs.open_privacy", "Open Settings", "Buka Pengaturan"),
    // Plan 14: the settings button, the pages, the appearance switch, the
    // key expanders, the folded sidebar.
    ("prefs.button", "Settings (⌘,)", "Pengaturan (⌘,)"),
    ("prefs.page_general", "General", "Umum"),
    ("prefs.interface", "Interface", "Antarmuka"),
    ("prefs.appearance", "Appearance", "Tampilan"),
    ("prefs.appearance_system", "System", "Sistem"),
    ("prefs.appearance_light", "Light", "Terang"),
    ("prefs.appearance_dark", "Dark", "Gelap"),
    ("prefs.keys", "API keys", "Kunci API"),
    (
        "prefs.keys_about",
        "Only the chosen cloud provider needs its key. Keys stay in the macOS Keychain.",
        "Hanya penyedia cloud yang dipilih yang butuh kuncinya. Kunci disimpan di Keychain macOS."
    ),
    ("prefs.key_none", "No key yet", "Belum ada kunci"),
    ("prefs.key_paste", "Paste the key", "Tempel kuncinya"),
    ("prefs.key_where", "Where to get it", "Cara mendapatkannya"),
    (
        "done.sidebar",
        "Show the meeting panel",
        "Tampilkan panel rapat"
    ),
    // Plan 15: the timer.
    ("menu.timer", "Timer…", "Pengatur Waktu…"),
    ("timer.title", "Timer", "Pengatur waktu"),
    ("timer.row", "Timer", "Pengatur waktu"),
    ("timer.off", "Off", "Mati"),
    ("timer.set", "Set", "Atur"),
    ("timer.stop_after", "Stop after", "Hentikan setelah"),
    ("timer.start_at", "Start at", "Mulai pada"),
    ("timer.stop_at", "Stop at", "Hentikan pada"),
    ("timer.hours_row", "Hours", "Jam"),
    ("timer.minutes_row", "Minutes", "Menit"),
    (
        "timer.times_hint",
        "A time that has already passed today means tomorrow.",
        "Waktu yang sudah lewat hari ini berarti besok."
    ),
    (
        "timer.needs_length",
        "Set a length of at least one minute",
        "Atur durasi minimal satu menit"
    ),
    (
        "timer.no_such_time",
        "That time is skipped by the clock (a daylight-saving change); pick another",
        "Waktu itu dilewati jam (pergantian waktu musim panas); pilih waktu lain"
    ),
    ("timer.starts_at", "Starts {}", "Mulai {}"),
    ("timer.stops_after", "Stops after {}", "Berhenti setelah {}"),
    ("timer.stops_at", "Stops {}", "Berhenti {}"),
    ("timer.hours", "{} h", "{} jam"),
    ("timer.minutes", "{} min", "{} mnt"),
    (
        "timer.status_scheduled",
        "Recording starts on its own at {}.",
        "Perekaman mulai sendiri pada {}."
    ),
    (
        "timer.status_countdown",
        "Recording. Stops in {}.",
        "Merekam. Berhenti dalam {}."
    ),
    (
        "timer.one_minute",
        "One minute left on the timer",
        "Satu menit lagi sebelum pengatur waktu berhenti"
    ),
    (
        "prefs.timer_about",
        "What the Timer dialog opens with.",
        "Nilai awal dialog pengatur waktu."
    ),
    (
        "prefs.timer_default",
        "Stop after (minutes)",
        "Hentikan setelah (menit)"
    ),
    // Plan 15: audio sources.
    ("prefs.mic_default", "System default", "Default sistem"),
    (
        "prefs.mic_hint",
        "Which input is recorded; a change applies at once",
        "Input mana yang direkam; perubahan berlaku seketika"
    ),
    ("prefs.computer_scope", "Record", "Rekam"),
    (
        "prefs.computer_scope_hint",
        "Every app, or only the ones switched on below",
        "Semua aplikasi, atau hanya yang diaktifkan di bawah"
    ),
    ("prefs.computer_all", "All apps", "Semua aplikasi"),
    (
        "prefs.computer_chosen",
        "Only chosen apps",
        "Hanya aplikasi pilihan"
    ),
    ("prefs.computer_status", "Status", "Status"),
    ("prefs.apps", "Apps to record", "Aplikasi yang direkam"),
    (
        "prefs.apps_about",
        "Apps with audio right now, and the chosen ones that are not running. A chosen app is heard as soon as it starts.",
        "Aplikasi yang sedang bersuara, dan yang dipilih tapi tidak berjalan. Aplikasi pilihan terdengar begitu dijalankan."
    ),
    (
        "prefs.apps_none",
        "No app is playing audio now",
        "Tidak ada aplikasi yang bersuara sekarang"
    ),
    ("prefs.app_playing", "playing now", "sedang bersuara"),
    ("prefs.app_not_running", "Not running", "Tidak berjalan"),
    // Plan 15: storage.
    ("prefs.storage", "Storage", "Penyimpanan"),
    (
        "storage.about",
        "Meetings in the meetings folder and API keys in the Keychain are never touched here.",
        "Rapat di folder rapat dan kunci API di Keychain tidak pernah disentuh di sini."
    ),
    ("storage.cache", "Cache", "Cache"),
    ("storage.models", "Speech models", "Model wicara"),
    ("storage.settings", "Settings", "Pengaturan"),
    (
        "storage.settings_hint",
        "settings.json and config.toml back to their defaults",
        "settings.json dan config.toml kembali ke default"
    ),
    (
        "storage.unfinished",
        "{} unfinished recordings",
        "{} rekaman belum selesai"
    ),
    ("storage.clear", "Clear…", "Bersihkan…"),
    ("storage.delete", "Delete…", "Hapus…"),
    ("storage.reset", "Reset…", "Setel ulang…"),
    ("storage.cleared", "{} freed", "{} dibebaskan"),
    (
        "storage.failed",
        "Could not clear it: {}",
        "Tidak bisa dibersihkan: {}"
    ),
    (
        "storage.cache_title",
        "Clear the cache?",
        "Bersihkan cache?"
    ),
    (
        "storage.cache_body",
        "Staging files and import scratch go; meetings and settings stay.",
        "Berkas staging dan sisa impor dihapus; rapat dan pengaturan tetap."
    ),
    (
        "storage.cache_unfinished",
        "This includes {} unfinished recordings that could still be saved as meetings.",
        "Termasuk {} rekaman belum selesai yang masih bisa disimpan sebagai rapat."
    ),
    (
        "storage.models_title",
        "Delete the speech models?",
        "Hapus model wicara?"
    ),
    (
        "storage.models_body",
        "The next transcription downloads the model again. Meetings and transcripts stay.",
        "Transkripsi berikutnya mengunduh modelnya lagi. Rapat dan transkrip tetap."
    ),
    (
        "storage.settings_title",
        "Reset the settings?",
        "Setel ulang pengaturan?"
    ),
    (
        "storage.settings_body",
        "Every setting returns to its default, including the model, the provider and the agent. API keys stay in the Keychain; meetings stay.",
        "Semua pengaturan kembali ke default, termasuk model, penyedia, dan agen. Kunci API tetap di Keychain; rapat tetap."
    ),
    (
        "storage.settings_reset",
        "Settings reset",
        "Pengaturan disetel ulang"
    ),
    (
        "storage.everything",
        "Reset MOM Recorder…",
        "Setel ulang MOM Recorder…"
    ),
    (
        "storage.everything_about",
        "Cache, speech models and settings in one go.",
        "Cache, model wicara, dan pengaturan sekaligus."
    ),
    (
        "storage.everything_title",
        "Reset MOM Recorder?",
        "Setel ulang MOM Recorder?"
    ),
    (
        "storage.everything_body",
        "Clears the cache, deletes the downloaded speech models and resets every setting. Meetings and API keys stay.",
        "Membersihkan cache, menghapus model wicara yang diunduh, dan menyetel ulang semua pengaturan. Rapat dan kunci API tetap."
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_has_text_and_matching_placeholders() {
        let mut bad = Vec::new();
        for key in KEYS {
            let en = t_in(Lang::English, key);
            let id = t_in(Lang::Indonesian, key);
            if en.is_empty()
                || id.is_empty()
                || en.matches("{}").count() != id.matches("{}").count()
            {
                bad.push(*key);
            }
        }
        assert!(
            bad.is_empty(),
            "empty text or placeholder mismatch: {bad:?}"
        );
    }

    /// Every `t("…")`/`tf("…")` literal in the sources is in the table, so a
    /// typo shows up here instead of as "missing string" on screen.
    #[test]
    fn every_looked_up_key_exists() {
        let sources = [
            include_str!("agent.rs"),
            include_str!("audio.rs"),
            include_str!("chapters.rs"),
            include_str!("cleanup.rs"),
            include_str!("diarize.rs"),
            include_str!("export.rs"),
            include_str!("finish.rs"),
            include_str!("helper.rs"),
            include_str!("ipc.rs"),
            include_str!("meeting.rs"),
            include_str!("models.rs"),
            include_str!("nemotron.rs"),
            include_str!("playback.rs"),
            include_str!("provider.rs"),
            include_str!("settings.rs"),
            include_str!("timer.rs"),
            include_str!("transcribe.rs"),
            // The GTK shell looks its strings up in this table too, so its
            // sources are scanned from here, three levels up at the root.
            include_str!("../../../src/main.rs"),
            include_str!("../../../src/player.rs"),
            include_str!("../../../src/ui.rs"),
        ];
        let mut missing = Vec::new();
        for source in sources {
            for call in ["t(", "tf(", "t_in(Lang::English,"] {
                for (at, _) in source.match_indices(call) {
                    // `set_text(` also ends in `t(`: only a call whose name
                    // starts here counts.
                    let before = source[..at].chars().next_back();
                    if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                        continue;
                    }
                    // rustfmt may break the line before a long key.
                    let Some(rest) = source[at + call.len()..].trim_start().strip_prefix('"')
                    else {
                        continue;
                    };
                    let Some(end) = rest.find('"') else { continue };
                    let key = &rest[..end];
                    if key.contains('.') && !key.contains(' ') && !KEYS.contains(&key) {
                        missing.push(key.to_owned());
                    }
                }
            }
        }
        missing.sort();
        missing.dedup();
        assert!(missing.is_empty(), "keys not in the table: {missing:?}");
    }

    #[test]
    fn unknown_key_falls_back_to_a_marker() {
        assert_eq!(t("no.such.key"), "missing string");
    }

    #[test]
    fn fill_is_one_pass() {
        assert_eq!(fill("{} and {}", &["a{}", "b"]), "a{} and b");
        assert_eq!(fill("{} only", &[]), "{} only");
    }

    #[test]
    fn launch_language_prefers_settings_then_system_then_env() {
        use Lang::{English, Indonesian};
        assert_eq!(resolve_lang(Some(English), Some("id-ID"), "id_ID"), English);
        assert_eq!(resolve_lang(None, Some("\"id-ID\""), "en-US"), Indonesian);
        assert_eq!(resolve_lang(None, None, "id_ID.UTF-8"), Indonesian);
        assert_eq!(resolve_lang(None, None, ""), English);
    }

    #[test]
    fn languages_parse() {
        assert_eq!(Lang::from_locale("id_ID.UTF-8"), Lang::Indonesian);
        assert_eq!(Lang::from_locale("\"id-ID\""), Lang::Indonesian);
        assert_eq!(Lang::from_locale("en-GB"), Lang::English);
        assert_eq!(Lang::from_locale(""), Lang::English);
        assert_eq!(
            first_apple_language("(\n    \"id-ID\",\n    \"en-US\"\n)\n").as_deref(),
            Some("id-ID")
        );
        assert_eq!(first_apple_language("()"), None);
        for lang in [Lang::English, Lang::Indonesian] {
            assert_eq!(Lang::from_code(lang.code()), Some(lang));
        }
    }
}
