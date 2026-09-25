//! Interface language: English or Indonesian. Every user-visible literal in
//! the app goes through `t()`, with English and Indonesian tables holding
//! exactly the same keys (the completeness test enforces it). Format
//! arguments stay positional (`{}`) with matching order in both languages.
//! Transcript *content* (names, markdown) is untouched; only the chrome
//! translates. A language change takes effect on the next launch: the widgets
//! are built once at startup.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    English,
    Indonesian,
}

/// The interface language from settings.
pub fn current() -> Lang {
    match crate::settings::load_ui_language() {
        "id" => Lang::Indonesian,
        _ => Lang::English,
    }
}

/// The string for `key` in the current language.
pub fn t(key: &str) -> &'static str {
    t_in(current(), key)
}

pub fn t_in(lang: Lang, key: &str) -> &'static str {
    match lang {
        Lang::English => en(key),
        Lang::Indonesian => id(key).unwrap_or_else(|| en(key)),
    }
}

macro_rules! table {
    ($(($key:literal, $text:literal)),* $(,)?) => {
        |key: &str| -> Option<&'static str> {
            match key {
                $($key => Some($text),)*
                _ => None,
            }
        }
    };
}

fn en(key: &str) -> &'static str {
    let lookup = table![
        ("menu.new", "New Recording"),
        ("menu.open", "Open Meeting…"),
        ("menu.import", "Import Audio File…"),
        ("menu.reveal", "Reveal in Finder"),
        ("menu.close_window", "Close Window"),
        ("menu.undo", "Undo"),
        ("menu.redo", "Redo"),
        ("menu.cut", "Cut"),
        ("menu.copy", "Copy"),
        ("menu.paste", "Paste"),
        ("menu.select_all", "Select All"),
        ("menu.copy_transcript", "Copy Transcript"),
        ("menu.start", "Start Recording"),
        ("menu.pause_resume", "Pause / Resume"),
        ("menu.stop", "Stop Recording"),
        ("menu.compact", "Compact Strip"),
        ("menu.fullscreen", "Enter Full Screen"),
        ("menu.transcribe_again", "Transcribe Again"),
        ("menu.window", "Window"),
        ("menu.help", "MOM Recorder Help"),
        ("menu.file", "File"),
        ("menu.edit", "Edit"),
        ("menu.recording", "Recording"),
        ("menu.view", "View"),
        ("ready.mic", "You (microphone)"),
        ("ready.computer", "Computer audio"),
        ("ready.hint", "Drop to import"),
        (
            "ready.import_hint",
            "Import an audio file, or drop one here"
        ),
        ("ready.start", "Start recording"),
        ("ready.stop", "Stop recording"),
        ("ready.pause", "Pause"),
        ("ready.resume", "Resume"),
        (
            "ready.status_idle",
            "Ready. Press Start recording when the meeting begins."
        ),
        (
            "ready.status_recording",
            "Recording. Name, audio file and language can still be changed."
        ),
        (
            "ready.status_paused",
            "Paused. Nothing is recorded until you resume."
        ),
        ("ready.status_stopping", "Saving the audio…"),
        (
            "ready.status_transcribing",
            "Transcribing the meeting on this computer…"
        ),
        ("ready.language_title", "Language"),
        (
            "ready.language_subtitle",
            "Used for the transcript after the call"
        ),
        ("ready.format_title", "Audio file"),
        ("ready.format_subtitle", "Can be changed during the call"),
        ("ready.import_button", "Import an audio file"),
        (
            "ready.model_needed",
            "The speech model ({}, {}) is needed to transcribe"
        ),
        ("ready.model_download", "Download"),
        ("ready.model_downloading", "Downloading the speech model…"),
        (
            "ready.model_progress",
            "Downloading the speech model… {:.0}%"
        ),
        (
            "ready.press_start",
            "Press Start recording when the meeting begins."
        ),
        ("ready.button_saving", "Saving…"),
        ("ready.button_transcribing", "Transcribing…"),
        ("done.copy", "Copy transcript"),
        ("done.copied", "Copied to clipboard"),
        ("done.no_transcript", "No transcript found"),
        ("done.new", "New recording"),
        ("done.reveal", "Reveal in Finder"),
        ("done.open_folder", "Open folder"),
        ("done.language_again", "Language"),
        ("done.transcribe_again", "Transcribe again"),
        (
            "done.transcribe_again_hint",
            "Transcribe the meeting again with the selected language"
        ),
        (
            "done.transcribe_again_gone",
            "The separate tracks of this meeting are gone, so it cannot be transcribed again"
        ),
        ("done.chapters", "Chapters"),
        (
            "done.chapters_hint",
            "For meetings of three minutes or more"
        ),
        ("done.chapters_added", "{} chapters added"),
        ("done.chapters_none", "No chapters yet"),
        ("done.chapters_writing", "Writing chapters with {}…"),
        ("done.speakers", "Speakers"),
        ("done.rename_meeting", "Meeting name"),
        ("done.your_name", "Your name"),
        ("done.recovery_title", "Unfinished recording found"),
        (
            "done.recovery_body",
            "A recording from {} ({}) was not stopped properly, probably because the app quit. Save it as a meeting?"
        ),
        ("done.recovery_discard", "Discard"),
        ("done.recovery_later", "Later"),
        ("done.recovery_save", "Save"),
        ("done.close_recording_title", "Still recording"),
        (
            "done.close_recording_body",
            "Closing stops the meeting. The audio is saved and transcribed first, then the app quits."
        ),
        ("done.close_recording_stop", "Stop and close"),
        ("done.close_recording_keep", "Keep recording"),
        ("done.close_transcribing_title", "Still transcribing"),
        (
            "done.close_transcribing_body",
            "The audio is already saved. The transcript is not finished yet."
        ),
        ("done.close_transcribing_later", "Close when done"),
        ("done.close_transcribing_keep", "Keep open"),
        ("import.title", "Import an audio file"),
        ("import.subtitle_language", "Language"),
        ("import.subtitle_speakers", "Recognized by their voices"),
        ("import.dialog_title", "Import audio"),
        ("import.confirm", "Import"),
        ("import.cancel", "Cancel"),
        ("import.auto_speakers", "Automatic"),
        ("import.drop_hint", "Import an audio file, or drop one here"),
        (
            "import.exists",
            "A meeting folder with that name already exists"
        ),
        (
            "import.not_openable",
            "This is not a meeting the recorder can open"
        ),
        (
            "banner.audio_permission",
            "Microphone permission was refused — allow it in System Settings › Privacy & Security › Microphone."
        ),
        (
            "banner.audio_tap_permission",
            "System Audio Recording permission was refused — allow it in System Settings › Privacy & Security, then restart the app."
        ),
        (
            "banner.audio_tap_unsupported",
            "This macOS cannot tap the system audio."
        ),
        (
            "banner.audio_install_blackhole",
            "Install BlackHole to record the computer audio instead."
        ),
        (
            "banner.audio_helper_missing",
            "The momr-audio helper was not found — reinstall MOM Recorder."
        ),
        (
            "banner.audio_device_gone",
            "The computer-audio device went away — retrying. Check it in System Settings › Sound."
        ),
        (
            "banner.audio_mic_failed",
            "Microphone capture failed (exit {}) — check System Settings › Privacy & Security › Microphone."
        ),
        (
            "banner.audio_no_ffmpeg",
            "Could not start microphone capture — is ffmpeg installed?"
        ),
        (
            "banner.audio_no_program",
            "Could not start {} — is it installed?"
        ),
        (
            "banner.computer_recording",
            "Computer audio is being recorded."
        ),
        ("strip.tooltip", "Compact strip (⇧⌘M)"),
        ("strip.drag_hint", "Drag to move, ⇧⌘M to expand"),
        ("prefs.title", "Settings"),
        ("prefs.transcription", "Transcription"),
        ("prefs.model", "Speech model"),
        ("prefs.model_present", "On this Mac"),
        ("prefs.model_downloads", "Downloads on first use"),
        (
            "prefs.model_size_gb",
            "About {size} GB, downloads on first use"
        ),
        (
            "prefs.model_size_mb",
            "About {size} MB, downloads on first use"
        ),
        ("prefs.language", "Default language"),
        ("prefs.provider", "Transcription provider"),
        ("prefs.provider_local_note", "Nothing leaves this Mac"),
        (
            "prefs.provider_eleven_note",
            "Sends meeting audio to ElevenLabs when transcribing"
        ),
        (
            "prefs.provider_google_note",
            "Sends meeting audio to Google Cloud when transcribing"
        ),
        (
            "prefs.provider_openrouter_note",
            "Sends meeting audio to the chosen OpenRouter model when transcribing"
        ),
        ("prefs.eleven_key", "ElevenLabs API key"),
        ("prefs.google_key", "Google API key"),
        ("prefs.openrouter_key", "OpenRouter API key"),
        (
            "prefs.key_hint_eleven",
            "Dashboard › profile › API Keys (elevenlabs.io/app/settings/api-keys)"
        ),
        (
            "prefs.key_hint_google",
            "Console › project › Speech-to-Text API › Credentials, restricted to the API"
        ),
        (
            "prefs.key_hint_openrouter",
            "OpenRouter dashboard › Keys (openrouter.ai/settings/keys)"
        ),
        ("prefs.key_saved", "Saved in the Keychain"),
        ("prefs.key_saved_toast", "API key saved"),
        ("prefs.chapters", "Chapters"),
        (
            "prefs.chapters_about",
            "A coding agent with every tool switched off writes the chapter titles."
        ),
        ("prefs.agent", "Agent"),
        ("prefs.agent_none", "None"),
        ("prefs.recording", "Recording"),
        ("prefs.format", "Default audio format"),
        ("prefs.name", "Your name in transcripts"),
        ("prefs.meetings", "Meetings folder"),
        ("prefs.meetings_choose", "Choose…"),
        ("prefs.audio", "Audio"),
        ("prefs.mic", "Microphone"),
        (
            "prefs.mic_inputs",
            "{} input devices, default follows the system"
        ),
        ("prefs.mic_none", "No input device found"),
        ("prefs.computer", "Computer audio"),
        ("prefs.computer_tap", "Records what the Mac plays"),
        ("prefs.computer_blackhole", "Through {}"),
        (
            "prefs.computer_unavailable",
            "Unavailable: install BlackHole"
        ),
        ("prefs.blackhole_how", "How to set up BlackHole"),
        ("prefs.menubar", "Menu Bar"),
        ("prefs.menubar_show", "Show recording status"),
        ("prefs.menubar_restart", "Takes effect on the next launch"),
        ("prefs.ui_language", "Interface language"),
        (
            "prefs.ui_language_hint",
            "Bahasa Indonesia or English. Takes effect on the next launch."
        ),
        (
            "about.comments",
            "Two-track meeting recorder: your microphone and the computer audio, transcribed on this Mac."
        ),
        ("about.transcription_credit", "Transcription"),
        (
            "about.based_on",
            "Based on Meeting Recorder by Jankees van Woezik"
        ),
        ("help.meeting_saved", "Meeting saved"),
        ("help.recovered", "Recovered recording"),
        ("help.finish_first", "Finish the current recording first"),
        ("help.line_deleted", "Line deleted"),
        ("help.saved", "Saved"),
        ("help.copied_clipboard", "Copied to clipboard"),
        ("notify.transcribed", "Meeting transcribed"),
        ("misc.cancel", "Cancel"),
        ("misc.close", "Close"),
        ("misc.save", "Save"),
        ("misc.discard", "Discard"),
        ("misc.later", "Later"),
        ("misc.download", "Download"),
        ("row.edit", "Edit this line"),
        ("row.next_speaker", "Next speaker"),
        ("row.delete", "Delete this line"),
        ("row.play_from", "Play from {}"),
        ("chapter.play_from", "Play from {}"),
        ("chapters.generate", "Generate"),
        ("chapters.redo", "Redo"),
        ("chapters.made_with", "Made with {}"),
        (
            "chapters.let_divide",
            "Let {} divide the meeting into chapters"
        ),
        ("chapters.could_not", "{} could not make chapters"),
        ("import.filter", "Audio and video"),
        ("import.importing", "Importing audio"),
        ("import.no_audio", "this file has no audio ffmpeg can read"),
        ("import.no_convert", "could not convert the audio"),
        ("open.title", "Open a meeting (.meeting-recorder file)"),
        ("open.filter", "Meeting recordings"),
        ("close.cancel_transcription", "Cancel transcription"),
        ("canvas.not_recording", "Not recording"),
        ("canvas.paused", "PAUSED"),
        ("done.meeting_saved", "Meeting saved"),
        ("done.transcript_ready", "Transcript ready"),
        ("done.imported_audio", "Imported audio"),
        ("done.recovered_title", "Recovered recording"),
        ("done.fallback_title", "Meeting"),
        (
            "help.stopped_unexpectedly",
            "the import stopped unexpectedly"
        ),
        (
            "help.transcription_stopped",
            "transcription stopped unexpectedly"
        ),
        ("help.no_meeting_folder", "no meeting folder"),
        ("help.write_failed", "could not write the transcript: {}"),
        (
            "help.could_not_save_transcript",
            "Could not save the transcript"
        ),
        ("help.model_ready", "Speech model ready"),
        (
            "help.model_download_failed",
            "Could not download the model: {}"
        ),
        ("help.download_stopped", "the download stopped"),
        ("help.not_openable", "not a meeting the recorder can open"),
        (
            "help.no_transcript_yet",
            "This meeting has no transcript yet."
        ),
        ("help.cancelled", "Transcription cancelled."),
        ("help.failed", "Transcription failed: {}."),
        ("help.no_audio", "Could not save the audio."),
        ("help.could_not_start", "Could not start recording: {}"),
        ("help.stages_saving", "Saving audio"),
        ("help.stages_loading", "Loading audio"),
        ("help.stages_done", "Done"),
        (
            "help.folder_exists",
            "A meeting folder with that name already exists"
        ),
        ("help.rename_failed", "Could not rename the folder: {}"),
        ("help.every_speaker", "Every speaker needs a different name"),
        ("speaker.row_mic", "Speaker on the microphone"),
        ("speaker.row_computer", "Speaker on the computer audio"),
        ("speaker.row_computer_n", "Speaker {} on the computer audio"),
        ("speaker.import_default", "Speaker {}"),
        ("speaker.empty_fallback", "Speaker {}"),
        ("speaker.you", "You"),
        ("speaker.remote", "Remote"),
        ("speaker.remote_n", "Remote {}"),
        ("stage.loading_model", "Loading model"),
        ("stage.transcribing", "Transcribing"),
        ("stage.loading_audio", "Loading audio"),
        ("stage.warming_up", "Warming up"),
        ("stage.finding_speakers", "Finding speakers"),
        ("download.model", "Downloading model"),
        ("download.speaker", "Downloading the speaker model"),
        ("player.play", "Play"),
        ("player.pause", "Pause"),
        ("format.mono", "Mono"),
        ("format.stereo", "Stereo (mic left, computer right)"),
        ("format.separate", "Separate files"),
        ("format.short_mono", "Mono"),
        ("format.short_stereo", "Stereo"),
        ("format.short_separate", "Separate files"),
        ("lang.auto", "Auto-detect"),
        ("lang.en", "English"),
        ("lang.id", "Indonesian"),
        ("lang.nl", "Dutch"),
        ("lang.de", "German"),
        ("lang.fr", "French"),
        ("lang.es", "Spanish"),
        ("lang.it", "Italian"),
        ("lang.pt", "Portuguese"),
        ("provider.name_local", "On this Mac (whisper)"),
        ("provider.name_eleven", "ElevenLabs"),
        ("provider.name_google", "Google Cloud Speech-to-Text"),
        ("provider.name_openrouter", "OpenRouter"),
        ("misc.keep", "Keep"),
        ("misc.undo", "Undo"),
        (
            "cli.usage",
            "Usage: {} [start | stop | pause | compact | watch | transcribe <mic> <computer> [--language xx]]"
        ),
        (
            "cli.no_command",
            "(no command)  open the recorder, ready to record"
        ),
        (
            "cli.meeting",
            "<meeting>     open a .meeting-recorder file or a meeting folder"
        ),
        (
            "cli.start",
            "start         start recording in the open window (for a keybinding)"
        ),
        (
            "cli.stop",
            "stop          stop the running recording (for a keybinding)"
        ),
        (
            "cli.compact",
            "compact       switch the recording window between full and compact"
        ),
        (
            "cli.pause",
            "pause         pause or resume the running recording"
        ),
        (
            "cli.watch",
            "watch         stream the recorder state as NDJSON, for a menu bar item or any other client"
        ),
        (
            "cli.transcribe",
            "transcribe    transcribe two tracks and print the transcript as Markdown"
        ),
        (
            "cli.ask",
            "ask           run a prompt over stdin through the default agent, without tools"
        ),
        ("cli.not_running", "the recorder is not running"),
        ("cli.diarize", "Usage: {} diarize <audio> [--speakers N]"),
        ("cli.unknown", "unknown command '{}', see --help"),
        ("agent.unset", "No agent set. Add agent = \"claude\" to {}"),
        ("agent.missing", "{} is not installed"),
        (
            "agent.refused_agy",
            "Antigravity only offers a blanket sandbox, not a way to remove its tools, so the recorder will not send your transcript to it"
        ),
        (
            "agent.refused_crush",
            "Crush has no flag to run without tools, so the recorder will not send your transcript to it"
        ),
        (
            "agent.refused_unknown",
            "The recorder does not know how to run {} without tools"
        ),
        (
            "agent.ori_needs",
            "Ori needs Claude Code or Pi installed to work on a transcript"
        ),
        ("agent.ori_harness", "Ori needs Claude Code or Pi installed"),
        (
            "agent.opencode_denied",
            "OpenCode did not come back with every tool denied, so the recorder will not send your transcript to it"
        ),
        (
            "agent.grok_setup",
            "Grok has not been set up yet. Run grok once in a terminal, then try again."
        ),
        (
            "agent.too_long",
            "The transcript is too long to send to the agent"
        ),
        ("agent.no_workdir", "Could not make a working directory: {}"),
        ("agent.no_start", "Could not start {}: {}"),
        ("agent.no_answer", "{} did not answer within {} seconds"),
        ("agent.exited", "{} exited with status {}"),
        ("agent.nothing", "{} returned nothing"),
        (
            "ask.usage",
            "Usage: {} ask \"<prompt>\" < text | ask --agent"
        ),
        ("ask.no_stdin", "could not read the text from stdin"),
        ("help.agent_stopped", "the agent stopped unexpectedly"),
    ];
    lookup(key).unwrap_or("missing string")
}

fn id(key: &str) -> Option<&'static str> {
    let lookup = table![
        ("menu.new", "Rekaman Baru"),
        ("menu.open", "Buka Rapat…"),
        ("menu.import", "Impor Berkas Audio…"),
        ("menu.reveal", "Tampilkan di Finder"),
        ("menu.close_window", "Tutup Jendela"),
        ("menu.undo", "Urungkan"),
        ("menu.redo", "Ulangi"),
        ("menu.cut", "Potong"),
        ("menu.copy", "Salin"),
        ("menu.paste", "Tempel"),
        ("menu.select_all", "Pilih Semua"),
        ("menu.copy_transcript", "Salin Transkrip"),
        ("menu.start", "Mulai Merekam"),
        ("menu.pause_resume", "Jeda / Lanjutkan"),
        ("menu.stop", "Hentikan Rekaman"),
        ("menu.compact", "Strip Ringkas"),
        ("menu.fullscreen", "Masuk Layar Penuh"),
        ("menu.transcribe_again", "Transkripsikan Lagi"),
        ("menu.window", "Jendela"),
        ("menu.help", "Bantuan MOM Recorder"),
        ("menu.file", "Berkas"),
        ("menu.edit", "Sunting"),
        ("menu.recording", "Perekaman"),
        ("menu.view", "Tampilan"),
        ("ready.mic", "Kamu (mikrofon)"),
        ("ready.computer", "Audio komputer"),
        ("ready.hint", "Jatuhkan untuk mengimpor"),
        (
            "ready.import_hint",
            "Impor berkas audio, atau jatuhkan ke sini"
        ),
        ("ready.start", "Mulai merekam"),
        ("ready.stop", "Hentikan perekaman"),
        ("ready.pause", "Jeda"),
        ("ready.resume", "Lanjutkan"),
        (
            "ready.status_idle",
            "Siap. Tekan Mulai merekam saat rapat dimulai."
        ),
        (
            "ready.status_recording",
            "Merekam. Nama, berkas audio, dan bahasa masih bisa diubah."
        ),
        (
            "ready.status_paused",
            "Dijeda. Tidak ada yang direkam sampai dilanjutkan."
        ),
        ("ready.status_stopping", "Menyimpan audio…"),
        (
            "ready.status_transcribing",
            "Mentranskripsikan rapat di komputer ini…"
        ),
        ("ready.language_title", "Bahasa"),
        (
            "ready.language_subtitle",
            "Dipakai untuk transkrip setelah panggilan"
        ),
        ("ready.format_title", "Berkas audio"),
        ("ready.format_subtitle", "Bisa diubah selama panggilan"),
        ("ready.import_button", "Impor berkas audio"),
        (
            "ready.model_needed",
            "Model wicara ({}, {}) diperlukan untuk transkripsi"
        ),
        ("ready.model_download", "Unduh"),
        ("ready.model_downloading", "Mengunduh model wicara…"),
        ("ready.model_progress", "Mengunduh model wicara… {:.0}%"),
        (
            "ready.press_start",
            "Tekan Mulai merekam saat rapat dimulai."
        ),
        ("ready.button_saving", "Menyimpan…"),
        ("ready.button_transcribing", "Mentranskripsikan…"),
        ("done.copy", "Salin transkrip"),
        ("done.copied", "Disalin ke papan klip"),
        ("done.no_transcript", "Transkrip tidak ditemukan"),
        ("done.new", "Rekaman baru"),
        ("done.reveal", "Tampilkan di Finder"),
        ("done.open_folder", "Buka folder"),
        ("done.language_again", "Bahasa"),
        ("done.transcribe_again", "Transkripsikan lagi"),
        (
            "done.transcribe_again_hint",
            "Transkripsikan lagi rapat ini dengan bahasa yang dipilih"
        ),
        (
            "done.transcribe_again_gone",
            "Trek terpisah rapat ini sudah hilang, jadi tidak bisa ditranskripsikan lagi"
        ),
        ("done.chapters", "Bab"),
        ("done.chapters_hint", "Untuk rapat tiga menit atau lebih"),
        ("done.chapters_added", "{} bab ditambahkan"),
        ("done.chapters_none", "Belum ada bab"),
        ("done.chapters_writing", "Menulis bab dengan {}…"),
        ("done.speakers", "Pembicara"),
        ("done.rename_meeting", "Nama rapat"),
        ("done.your_name", "Namamu"),
        (
            "done.recovery_title",
            "Rekaman yang belum selesai ditemukan"
        ),
        (
            "done.recovery_body",
            "Rekaman dari {} ({}) tidak dihentikan dengan benar, mungkin karena aplikasi keluar. Simpan sebagai rapat?"
        ),
        ("done.recovery_discard", "Buang"),
        ("done.recovery_later", "Nanti"),
        ("done.recovery_save", "Simpan"),
        ("done.close_recording_title", "Masih merekam"),
        (
            "done.close_recording_body",
            "Menutup menghentikan rapat. Audio disimpan dan ditranskripsikan dulu, lalu aplikasi keluar."
        ),
        ("done.close_recording_stop", "Hentikan dan tutup"),
        ("done.close_recording_keep", "Lanjutkan merekam"),
        ("done.close_transcribing_title", "Masih mentranskripsikan"),
        (
            "done.close_transcribing_body",
            "Audio sudah tersimpan. Transkripnya belum selesai."
        ),
        ("done.close_transcribing_later", "Tutup jika sudah selesai"),
        ("done.close_transcribing_keep", "Tetap buka"),
        ("import.title", "Impor berkas audio"),
        ("import.subtitle_language", "Bahasa"),
        ("import.subtitle_speakers", "Dikenali dari suaranya"),
        ("import.dialog_title", "Impor audio"),
        ("import.confirm", "Impor"),
        ("import.cancel", "Batal"),
        ("import.auto_speakers", "Otomatis"),
        (
            "import.drop_hint",
            "Impor berkas audio, atau jatuhkan ke sini"
        ),
        ("import.exists", "Folder rapat dengan nama itu sudah ada"),
        (
            "import.not_openable",
            "Ini bukan rapat yang bisa dibuka perekam"
        ),
        (
            "banner.audio_permission",
            "Izin mikrofon ditolak — izinkan di Pengaturan Sistem › Privasi & Keamanan › Mikrofon."
        ),
        (
            "banner.audio_tap_permission",
            "Izin Perekaman Audio Sistem ditolak — izinkan di Pengaturan Sistem › Privasi & Keamanan, lalu buka ulang aplikasi."
        ),
        (
            "banner.audio_tap_unsupported",
            "macOS ini tidak bisa menyadap audio sistem."
        ),
        (
            "banner.audio_install_blackhole",
            "Pasang BlackHole agar audio komputer ikut terekam."
        ),
        (
            "banner.audio_helper_missing",
            "Helper momr-audio tidak ditemukan — pasang ulang MOM Recorder."
        ),
        (
            "banner.audio_device_gone",
            "Perangkat audio komputer hilang — mencoba lagi. Periksa di Pengaturan Sistem › Suara."
        ),
        (
            "banner.audio_mic_failed",
            "Perekaman mikrofon gagal (exit {}) — periksa Pengaturan Sistem › Privasi & Keamanan › Mikrofon."
        ),
        (
            "banner.audio_no_ffmpeg",
            "Tidak bisa mulai perekaman mikrofon — apakah ffmpeg terpasang?"
        ),
        (
            "banner.audio_no_program",
            "Tidak bisa menjalankan {} — apakah terpasang?"
        ),
        (
            "banner.computer_recording",
            "Audio komputer sedang direkam."
        ),
        ("strip.tooltip", "Strip ringkas (⇧⌘M)"),
        (
            "strip.drag_hint",
            "Seret untuk memindah, ⇧⌘M untuk melebarkan"
        ),
        ("prefs.title", "Pengaturan"),
        ("prefs.transcription", "Transkripsi"),
        ("prefs.model", "Model wicara"),
        ("prefs.model_present", "Di Mac ini"),
        ("prefs.model_downloads", "Diunduh saat pertama dipakai"),
        (
            "prefs.model_size_gb",
            "Sekitar {size} GB, diunduh saat pertama dipakai"
        ),
        (
            "prefs.model_size_mb",
            "Sekitar {size} MB, diunduh saat pertama dipakai"
        ),
        ("prefs.language", "Bahasa default"),
        ("prefs.provider", "Penyedia transkripsi"),
        (
            "prefs.provider_local_note",
            "Tidak ada yang keluar dari Mac ini"
        ),
        (
            "prefs.provider_eleven_note",
            "Mengirim audio rapat ke ElevenLabs saat transkripsi"
        ),
        (
            "prefs.provider_google_note",
            "Mengirim audio rapat ke Google Cloud saat transkripsi"
        ),
        (
            "prefs.provider_openrouter_note",
            "Mengirim audio rapat ke model OpenRouter pilihan saat transkripsi"
        ),
        ("prefs.eleven_key", "Kunci API ElevenLabs"),
        ("prefs.google_key", "Kunci API Google"),
        ("prefs.openrouter_key", "Kunci API OpenRouter"),
        (
            "prefs.key_hint_eleven",
            "Dasbor › profil › API Keys (elevenlabs.io/app/settings/api-keys)"
        ),
        (
            "prefs.key_hint_google",
            "Konsol › proyek › Speech-to-Text API › Credentials, dibatasi untuk API itu"
        ),
        (
            "prefs.key_hint_openrouter",
            "Dasbor OpenRouter › Keys (openrouter.ai/settings/keys)"
        ),
        ("prefs.key_saved", "Tersimpan di Keychain"),
        ("prefs.key_saved_toast", "Kunci API tersimpan"),
        ("prefs.chapters", "Bab"),
        (
            "prefs.chapters_about",
            "Agen pengode dengan semua perkakas dimatikan yang menulis judul bab."
        ),
        ("prefs.agent", "Agen"),
        ("prefs.agent_none", "Tidak ada"),
        ("prefs.recording", "Perekaman"),
        ("prefs.format", "Format audio default"),
        ("prefs.name", "Namamu di transkrip"),
        ("prefs.meetings", "Folder rapat"),
        ("prefs.meetings_choose", "Pilih…"),
        ("prefs.audio", "Audio"),
        ("prefs.mic", "Mikrofon"),
        (
            "prefs.mic_inputs",
            "{} perangkat input, default mengikuti sistem"
        ),
        ("prefs.mic_none", "Tidak ada perangkat input"),
        ("prefs.computer", "Audio komputer"),
        ("prefs.computer_tap", "Merekam yang dimainkan Mac"),
        ("prefs.computer_blackhole", "Melalui {}"),
        (
            "prefs.computer_unavailable",
            "Tidak tersedia: pasang BlackHole"
        ),
        ("prefs.blackhole_how", "Cara memasang BlackHole"),
        ("prefs.menubar", "Bilah Menu"),
        ("prefs.menubar_show", "Tampilkan status perekaman"),
        ("prefs.menubar_restart", "Berlaku saat dibuka berikutnya"),
        ("prefs.ui_language", "Bahasa antarmuka"),
        (
            "prefs.ui_language_hint",
            "Bahasa Indonesia atau English. Berlaku saat dibuka berikutnya."
        ),
        (
            "about.comments",
            "Perekam rapat dua trek: mikrofonmu dan audio komputermu, ditranskripsikan di Mac ini."
        ),
        ("about.transcription_credit", "Transkripsi"),
        (
            "about.based_on",
            "Berdasarkan Meeting Recorder oleh Jankees van Woezik"
        ),
        ("help.meeting_saved", "Rapat tersimpan"),
        ("help.recovered", "Rekaman pulihan"),
        (
            "help.finish_first",
            "Selesaikan dulu perekaman yang berjalan"
        ),
        ("help.line_deleted", "Baris dihapus"),
        ("help.saved", "Tersimpan"),
        ("help.copied_clipboard", "Disalin ke papan klip"),
        ("notify.transcribed", "Rapat ditranskripsikan"),
        ("misc.cancel", "Batal"),
        ("misc.close", "Tutup"),
        ("misc.save", "Simpan"),
        ("misc.discard", "Buang"),
        ("misc.later", "Nanti"),
        ("misc.download", "Unduh"),
        ("misc.keep", "Tetap"),
        ("misc.undo", "Urungkan"),
        (
            "cli.usage",
            "Pakai: {} [start | stop | pause | compact | watch | transcribe <mic> <computer> [--language xx]]"
        ),
        (
            "cli.no_command",
            "(tanpa perintah)  buka perekam, siap merekam"
        ),
        (
            "cli.meeting",
            "<rapat>       buka berkas .meeting-recorder atau folder rapat"
        ),
        (
            "cli.start",
            "start         mulai merekam di jendela yang terbuka (untuk pintasan)"
        ),
        (
            "cli.stop",
            "stop          hentikan perekaman yang berjalan (untuk pintasan)"
        ),
        (
            "cli.compact",
            "compact       alihkan jendela perekaman antara penuh dan ringkas"
        ),
        (
            "cli.pause",
            "pause         jeda atau lanjutkan rekaman yang berjalan"
        ),
        (
            "cli.watch",
            "watch         alirkan status perekam sebagai NDJSON, untuk item bilah menu atau klien lain"
        ),
        (
            "cli.transcribe",
            "transcribe    transkripsikan dua trek dan cetak transkrip sebagai Markdown"
        ),
        (
            "cli.ask",
            "ask           jalankan prompt lewat stdin melalui agen default, tanpa perkakas"
        ),
        ("cli.not_running", "perekam tidak berjalan"),
        ("cli.diarize", "Pakai: {} diarize <audio> [--speakers N]"),
        ("cli.unknown", "perintah '{}' tidak dikenal, lihat --help"),
        (
            "agent.unset",
            "Agen belum diatur. Tambahkan agent = \"claude\" ke {}"
        ),
        ("agent.missing", "{} belum terpasang"),
        (
            "agent.refused_agy",
            "Antigravity hanya menawarkan sandbox umum, bukan cara mematikan perkakasnya, jadi perekam tidak akan mengirim transkripmu ke sana"
        ),
        (
            "agent.refused_crush",
            "Crush tidak punya flag untuk berjalan tanpa perkakas, jadi perekam tidak akan mengirim transkripmu ke sana"
        ),
        (
            "agent.refused_unknown",
            "Perekam tidak tahu cara menjalankan {} tanpa perkakas"
        ),
        (
            "agent.ori_needs",
            "Ori butuh Claude Code atau Pi terpasang agar bisa mengolah transkrip"
        ),
        (
            "agent.ori_harness",
            "Ori butuh Claude Code atau Pi terpasang"
        ),
        (
            "agent.opencode_denied",
            "OpenCode tidak kembali dengan semua perkakas dimatikan, jadi perekam tidak akan mengirim transkripmu ke sana"
        ),
        (
            "agent.grok_setup",
            "Grok belum disiapkan. Jalankan grok sekali di terminal, lalu coba lagi."
        ),
        (
            "agent.too_long",
            "Transkrip terlalu panjang untuk dikirim ke agen"
        ),
        ("agent.no_workdir", "Tidak bisa membuat direktori kerja: {}"),
        ("agent.no_start", "Tidak bisa menjalankan {}: {}"),
        ("agent.no_answer", "{} tidak menjawab dalam {} detik"),
        ("agent.exited", "{} keluar dengan status {}"),
        ("agent.nothing", "{} tidak mengembalikan apa-apa"),
        (
            "ask.usage",
            "Pakai: {} ask \"<prompt>\" < text | ask --agent"
        ),
        ("ask.no_stdin", "tidak bisa membaca teks dari stdin"),
        ("help.agent_stopped", "agen berhenti tiba-tiba"),
        ("row.edit", "Sunting baris ini"),
        ("row.next_speaker", "Pembicara berikutnya"),
        ("row.delete", "Hapus baris ini"),
        ("row.play_from", "Putar dari {}"),
        ("chapter.play_from", "Putar dari {}"),
        ("chapters.generate", "Buat"),
        ("chapters.redo", "Ulangi"),
        ("chapters.made_with", "Dibuat dengan {}"),
        ("chapters.let_divide", "Minta {} membagi rapat menjadi bab"),
        ("chapters.could_not", "{} tidak bisa membuat bab"),
        ("import.filter", "Audio dan video"),
        ("import.importing", "Mengimpor audio"),
        (
            "import.no_audio",
            "berkas ini tidak punya audio yang bisa dibaca ffmpeg"
        ),
        ("import.no_convert", "tidak bisa mengonversi audio"),
        ("open.title", "Buka rapat (berkas .meeting-recorder)"),
        ("open.filter", "Rekaman rapat"),
        ("close.cancel_transcription", "Batalkan transkripsi"),
        ("canvas.not_recording", "Tidak merekam"),
        ("canvas.paused", "DIJEDA"),
        ("done.meeting_saved", "Rapat tersimpan"),
        ("done.transcript_ready", "Transkrip siap"),
        ("done.imported_audio", "Audio impor"),
        ("done.recovered_title", "Rekaman pulihan"),
        ("done.fallback_title", "Rapat"),
        ("help.stopped_unexpectedly", "impor berhenti tiba-tiba"),
        (
            "help.transcription_stopped",
            "transkripsi berhenti tiba-tiba"
        ),
        ("help.no_meeting_folder", "tidak ada folder rapat"),
        ("help.write_failed", "tidak bisa menulis transkrip: {}"),
        (
            "help.could_not_save_transcript",
            "Tidak bisa menyimpan transkrip"
        ),
        ("help.model_ready", "Model wicara siap"),
        (
            "help.model_download_failed",
            "Tidak bisa mengunduh model: {}"
        ),
        ("help.download_stopped", "unduhan berhenti"),
        ("help.not_openable", "bukan rapat yang bisa dibuka perekam"),
        ("help.no_transcript_yet", "Rapat ini belum punya transkrip."),
        ("help.cancelled", "Transkripsi dibatalkan."),
        ("help.failed", "Transkripsi gagal: {}."),
        ("help.no_audio", "Tidak bisa menyimpan audio."),
        ("help.could_not_start", "Tidak bisa mulai merekam: {}"),
        ("help.stages_saving", "Menyimpan audio"),
        ("help.stages_loading", "Memuat audio"),
        ("help.stages_done", "Selesai"),
        (
            "help.folder_exists",
            "Folder rapat dengan nama itu sudah ada"
        ),
        ("help.rename_failed", "Tidak bisa mengganti nama folder: {}"),
        ("help.every_speaker", "Setiap pembicara butuh nama berbeda"),
        ("speaker.row_mic", "Pembicara di mikrofon"),
        ("speaker.row_computer", "Pembicara di audio komputer"),
        ("speaker.row_computer_n", "Pembicara {} di audio komputer"),
        ("speaker.import_default", "Pembicara {}"),
        ("speaker.empty_fallback", "Pembicara {}"),
        ("speaker.you", "Kamu"),
        ("speaker.remote", "Remote"),
        ("speaker.remote_n", "Remote {}"),
        ("stage.loading_model", "Memuat model"),
        ("stage.transcribing", "Mentranskripsikan"),
        ("stage.loading_audio", "Memuat audio"),
        ("stage.warming_up", "Pemanasan"),
        ("stage.finding_speakers", "Mencari pembicara"),
        ("download.model", "Mengunduh model"),
        ("download.speaker", "Mengunduh model pembicara"),
        ("player.play", "Putar"),
        ("player.pause", "Jeda"),
        ("format.mono", "Mono"),
        ("format.stereo", "Stereo (mic kiri, komputer kanan)"),
        ("format.separate", "Berkas terpisah"),
        ("format.short_mono", "Mono"),
        ("format.short_stereo", "Stereo"),
        ("format.short_separate", "Berkas terpisah"),
        ("lang.auto", "Otomatis"),
        ("lang.en", "Inggris"),
        ("lang.id", "Indonesia"),
        ("lang.nl", "Belanda"),
        ("lang.de", "Jerman"),
        ("lang.fr", "Prancis"),
        ("lang.es", "Spanyol"),
        ("lang.it", "Italia"),
        ("lang.pt", "Portugis"),
        ("provider.name_local", "Di Mac ini (whisper)"),
        ("provider.name_eleven", "ElevenLabs"),
        ("provider.name_google", "Google Cloud Speech-to-Text"),
        ("provider.name_openrouter", "OpenRouter"),
    ];
    lookup(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All lookup keys, the single source of truth for the completeness test.
    const KEYS: &[&str] = &[
        "menu.new",
        "menu.open",
        "menu.import",
        "menu.reveal",
        "menu.close_window",
        "menu.undo",
        "menu.redo",
        "menu.cut",
        "menu.copy",
        "menu.paste",
        "menu.select_all",
        "menu.copy_transcript",
        "menu.start",
        "menu.pause_resume",
        "menu.stop",
        "menu.compact",
        "menu.fullscreen",
        "menu.transcribe_again",
        "menu.window",
        "menu.help",
        "menu.file",
        "menu.edit",
        "menu.recording",
        "menu.view",
        "ready.mic",
        "ready.computer",
        "ready.hint",
        "ready.import_hint",
        "ready.start",
        "ready.stop",
        "ready.pause",
        "ready.resume",
        "ready.status_idle",
        "ready.status_recording",
        "ready.status_paused",
        "ready.status_stopping",
        "ready.status_transcribing",
        "ready.language_title",
        "ready.language_subtitle",
        "ready.format_title",
        "ready.format_subtitle",
        "ready.import_button",
        "ready.model_needed",
        "ready.model_download",
        "ready.model_downloading",
        "ready.model_progress",
        "ready.press_start",
        "ready.button_saving",
        "ready.button_transcribing",
        "done.copy",
        "done.copied",
        "done.no_transcript",
        "done.new",
        "done.reveal",
        "done.open_folder",
        "done.language_again",
        "done.transcribe_again",
        "done.transcribe_again_hint",
        "done.transcribe_again_gone",
        "done.chapters",
        "done.chapters_hint",
        "done.chapters_added",
        "done.chapters_none",
        "done.chapters_writing",
        "done.speakers",
        "done.rename_meeting",
        "done.your_name",
        "done.recovery_title",
        "done.recovery_body",
        "done.recovery_discard",
        "done.recovery_later",
        "done.recovery_save",
        "done.close_recording_title",
        "done.close_recording_body",
        "done.close_recording_stop",
        "done.close_recording_keep",
        "done.close_transcribing_title",
        "done.close_transcribing_body",
        "done.close_transcribing_later",
        "done.close_transcribing_keep",
        "import.title",
        "import.subtitle_language",
        "import.subtitle_speakers",
        "import.dialog_title",
        "import.confirm",
        "import.cancel",
        "import.auto_speakers",
        "import.drop_hint",
        "import.exists",
        "import.not_openable",
        "banner.audio_permission",
        "banner.audio_tap_permission",
        "banner.audio_tap_unsupported",
        "banner.audio_install_blackhole",
        "banner.audio_helper_missing",
        "banner.audio_device_gone",
        "banner.audio_mic_failed",
        "banner.audio_no_ffmpeg",
        "banner.audio_no_program",
        "banner.computer_recording",
        "strip.tooltip",
        "strip.drag_hint",
        "prefs.title",
        "prefs.transcription",
        "prefs.model",
        "prefs.model_present",
        "prefs.model_downloads",
        "prefs.model_size_gb",
        "prefs.model_size_mb",
        "prefs.language",
        "prefs.provider",
        "prefs.provider_local_note",
        "prefs.provider_eleven_note",
        "prefs.provider_google_note",
        "prefs.provider_openrouter_note",
        "prefs.eleven_key",
        "prefs.google_key",
        "prefs.openrouter_key",
        "prefs.key_hint_eleven",
        "prefs.key_hint_google",
        "prefs.key_hint_openrouter",
        "prefs.key_saved",
        "prefs.key_saved_toast",
        "prefs.chapters",
        "prefs.chapters_about",
        "prefs.agent",
        "prefs.agent_none",
        "row.edit",
        "row.next_speaker",
        "row.delete",
        "row.play_from",
        "chapter.play_from",
        "chapters.generate",
        "chapters.redo",
        "chapters.made_with",
        "chapters.let_divide",
        "chapters.could_not",
        "import.filter",
        "import.importing",
        "import.no_audio",
        "import.no_convert",
        "import.auto_speakers",
        "open.title",
        "open.filter",
        "close.cancel_transcription",
        "canvas.not_recording",
        "canvas.paused",
        "done.meeting_saved",
        "done.transcript_ready",
        "done.imported_audio",
        "done.recovered_title",
        "done.fallback_title",
        "help.stopped_unexpectedly",
        "help.transcription_stopped",
        "help.no_meeting_folder",
        "help.write_failed",
        "help.could_not_save_transcript",
        "help.model_ready",
        "help.model_download_failed",
        "help.download_stopped",
        "help.not_openable",
        "help.no_transcript_yet",
        "help.cancelled",
        "help.failed",
        "help.no_audio",
        "help.could_not_start",
        "help.stages_saving",
        "help.stages_loading",
        "help.stages_done",
        "help.folder_exists",
        "help.rename_failed",
        "help.every_speaker",
        "speaker.row_mic",
        "speaker.row_computer",
        "speaker.row_computer_n",
        "speaker.import_default",
        "speaker.empty_fallback",
        "speaker.you",
        "speaker.remote",
        "speaker.remote_n",
        "stage.loading_model",
        "stage.transcribing",
        "stage.loading_audio",
        "stage.warming_up",
        "stage.finding_speakers",
        "download.model",
        "download.speaker",
        "player.play",
        "player.pause",
        "format.mono",
        "format.stereo",
        "format.separate",
        "format.short_mono",
        "format.short_stereo",
        "format.short_separate",
        "lang.auto",
        "lang.en",
        "lang.id",
        "lang.nl",
        "lang.de",
        "lang.fr",
        "lang.es",
        "lang.it",
        "lang.pt",
        "provider.name_local",
        "provider.name_eleven",
        "provider.name_google",
        "provider.name_openrouter",
        "prefs.mic",
        "prefs.mic_inputs",
        "prefs.mic_none",
        "prefs.computer",
        "prefs.computer_tap",
        "prefs.computer_blackhole",
        "prefs.computer_unavailable",
        "prefs.blackhole_how",
        "prefs.menubar",
        "prefs.menubar_show",
        "prefs.menubar_restart",
        "prefs.ui_language",
        "prefs.ui_language_hint",
        "about.comments",
        "about.transcription_credit",
        "about.based_on",
        "help.meeting_saved",
        "help.recovered",
        "help.finish_first",
        "help.line_deleted",
        "help.saved",
        "help.copied_clipboard",
        "notify.transcribed",
        "misc.cancel",
        "misc.close",
        "misc.save",
        "misc.discard",
        "misc.later",
        "misc.download",
        "misc.keep",
        "misc.undo",
        "cli.usage",
        "cli.no_command",
        "cli.meeting",
        "cli.start",
        "cli.stop",
        "cli.compact",
        "cli.pause",
        "cli.watch",
        "cli.transcribe",
        "cli.ask",
        "cli.not_running",
        "cli.diarize",
        "cli.unknown",
        "agent.unset",
        "agent.missing",
        "agent.refused_agy",
        "agent.refused_crush",
        "agent.refused_unknown",
        "agent.ori_needs",
        "agent.ori_harness",
        "agent.opencode_denied",
        "agent.grok_setup",
        "agent.too_long",
        "agent.no_workdir",
        "agent.no_start",
        "agent.no_answer",
        "agent.exited",
        "agent.nothing",
        "ask.usage",
        "ask.no_stdin",
        "help.agent_stopped",
    ];

    #[test]
    fn both_tables_cover_every_key() {
        let mut missing = Vec::new();
        for key in KEYS {
            // English needs a real text, not the key or the missing marker.
            let en_text = t_in(Lang::English, key);
            if en_text == *key || en_text == "missing string" {
                missing.push(("en", key));
            }
            // Indonesian needs its own entry, not a silent fall back to English.
            if id(key).is_none() {
                missing.push(("id", key));
            }
        }
        assert!(missing.is_empty(), "untranslated keys: {missing:?}");
    }

    #[test]
    fn unknown_key_falls_back_to_a_marker() {
        assert_eq!(t("no.such.key"), "missing string");
    }
}
