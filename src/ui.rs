//! The recorder window. It has one page per phase: recording (which can shrink
//! to a compact strip: the clock in the title bar over one two-lane wave),
//! transcribing (the animation, edge to edge) and done (the transcript and
//! what to do with it, in a split view whose sidebar folds away in a narrow
//! window). The window keeps the size the user gave it, remembered between
//! launches; pages adapt to it, and only the strip changes it. Around it:
//! the native menu bar and the actions behind it, the Settings dialog in
//! pages, the Timer dialog, the About window, and the menu bar item launched
//! next to the app. Every other module is a leaf this one calls.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use adw::prelude::*;
use gtk::{gio, glib};

use crate::animation::TranscribeAnimation;
use crate::player::Player;
use crate::{APP_ID, APP_NAME};
use momr_core::agent::{self, Agent};
use momr_core::audio::{Device, HISTORY, Source, to_meter};
use momr_core::chapters;
use momr_core::export::{self, Format, export_audio, export_tracks};
use momr_core::finish::{self, RecordingNote, raw_duration};
use momr_core::ipc::{SharedStatus, Status};
use momr_core::locales::{Lang, t, tf};
use momr_core::meeting::Chapter;
use momr_core::meeting::{self, Manifest, parse_segment};
use momr_core::provider::{Cloud, Provider};
use momr_core::settings;
use momr_core::transcribe::{self, Abort, CANCELLED, Event, LANGUAGE_CODES, language_label};

const MIC_COLOR: (f64, f64, f64) = (0.0, 0.478, 1.0);
const SYSTEM_COLOR: (f64, f64, f64) = (1.0, 0.584, 0.0);
/// The first launch's window; later ones open at the remembered size.
const DEFAULT_SIZE: (i32, i32) = (960, 680);
/// Small enough for a laptop next to a call, large enough for the done page.
const MIN_SIZE: (i32, i32) = (560, 520);
/// The compact strip: the header bar and one wave. Also its size request,
/// since libadwaita holds every window at 360×200 unless told otherwise.
const COMPACT_SIZE: (i32, i32) = (380, 96);
/// A recording at least this long with a computer track of exact zeros gets
/// the permission hint; a shorter one is likely a test.
const SILENT_HINT_SECS: i64 = 30;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Idle,
    Recording,
    /// Writing the audio files.
    Stopping,
    Transcribing,
    Done,
}

impl State {
    fn key(self) -> &'static str {
        match self {
            State::Idle => "idle",
            State::Recording => "recording",
            State::Stopping => "stopping",
            State::Transcribing => "transcribing",
            State::Done => "done",
        }
    }
}

/// Where a transcription reads its two tracks from.
enum Tracks {
    /// The staging files of a recording that just stopped.
    Raw(PathBuf),
    /// The `.tracks` directory kept in a meeting folder.
    Kept(PathBuf),
    /// One imported file, with the number of speakers asked for.
    Single(PathBuf, Option<usize>),
}

/// The imported source audio, kept for transcribing again.
fn source_track(meeting_dir: &std::path::Path) -> PathBuf {
    meeting_dir.join(export::TRACKS_DIR).join("source.ogg")
}

/// Choices in the import dialog: automatic, then a fixed number.
const SPEAKER_NUMBERS: [&str; 6] = ["1", "2", "3", "4", "5", "6"];

/// The menu structure alone, so tests can walk every item without an app.
fn menu_model() -> gio::Menu {
    use gio::Menu;
    let item = |label: &str, action: &str| gio::MenuItem::new(Some(label), Some(action));

    let file = Menu::new();
    file.append_item(&item(t("menu.new"), "win.new-recording"));
    file.append_item(&item(t("menu.open"), "win.open-meeting"));
    file.append_item(&item(t("menu.import"), "win.import"));
    let finder = Menu::new();
    finder.append_item(&item(t("menu.reveal"), "win.reveal"));
    file.append_section(None, &finder);
    let close = Menu::new();
    close.append_item(&item(t("menu.close_window"), "window.close"));
    file.append_section(None, &close);

    let edit = Menu::new();
    for (label, action) in [(t("menu.undo"), "text.undo"), (t("menu.redo"), "text.redo")] {
        edit.append_item(&item(label, action));
    }
    let clipboard = Menu::new();
    for (label, action) in [
        (t("menu.cut"), "clipboard.cut"),
        (t("menu.copy"), "clipboard.copy"),
        (t("menu.paste"), "clipboard.paste"),
        (t("menu.select_all"), "selection.select-all"),
    ] {
        clipboard.append_item(&item(label, action));
    }
    edit.append_section(None, &clipboard);
    let transcript = Menu::new();
    transcript.append_item(&item(t("menu.copy_transcript"), "win.copy-transcript"));
    edit.append_section(None, &transcript);

    let recording = Menu::new();
    recording.append_item(&item(t("menu.start"), "win.start"));
    recording.append_item(&item(t("menu.pause_resume"), "win.pause"));
    recording.append_item(&item(t("menu.stop"), "win.stop"));
    let timed = Menu::new();
    timed.append_item(&item(t("menu.timer"), "win.timer"));
    recording.append_section(None, &timed);

    let view = Menu::new();
    view.append_item(&item(t("menu.compact"), "win.compact"));
    view.append_item(&item(t("menu.fullscreen"), "win.fullscreen"));
    let again = Menu::new();
    for code in LANGUAGE_CODES {
        let name = language_label(code);
        let entry = gio::MenuItem::new(Some(name), None);
        entry.set_action_and_target_value(
            Some("win.transcribe-again"),
            Some(&glib::Variant::from(code)),
        );
        again.append_item(&entry);
    }
    view.append_submenu(Some(t("menu.transcribe_again")), &again);

    // macOS manages Minimize, Zoom and the window list itself.
    let window = Menu::new();
    let help = Menu::new();
    help.append_item(&item(t("menu.help"), "app.help"));

    let bar = Menu::new();
    bar.append_submenu(Some(t("menu.file")), &file);
    bar.append_submenu(Some(t("menu.edit")), &edit);
    bar.append_submenu(Some(t("menu.recording")), &recording);
    bar.append_submenu(Some(t("menu.view")), &view);
    bar.append_submenu(Some(t("menu.window")), &window);
    bar.append_submenu(Some(t("menu.help")), &help);
    bar
}

/// The native menu bar: GTK's quartz backend turns this `GMenuModel` into the
/// NSMenu bar, and adds the standard app menu when `app.about`,
/// `app.preferences` and `app.quit` exist. The app's own items are `GAction`s
/// from `install_actions`, so enabled state is shared with the buttons; the
/// Edit items are GTK's built-in `text.*`, `clipboard.*` and `selection.*`
/// actions, and Close Window is `window.close`.
fn install_menubar(app: &adw::Application) {
    app.set_menubar(Some(&menu_model()));
}

/// Runs the app. `open` is a `.meeting-recorder` file or a meeting folder to show
/// instead of starting a new recording.
pub fn run(open: Option<&str>) -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();
    let menubar: Rc<RefCell<Option<std::process::Child>>> = Rc::new(RefCell::new(None));
    let menubar_startup = menubar.clone();
    app.connect_startup(move |app| {
        spawn_menubar(&menubar_startup);
        if let Some(settings) = gtk::Settings::default() {
            // Window buttons on the left, drawn as traffic lights by macos.css.
            settings.set_gtk_decoration_layout(Some("close,minimize,maximize:"));
            // GDK reports the system font at 12 pt; macOS body text is 13 pt.
            // Keep the family it reported, bump the size.
            let name = settings
                .gtk_font_name()
                .map(|n| n.to_string())
                .unwrap_or_default();
            if !name.is_empty() {
                let family = name.rsplit_once(' ').map_or(name.as_str(), |(f, _)| f);
                settings.set_gtk_font_name(Some(&format!("{family} 13")));
            }
        }
        crate::theme::follow(|| {
            for window in gtk::Window::list_toplevels() {
                redraw(&window);
            }
        });
        crate::theme::apply_appearance(
            settings::load_appearance(),
            crate::theme::macos_prefers_dark,
        );
        install_menubar(app);
        app.set_accels_for_action("win.new-recording", &["<Primary>n"]);
        app.set_accels_for_action("win.open-meeting", &["<Primary>o"]);
        app.set_accels_for_action("win.import", &["<Primary><Shift>i"]);
        app.set_accels_for_action("win.reveal", &["<Primary><Alt>r"]);
        app.set_accels_for_action("window.close", &["<Primary>w"]);
        app.set_accels_for_action("win.start", &["<Primary>r"]);
        app.set_accels_for_action("win.pause", &["<Primary><Shift>r"]);
        app.set_accels_for_action("win.stop", &["<Primary>period"]);
        app.set_accels_for_action("win.timer", &["<Primary>t"]);
        app.set_accels_for_action("win.compact", &["<Primary><Shift>m"]);
        app.set_accels_for_action("win.copy-transcript", &["<Primary><Shift>c"]);
        app.set_accels_for_action("win.fullscreen", &["<Primary><Control>f"]);
        app.set_accels_for_action("app.preferences", &["<Primary>comma"]);
        app.set_accels_for_action("app.quit", &["<Primary>q"]);
    });
    let recorder: Rc<RefCell<Option<Rc<Recorder>>>> = Rc::default();
    let get = {
        let recorder = recorder.clone();
        move |app: &adw::Application| {
            let existing = recorder.borrow().clone();
            existing.unwrap_or_else(|| {
                let created = Recorder::new(app);
                *recorder.borrow_mut() = Some(created.clone());
                created
            })
        }
    };
    let get_for_open = get.clone();
    app.connect_open(move |app, files, _| {
        let recorder = get_for_open(app);
        if let Some(path) = files.first().and_then(|f| f.path()) {
            recorder.open_meeting(&path);
        }
        recorder.window.present();
    });
    app.connect_activate(move |app| {
        let recorder = get(app);
        // Opened again while it was finishing in the background: stay open.
        recorder.quit_when_done.set(false);
        if recorder.state.get() == State::Idle {
            let recorder = recorder.clone();
            glib::idle_add_local_once(move || recorder.offer_recovery());
        }
        recorder.window.present();
    });
    app.connect_shutdown(move |_| {
        if let Some(mut child) = menubar.borrow_mut().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    });
    match open {
        Some(path) => app.run_with_args(&[APP_NAME, path]),
        None => app.run_with_args::<&str>(&[]),
    }
}

/// The menu bar item, launched once next to the app. `menubar = "false"` in
/// config.toml disables it; a missing binary is skipped (a terminal build
/// without the helpers has none). It gets the socket path and the interface
/// language from here, so the two processes cannot disagree on either.
fn spawn_menubar(slot: &Rc<RefCell<Option<std::process::Child>>>) {
    if !momr_core::models::menubar_enabled() {
        return;
    }
    let Some(binary) = momr_core::helper::menubar_path() else {
        return;
    };
    match std::process::Command::new(&binary)
        .env("MOMR_SOCKET", momr_core::ipc::socket_path())
        .env("MOMR_LANG", momr_core::locales::current().code())
        // The item quits when this pid ends, so a crash or a kill does not
        // leave it behind; a clean quit still stops it below.
        .env("MOMR_PARENT_PID", std::process::id().to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit())
        .spawn()
    {
        Ok(child) => *slot.borrow_mut() = Some(child),
        Err(e) => eprintln!("{APP_NAME}: could not start {}: {e}", binary.display()),
    }
}

struct Recorder {
    window: adw::ApplicationWindow,
    view: adw::ToolbarView,
    header: adw::HeaderBar,
    toasts: adw::ToastOverlay,
    layout: gtk::Stack,
    compact_action: gio::SimpleAction,
    compact_button: gtk::Button,
    gear: gtk::Button,
    /// Brings the folded sidebar back on the done page.
    sidebar_toggle: gtk::ToggleButton,
    split: adw::OverlaySplitView,
    /// Pause, Stop and Expand in the header bar while compact.
    strip_buttons: gtk::Box,
    strip_pause: gtk::Button,
    /// The clock, as the header bar's title while compact.
    compact_title: gtk::Box,
    title_row: adw::EntryRow,
    format_row: adw::ComboRow,
    language_row: adw::ComboRow,
    timer_row: adw::ActionRow,
    timer_plan: RefCell<momr_core::timer::Plan>,
    /// Whether the one-minute warning has been shown for this recording.
    timer_warned: Cell<bool>,
    animation: TranscribeAnimation,
    meters: [gtk::DrawingArea; 2],
    /// The strip's two-lane wave.
    compact_wave: gtk::DrawingArea,
    dot: gtk::Label,
    timer: gtk::Label,
    compact_dot: gtk::Label,
    compact_timer: gtk::Label,
    status_label: gtk::Label,
    button: gtk::Button,
    copy_button: gtk::Button,
    again_button: gtk::Button,
    done_title_row: adw::EntryRow,
    done_group: adw::PreferencesGroup,
    /// One name row per speaker, rebuilt for every meeting.
    speaker_rows: RefCell<Vec<adw::EntryRow>>,
    again_language_row: adw::ComboRow,
    done_icon: gtk::Image,
    done_heading: gtk::Label,
    done_meta: gtk::Label,
    transcript_list: gtk::ListBox,
    transcript_scroll: gtk::ScrolledWindow,
    player: Player,
    /// Start of each transcript line in ms, by buffer line, for seeking and highlighting.
    segments: RefCell<Vec<(i32, i64)>>,
    /// The paragraphs on screen as (start ms, speaker, text), what the agent reads.
    lines: RefCell<Vec<(i64, String, String)>>,
    chapters_group: adw::PreferencesGroup,
    chapters_list: gtk::ListBox,
    /// Start of each row in the chapters list, in ms.
    chapter_starts: RefCell<Vec<i64>>,
    chapters_button: gtk::Button,
    chapters_spinner: adw::Spinner,
    /// Looked up once: the default agent, if any.
    agent: std::cell::OnceCell<Option<Agent>>,
    generating: Cell<bool>,
    current_line: Cell<i32>,

    mic: Source,
    system: Source,
    shared: SharedStatus,

    state: Cell<State>,
    compact: Cell<bool>,
    /// The size to restore when the strip expands again.
    full_size: Cell<(i32, i32)>,
    started_at: Cell<i64>,
    paused: Cell<bool>,
    frozen: [Frozen; 2],
    /// The waves only move while recording; before that they would look like it.
    live: Rc<Cell<bool>>,
    /// When the animation came on screen, to keep it there long enough.
    animation_since: Cell<Option<std::time::Instant>>,
    paused_secs: Cell<i64>,
    pause_began: Cell<i64>,
    /// Whether the silent computer-track hint has been shown this launch.
    silent_hint_shown: Cell<bool>,
    /// The `caffeinate` child keeping the Mac awake while recording.
    caffeinate: RefCell<Option<std::process::Child>>,
    pause_button: gtk::Button,
    import_button: gtk::Button,
    model_banner: adw::Banner,
    model_downloading: Cell<bool>,
    /// Why the computer source is not capturing, shown under the meters.
    audio_banner: adw::Banner,
    drop_hint: gtk::Label,
    staging: RefCell<Option<PathBuf>>,
    result_dir: RefCell<Option<PathBuf>>,
    abort: RefCell<Option<Abort>>,
    quit_when_done: Cell<bool>,
    /// Set while a meeting's own settings are shown, so they are not saved as defaults.
    loading: Cell<bool>,
    /// The settings of the meeting on the done page.
    manifest: RefCell<Option<Manifest>>,
}

impl Recorder {
    fn new(app: &adw::Application) -> Rc<Self> {
        let mic = Source::spawn(Device::Mic);
        let system = Source::spawn(Device::Computer);
        let shared: SharedStatus = Arc::new(Mutex::new(Status {
            state: "idle",
            ..Default::default()
        }));
        let (commands_tx, commands_rx) = async_channel::unbounded();
        let (mic_peaks, system_peaks) = (mic.clone(), system.clone());
        let socket_error = momr_core::ipc::serve(
            momr_core::ipc::socket_path(),
            shared.clone(),
            move || (mic_peaks.recent_peak(3), system_peaks.recent_peak(3)),
            commands_tx,
        )
        .err();

        // The size the window was closed with last time, so it opens the way
        // it was left; pages adapt to it instead of resizing it.
        let (width, height) = settings::load_window_size().unwrap_or(DEFAULT_SIZE);
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("MOM Recorder")
            .default_width(width)
            .default_height(height)
            .build();
        window.add_css_class("macos");
        window.set_size_request(MIN_SIZE.0, MIN_SIZE.1);
        let view = adw::ToolbarView::new();
        let header = adw::HeaderBar::new();
        let compact_button = gtk::Button::builder()
            .icon_name("view-restore-symbolic")
            .tooltip_text(t("strip.tooltip"))
            .action_name("win.compact")
            .build();
        header.pack_start(&compact_button);
        // On the done page in a narrow window the sidebar folds away; this
        // brings it back over the transcript.
        let sidebar_toggle = gtk::ToggleButton::builder()
            .icon_name("sidebar-show-symbolic")
            .tooltip_text(t("done.sidebar"))
            .visible(false)
            .build();
        header.pack_start(&sidebar_toggle);
        // Settings where Mac users look for it; ⌘, and the app menu stay.
        let gear = gtk::Button::builder()
            .icon_name("emblem-system-symbolic")
            .tooltip_text(t("prefs.button"))
            .action_name("app.preferences")
            .build();
        header.pack_end(&gear);
        // The strip's controls live in the header bar while compact, so the
        // strip needs no buttons of its own and the title bar stays the
        // handle to drag it by.
        let strip_pause = gtk::Button::builder()
            .icon_name("media-playback-pause-symbolic")
            .tooltip_text(t("ready.pause"))
            .action_name("win.pause")
            .css_classes(["flat", "circular"])
            .build();
        let strip_stop = gtk::Button::builder()
            .icon_name("media-playback-stop-symbolic")
            .tooltip_text(t("ready.stop"))
            .action_name("win.stop")
            .css_classes(["flat", "circular", "strip-stop"])
            .build();
        let strip_expand = gtk::Button::builder()
            .icon_name("view-fullscreen-symbolic")
            .tooltip_text(t("strip.tooltip"))
            .action_name("win.compact")
            .css_classes(["flat", "circular"])
            .build();
        let strip_buttons = gtk::Box::builder().spacing(2).visible(false).build();
        strip_buttons.append(&strip_pause);
        strip_buttons.append(&strip_stop);
        strip_buttons.append(&strip_expand);
        header.pack_end(&strip_buttons);
        view.add_top_bar(&header);
        // Under the header bar, full width, while the speech model still has
        // to be downloaded.
        let model_banner = adw::Banner::builder().revealed(false).build();
        view.add_top_bar(&model_banner);
        let toasts = adw::ToastOverlay::new();
        view.set_content(Some(&toasts));
        window.set_content(Some(&view));

        // Ready and recording: one column, clamped so a wide window keeps the
        // meters and the button together in the middle.
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(18)
            .margin_top(12)
            .margin_bottom(24)
            .margin_start(24)
            .margin_end(24)
            .build();

        let group = adw::PreferencesGroup::new();
        let title_row = adw::EntryRow::builder()
            .title(t("done.rename_meeting"))
            .show_apply_button(true)
            .build();
        let format_labels: Vec<&str> = Format::ALL.iter().map(|f| f.label()).collect();
        let format_row = adw::ComboRow::builder()
            .title(t("ready.format_title"))
            .subtitle(t("ready.format_subtitle"))
            .model(&gtk::StringList::new(&format_labels))
            .build();
        let saved = settings::load_format();
        format_row.set_selected(Format::ALL.iter().position(|f| *f == saved).unwrap_or(0) as u32);
        let language_labels: Vec<&str> = LANGUAGE_CODES
            .iter()
            .map(|code| language_label(code))
            .collect();
        let language_row = adw::ComboRow::builder()
            .title(t("ready.language_title"))
            .subtitle(t("ready.language_subtitle"))
            .model(&gtk::StringList::new(&language_labels))
            .build();
        let saved = settings::load_language();
        language_row.set_selected(
            LANGUAGE_CODES
                .iter()
                .position(|code| *code == saved)
                .unwrap_or(0) as u32,
        );
        // The timer: the plan in words, a click opens the dialog.
        let timer_row = adw::ActionRow::builder()
            .title(t("timer.row"))
            .subtitle(t("timer.off"))
            .activatable(true)
            .action_name("win.timer")
            .build();
        timer_row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        group.add(&title_row);
        group.add(&format_row);
        group.add(&language_row);
        group.add(&timer_row);
        content.append(&group);

        let frozen: [Frozen; 2] = Default::default();
        let live: Rc<Cell<bool>> = Rc::default();
        let meters = [
            meter(&mic, ("blue", MIC_COLOR), 56, &frozen[0], &live),
            meter(&system, ("orange", SYSTEM_COLOR), 56, &frozen[1], &live),
        ];
        let meters_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(18)
            .build();
        meters_box.append(&meter_block(t("ready.mic"), &meters[0]));
        meters_box.append(&meter_block(t("ready.computer"), &meters[1]));

        content.append(&meters_box);

        let audio_banner = adw::Banner::builder().revealed(false).build();
        content.append(&audio_banner);

        let status_row = gtk::Box::builder()
            .spacing(10)
            .halign(gtk::Align::Center)
            .build();
        let dot = gtk::Label::builder()
            .label("●")
            .css_classes(["error", "title-2"])
            .build();
        let timer = gtk::Label::builder()
            .label("00:00")
            .css_classes(["clock", "numeric"])
            .build();
        status_row.append(&dot);
        status_row.append(&timer);
        content.append(&status_row);

        let status_label = gtk::Label::builder()
            .css_classes(["dim-label"])
            .wrap(true)
            .justify(gtk::Justification::Center)
            .build();
        content.append(&status_label);

        let button = gtk::Button::builder().css_classes(["pill"]).build();
        let pause_button = gtk::Button::builder()
            .label(t("ready.pause"))
            .css_classes(["pill"])
            .visible(false)
            .build();
        let buttons = gtk::Box::builder()
            .spacing(10)
            .halign(gtk::Align::Center)
            .build();
        buttons.append(&pause_button);
        buttons.append(&button);
        content.append(&buttons);
        let import_button = gtk::Button::builder()
            .label(t("ready.import_hint"))
            .halign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        content.append(&import_button);
        let ready = adw::Clamp::builder()
            .child(&content)
            .maximum_size(640)
            .tightening_threshold(560)
            .build();
        let ready_scroll = gtk::ScrolledWindow::builder()
            .child(&ready)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();

        // Transcribing: the animation fills the whole window.
        let animation = TranscribeAnimation::new();

        // Done: a sidebar with the meeting and what to do with it, the player
        // and the transcript as the content. In a narrow window the sidebar
        // folds away and the header bar's toggle brings it back.
        let left = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(16)
            .margin_top(12)
            .margin_bottom(24)
            .margin_start(20)
            .margin_end(20)
            .build();
        let done_header = gtk::Box::builder().spacing(12).build();
        let done_icon = gtk::Image::builder()
            .icon_name("object-select-symbolic")
            .pixel_size(20)
            .css_classes(["done-icon"])
            .valign(gtk::Align::Center)
            .build();
        let done_text = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .valign(gtk::Align::Center)
            .build();
        let done_heading = gtk::Label::builder()
            .label(t("done.meeting_saved"))
            .xalign(0.0)
            .css_classes(["title-3"])
            .build();
        let done_meta = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .css_classes(["dim-label"])
            .build();
        done_text.append(&done_heading);
        done_text.append(&done_meta);
        done_header.append(&done_icon);
        done_header.append(&done_text);
        left.append(&done_header);

        let done_group = adw::PreferencesGroup::new();
        let done_title_row = adw::EntryRow::builder()
            .title(t("done.rename_meeting"))
            .show_apply_button(true)
            .build();
        done_group.add(&done_title_row);
        left.append(&done_group);

        // Chapters: a list to jump through, with the agent's button in the
        // header. The sidebar scrolls as a whole, so the list does not.
        let chapters_spinner = adw::Spinner::builder().visible(false).build();
        let chapters_button = gtk::Button::builder()
            .label(t("chapters.generate"))
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        let chapters_suffix = gtk::Box::builder().spacing(6).build();
        chapters_suffix.append(&chapters_spinner);
        chapters_suffix.append(&chapters_button);
        let chapters_group = adw::PreferencesGroup::builder()
            .title(t("done.chapters"))
            .header_suffix(&chapters_suffix)
            .build();
        let chapters_list = gtk::ListBox::builder()
            .css_classes(["boxed-list"])
            .selection_mode(gtk::SelectionMode::Single)
            .valign(gtk::Align::Start)
            .build();
        chapters_list.set_placeholder(Some(
            &gtk::Label::builder()
                .label(t("done.chapters_none"))
                .css_classes(["dim-label"])
                .margin_top(14)
                .margin_bottom(14)
                .build(),
        ));
        chapters_group.add(&chapters_list);
        left.append(&chapters_group);

        let copy_button = gtk::Button::builder()
            .label(t("done.copy"))
            .css_classes(["pill", "suggested-action"])
            .build();
        left.append(&copy_button);

        let actions = gtk::Box::builder().spacing(8).homogeneous(true).build();
        let open_button = gtk::Button::builder()
            .label(t("done.reveal"))
            .css_classes(["pill"])
            .action_name("win.reveal")
            .build();
        let new_button = gtk::Button::builder()
            .label(t("done.new"))
            .css_classes(["pill"])
            .action_name("win.new-recording")
            .build();
        actions.append(&open_button);
        actions.append(&new_button);
        left.append(&actions);

        let again_group = adw::PreferencesGroup::new();
        // No subtitle: the value needs the room, and the button says the rest.
        let again_language_row = adw::ComboRow::builder()
            .title(t("done.language_again"))
            .model(&gtk::StringList::new(&language_labels))
            .selected(language_row.selected())
            .build();
        let again_button = gtk::Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text(t("done.transcribe_again"))
            .valign(gtk::Align::Center)
            .css_classes(["flat", "circular"])
            .build();
        again_language_row.add_suffix(&again_button);
        again_group.add(&again_language_row);
        left.append(&again_group);
        let sidebar = gtk::ScrolledWindow::builder()
            .child(&left)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();

        let right = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .hexpand(true)
            .margin_top(12)
            .margin_bottom(24)
            .margin_start(20)
            .margin_end(24)
            .build();
        let player = Player::new();
        right.append(player.widget());
        // One row per paragraph: time and speaker in fixed columns, the text
        // wrapping in its own column so every line starts at the same place.
        let transcript_list = gtk::ListBox::builder()
            .css_classes(["transcript"])
            .selection_mode(gtk::SelectionMode::None)
            .activate_on_single_click(true)
            .build();
        let transcript_scroll = gtk::ScrolledWindow::builder()
            .child(&transcript_list)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .css_classes(["card"])
            .build();
        transcript_scroll.set_overflow(gtk::Overflow::Hidden);
        right.append(&transcript_scroll);

        let split = adw::OverlaySplitView::builder()
            .sidebar(&sidebar)
            .content(&right)
            .min_sidebar_width(340.0)
            .max_sidebar_width(400.0)
            .sidebar_width_fraction(0.36)
            .build();
        let narrow = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
            adw::BreakpointConditionLengthType::MaxWidth,
            860.0,
            adw::LengthUnit::Sp,
        ));
        narrow.add_setter(&split, "collapsed", Some(&true.to_value()));
        window.add_breakpoint(narrow);
        // Folded, the sidebar starts hidden and the toggle shows it as an
        // overlay; unfolded, it is simply there.
        split.connect_collapsed_notify(|split| split.set_show_sidebar(!split.is_collapsed()));
        split
            .bind_property("show-sidebar", &sidebar_toggle, "active")
            .bidirectional()
            .sync_create()
            .build();

        // Compact mode: the clock in the title bar, one two-lane wave under it.
        let compact_dot = gtk::Label::builder()
            .label("●")
            .css_classes(["error"])
            .build();
        let compact_timer = gtk::Label::builder()
            .label("00:00")
            .css_classes(["numeric", "strip-clock"])
            .build();
        let compact_title = gtk::Box::builder()
            .spacing(6)
            .valign(gtk::Align::Center)
            .build();
        compact_title.append(&compact_dot);
        compact_title.append(&compact_timer);
        let compact_wave = strip_wave(&mic, &system, &frozen, &live);

        // Not homogeneous, so the window can shrink to the compact page.
        let layout = gtk::Stack::builder()
            .hhomogeneous(false)
            .vhomogeneous(false)
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(250)
            .build();
        layout.add_named(&ready_scroll, Some("record"));
        layout.add_named(animation.widget(), Some("transcribing"));
        layout.add_named(&split, Some("done"));
        layout.add_named(&compact_wave, Some("compact"));
        let drop_hint = gtk::Label::builder()
            .label(t("ready.hint"))
            .css_classes(["drop-hint", "title-2"])
            .visible(false)
            .can_target(false)
            .build();
        let overlay = gtk::Overlay::builder().child(&layout).build();
        overlay.add_overlay(&drop_hint);
        toasts.set_child(Some(&overlay));
        let compact_action = gio::SimpleAction::new("compact", None);
        window.add_action(&compact_action);
        let quit_action = gio::SimpleAction::new("quit", None);
        app.add_action(&quit_action);

        let recorder = Rc::new(Recorder {
            window,
            view,
            header,
            toasts,
            layout,
            compact_action,
            compact_button,
            gear,
            sidebar_toggle,
            split,
            strip_buttons,
            strip_pause,
            compact_title,
            title_row,
            format_row,
            language_row,
            timer_row,
            timer_plan: RefCell::default(),
            timer_warned: Cell::new(false),
            animation,
            meters,
            compact_wave,
            dot,
            timer,
            compact_dot,
            compact_timer,
            status_label,
            button,
            copy_button,
            again_button,
            done_title_row,
            done_group,
            speaker_rows: RefCell::default(),
            again_language_row,
            done_icon,
            done_heading,
            done_meta,
            transcript_list,
            transcript_scroll,
            player,
            segments: RefCell::default(),
            lines: RefCell::default(),
            chapters_group,
            chapters_list,
            chapter_starts: RefCell::default(),
            chapters_button,
            chapters_spinner,
            agent: std::cell::OnceCell::new(),
            generating: Cell::new(false),
            current_line: Cell::new(-1),
            mic,
            system,
            shared,
            state: Cell::new(State::Idle),
            compact: Cell::new(false),
            full_size: Cell::new((width, height)),
            started_at: Cell::new(0),
            paused: Cell::new(false),
            frozen,
            live,
            animation_since: Cell::new(None),
            paused_secs: Cell::new(0),
            pause_began: Cell::new(0),
            silent_hint_shown: Cell::new(false),
            caffeinate: RefCell::default(),
            pause_button,
            import_button,
            model_banner,
            model_downloading: Cell::new(false),
            audio_banner,
            drop_hint,
            staging: RefCell::default(),
            result_dir: RefCell::default(),
            abort: RefCell::default(),
            quit_when_done: Cell::new(false),
            loading: Cell::new(false),
            manifest: RefCell::default(),
        });
        recorder.connect_signals(&quit_action);
        recorder.install_actions();
        let weak = Rc::downgrade(&recorder);
        glib::spawn_future_local(async move {
            while let Ok(command) = commands_rx.recv().await {
                let Some(r) = weak.upgrade() else { break };
                match command {
                    // A name typed on the ready page is kept; from the done page
                    // it starts fresh.
                    "start" if r.state.get() == State::Idle => r.start(),
                    "start" if r.state.get() == State::Done => {
                        r.ready();
                        r.start();
                    }
                    "stop" => r.stop(),
                    "compact" => r.set_compact(!r.compact.get()),
                    "pause" => r.toggle_pause(),
                    _ => {}
                }
            }
        });
        recorder.render();
        if let Some(e) = socket_error {
            recorder.toast(&tf("help.socket_failed", &[&e]));
        }
        recorder
    }

    fn connect_signals(self: &Rc<Self>, quit_action: &gio::SimpleAction) {
        let weak = Rc::downgrade(self);
        self.button.connect_clicked(move |_| {
            let Some(r) = weak.upgrade() else { return };
            match r.state.get() {
                State::Idle | State::Done => r.start(),
                State::Recording => r.stop(),
                _ => {}
            }
        });

        // Import: a file dropped anywhere on the window, or picked from a dialog.
        // Finder offers a file list; other sources a single file.
        let drop = gtk::DropTarget::new(glib::Type::INVALID, gtk::gdk::DragAction::COPY);
        drop.set_types(&[gtk::gdk::FileList::static_type(), gio::File::static_type()]);
        // GTK's own check turns the drag down when the source prefers to
        // move rather than copy, so decide on
        // the content alone. The file is only read; it is always a copy.
        drop.connect_accept(|_, offer| {
            let formats = offer.formats();
            formats.contains_type(gtk::gdk::FileList::static_type())
                || formats.contains_type(gio::File::static_type())
        });
        drop.connect_motion(|_, _, _| gtk::gdk::DragAction::COPY);
        let weak = Rc::downgrade(self);
        drop.connect_drop(move |_, value, _, _| {
            let Some(r) = weak.upgrade() else {
                return false;
            };
            r.drop_hint.set_visible(false);
            let file = value
                .get::<gtk::gdk::FileList>()
                .ok()
                .and_then(|list| list.files().into_iter().next())
                .or_else(|| value.get::<gio::File>().ok());
            let Some(file) = file else {
                return false;
            };
            match file.path() {
                Some(path) => {
                    r.confirm_import(path);
                    true
                }
                None => false,
            }
        });
        // While a file hovers over the window, say what dropping it does.
        let weak = Rc::downgrade(self);
        drop.connect_enter(move |_, _, _| {
            if let Some(r) = weak.upgrade() {
                r.drop_hint.set_visible(true);
            }
            gtk::gdk::DragAction::COPY
        });
        let weak = Rc::downgrade(self);
        drop.connect_leave(move |_| {
            if let Some(r) = weak.upgrade() {
                r.drop_hint.set_visible(false);
            }
        });
        self.window.add_controller(drop);

        let weak = Rc::downgrade(self);
        self.model_banner.connect_button_clicked(move |_| {
            if let Some(r) = weak.upgrade() {
                r.download_model();
            }
        });
        self.update_model_banner();

        let weak = Rc::downgrade(self);
        self.compact_action.connect_activate(move |_, _| {
            if let Some(r) = weak.upgrade() {
                r.set_compact(!r.compact.get());
            }
        });

        // ⌘Q goes through the same check as the close button.
        let weak = Rc::downgrade(self);
        quit_action.connect_activate(move |_, _| {
            if let Some(r) = weak.upgrade() {
                r.window.close();
            }
        });

        let weak = Rc::downgrade(self);
        self.format_row.connect_selected_notify(move |_| {
            if let Some(r) = weak.upgrade()
                && !r.loading.get()
            {
                Self::saved(&weak, settings::save_format(r.selected_format()));
            }
        });

        // The two language rows (recording page, done page) are one setting.
        let weak = Rc::downgrade(self);
        self.language_row.connect_selected_notify(move |row| {
            if let Some(r) = weak.upgrade() {
                if !r.loading.get() {
                    Self::saved(&weak, settings::save_language(r.selected_language()));
                }
                if r.again_language_row.selected() != row.selected() {
                    r.again_language_row.set_selected(row.selected());
                }
            }
        });
        let weak = Rc::downgrade(self);
        self.again_language_row.connect_selected_notify(move |row| {
            if let Some(r) = weak.upgrade()
                && r.language_row.selected() != row.selected()
            {
                r.language_row.set_selected(row.selected());
            }
        });

        // A click on a transcript row plays the meeting from there.
        let weak = Rc::downgrade(self);
        self.transcript_list.connect_row_activated(move |_, row| {
            let Some(r) = weak.upgrade() else { return };
            let index = row.index();
            let start = r
                .segments
                .borrow()
                .iter()
                .find(|(i, _)| *i == index)
                .map(|(_, ms)| *ms);
            if let Some(ms) = start {
                r.player.play_from(ms);
            }
        });

        let weak = Rc::downgrade(self);
        self.player.connect_error(move |reason| {
            if let Some(r) = weak.upgrade() {
                r.toast(&tf("player.failed", &[reason]));
            }
        });

        let weak = Rc::downgrade(self);
        self.player.connect_position(move |ms| {
            if let Some(r) = weak.upgrade() {
                r.highlight(ms);
            }
        });

        let weak = Rc::downgrade(self);
        self.chapters_list.connect_row_activated(move |_, row| {
            if let Some(r) = weak.upgrade() {
                let start = r.chapter_starts.borrow().get(row.index() as usize).copied();
                if let Some(ms) = start {
                    r.player.play_from(ms);
                }
            }
        });

        let weak = Rc::downgrade(self);
        self.chapters_button.connect_clicked(move |_| {
            if let Some(r) = weak.upgrade() {
                r.generate_chapters();
            }
        });

        // The name on the done page drives the same title and folder rename.
        let weak = Rc::downgrade(self);
        self.done_title_row.connect_changed(move |row| {
            if let Some(r) = weak.upgrade()
                && r.title_row.text() != row.text()
            {
                r.title_row.set_text(&row.text());
            }
        });
        let weak = Rc::downgrade(self);
        self.done_title_row.connect_apply(move |_| {
            if let Some(r) = weak.upgrade() {
                r.apply_title();
            }
        });
        let focus = gtk::EventControllerFocus::new();
        let weak = Rc::downgrade(self);
        focus.connect_leave(move |_| {
            if let Some(r) = weak.upgrade() {
                r.apply_title();
            }
        });
        self.done_title_row.add_controller(focus);

        let weak = Rc::downgrade(self);
        self.title_row.connect_changed(move |row| {
            if let Some(r) = weak.upgrade() {
                if r.state.get() == State::Recording
                    && let Some(staging) = r.staging.borrow().as_ref()
                {
                    write_recording_note(
                        staging,
                        row.text().trim(),
                        r.started_at.get(),
                        r.selected_format(),
                        r.selected_language(),
                    );
                }
                r.shared.lock().unwrap().title = row.text().to_string();
            }
        });

        // A new name renames the meeting folder, on Enter or when the row loses focus.
        let weak = Rc::downgrade(self);
        self.title_row.connect_apply(move |_| {
            if let Some(r) = weak.upgrade() {
                r.apply_title();
            }
        });
        let focus = gtk::EventControllerFocus::new();
        let weak = Rc::downgrade(self);
        focus.connect_leave(move |_| {
            if let Some(r) = weak.upgrade() {
                r.apply_title();
            }
        });
        self.title_row.add_controller(focus);

        let weak = Rc::downgrade(self);
        self.again_button.connect_clicked(move |_| {
            let Some(r) = weak.upgrade() else { return };
            r.transcribe_again(r.selected_language());
        });

        // The done page's sidebar toggle appears when the split folds.
        let weak = Rc::downgrade(self);
        self.split.connect_collapsed_notify(move |_| {
            if let Some(r) = weak.upgrade() {
                r.show_page();
            }
        });

        // Coming back to the app after a change in System Settings: follow
        // it, on a GTK that cannot do so itself (see theme.rs).
        self.window.connect_is_active_notify(|window| {
            if window.is_active() {
                crate::theme::apply_appearance(
                    settings::load_appearance(),
                    crate::theme::macos_prefers_dark,
                );
            }
        });

        let weak = Rc::downgrade(self);
        self.window.connect_close_request(move |_| {
            let Some(r) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            r.remember_size();
            match r.state.get() {
                State::Recording => {
                    r.confirm_close_recording();
                    glib::Propagation::Stop
                }
                State::Transcribing => {
                    r.confirm_close_transcribing();
                    glib::Propagation::Stop
                }
                // Saving the audio takes a moment; finish it, then quit.
                State::Stopping => {
                    r.close_when_done();
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });

        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(33), move || match weak.upgrade() {
            Some(r) => {
                if r.window.is_visible() {
                    if r.compact.get() {
                        r.compact_wave.queue_draw();
                    } else if r.layout.visible_child_name().as_deref() == Some("record") {
                        r.meters.iter().for_each(|m| m.queue_draw());
                    }
                }
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });

        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(500), move || match weak.upgrade() {
            Some(r) => {
                r.tick();
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
    }

    fn toast(&self, message: &str) {
        self.toasts.add_toast(adw::Toast::new(message));
    }

    /// Every user-facing operation as a `GAction`, so buttons and menu items
    /// share enabled state instead of each tracking it. Called once, after
    /// the recorder exists.
    fn install_actions(self: &Rc<Self>) {
        let act = |name: &str| {
            let action = gio::SimpleAction::new(name, None);
            self.window.add_action(&action);
            action
        };
        Self::on(&act("new-recording"), self, |r| {
            if matches!(r.state.get(), State::Idle | State::Done) {
                r.ready();
            }
        });
        Self::on(&act("open-meeting"), self, |r| r.open_meeting_dialog());
        Self::on(&act("import"), self, |r| r.ask_import_file());
        Self::on(&act("reveal"), self, |r| r.reveal_in_finder());
        Self::on(&act("start"), self, |r| {
            if r.state.get() == State::Idle {
                r.start();
            }
        });
        Self::on(&act("pause"), self, |r| r.toggle_pause());
        Self::on(&act("stop"), self, |r| r.stop());
        Self::on(&act("timer"), self, |r| r.show_timer_dialog());
        Self::on(&act("copy-transcript"), self, |r| r.copy_transcript());
        Self::on(&act("fullscreen"), self, |r| r.toggle_fullscreen());
        let again = gio::SimpleAction::new("transcribe-again", Some(glib::VariantTy::STRING));
        let weak = Rc::downgrade(self);
        again.connect_activate(move |_, param| {
            let Some(r) = weak.upgrade() else { return };
            let language = param.and_then(|p| p.get::<String>()).unwrap_or_default();
            r.transcribe_again(&language);
        });
        self.window.add_action(&again);
        // Single-purpose buttons trigger the same actions as the menu.
        self.pause_button.set_action_name(Some("win.pause"));
        self.import_button.set_action_name(Some("win.import"));
        self.copy_button
            .set_action_name(Some("win.copy-transcript"));

        let app = self.window.application().expect("window has an app");
        let about = gio::SimpleAction::new("about", None);
        app.add_action(&about);
        Self::on(&about, self, |r| r.show_about());
        let help = gio::SimpleAction::new("help", None);
        app.add_action(&help);
        Self::on(&help, self, |r| r.show_help());
        let preferences = gio::SimpleAction::new("preferences", None);
        app.add_action(&preferences);
        Self::on(&preferences, self, |r| r.show_preferences());
    }

    fn on(action: &gio::SimpleAction, this: &Rc<Self>, run: impl Fn(Rc<Self>) + 'static) {
        let weak = Rc::downgrade(this);
        action.connect_activate(move |_, _| {
            if let Some(r) = weak.upgrade() {
                run(r);
            }
        });
    }

    /// One `set_enabled` per state transition; the menu greys out with it.
    fn update_actions(&self) {
        let state = self.state.get();
        let enable = |name: &str, on: bool| {
            if let Some(action) = self.window.lookup_action(name)
                && let Ok(action) = action.downcast::<gio::SimpleAction>()
            {
                action.set_enabled(on);
            }
        };
        let idle_done = matches!(state, State::Idle | State::Done);
        let recording = state == State::Recording;
        enable("new-recording", idle_done);
        enable("open-meeting", true);
        enable("import", idle_done);
        enable("reveal", state == State::Done);
        enable("start", state == State::Idle);
        enable("pause", recording);
        enable("stop", recording);
        enable("compact", recording);
        enable("timer", matches!(state, State::Idle | State::Recording));
        enable("copy-transcript", state == State::Done);
        enable("transcribe-again", state == State::Done);
        enable("fullscreen", true);
    }

    /// The import file panel (NSOpenPanel through GTK's quartz chooser).
    fn ask_import_file(self: &Rc<Self>) {
        if !matches!(self.state.get(), State::Idle | State::Done) {
            return;
        }
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(t("import.filter")));
        filter.add_mime_type("audio/*");
        filter.add_mime_type("video/*");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title(t("import.title"))
            .filters(&filters)
            .build();
        let this = self.clone();
        dialog.open(Some(&self.window), gio::Cancellable::NONE, move |result| {
            if let Some(path) = result.ok().and_then(|f| f.path()) {
                this.confirm_import(path);
            }
        });
    }

    /// The open panel: a `.meeting-recorder` file from a meeting folder.
    fn open_meeting_dialog(self: &Rc<Self>) {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(t("open.filter")));
        filter.add_suffix("meeting-recorder");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title(t("open.title"))
            .filters(&filters)
            .build();
        let this = self.clone();
        dialog.open(Some(&self.window), gio::Cancellable::NONE, move |result| {
            if let Some(path) = result.ok().and_then(|f| f.path()) {
                this.open_meeting(&path);
            }
        });
    }

    /// Selects the meeting's manifest in Finder, the way "Reveal" means here.
    fn reveal_in_finder(&self) {
        let Some(dir) = self.result_dir.borrow().clone() else {
            return;
        };
        let target = momr_core::meeting::find(&dir).unwrap_or(dir);
        let _ = std::process::Command::new("open")
            .arg("-R")
            .arg(target)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }

    fn copy_transcript(&self) {
        let Some(dir) = self.result_dir.borrow().clone() else {
            return;
        };
        match std::fs::read_to_string(dir.join("transcript.md")) {
            Ok(text) => {
                self.window.clipboard().set_text(&text);
                self.toast(t("help.copied_clipboard"));
            }
            Err(_) => self.toast(t("done.no_transcript")),
        }
    }

    /// Transcribes the done meeting again in `language` (a code from
    /// `LANGUAGE_CODES`, falling back to the selected one).
    fn transcribe_again(self: &Rc<Self>, language: &str) {
        if self.state.get() != State::Done {
            return;
        }
        let language: &'static str = momr_core::transcribe::LANGUAGE_CODES
            .iter()
            .find(|code| **code == language)
            .copied()
            .unwrap_or_else(|| self.selected_language());
        let Some(dir) = self.result_dir.borrow().clone() else {
            return;
        };
        let tracks = match self.manifest.borrow().as_ref() {
            Some(m) if m.imported.is_some() => Tracks::Single(source_track(&dir), m.speaker_count),
            _ => Tracks::Kept(dir),
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let result = this.run_transcription(tracks, language).await;
            this.hold_animation(&result).await;
            this.finished(true, result);
        });
    }

    /// Asks the window, not a copy of its state: the green button, ⌃⌘F and
    /// Esc change it too.
    fn toggle_fullscreen(&self) {
        if !self.window.is_fullscreen() {
            self.window.fullscreen();
        } else {
            self.window.unfullscreen();
        }
    }

    /// The Timer dialog (⌘T): stop after a length, start at a time, stop at
    /// a time, each behind its own switch. Set replaces the whole plan.
    fn show_timer_dialog(self: &Rc<Self>) {
        let dialog = adw::Dialog::builder()
            .title(t("timer.title"))
            .content_width(460)
            .build();
        let view = adw::ToolbarView::new();
        let header = adw::HeaderBar::builder()
            .show_start_title_buttons(false)
            .show_end_title_buttons(false)
            .build();
        let cancel = gtk::Button::with_label(t("import.cancel"));
        let set = gtk::Button::builder()
            .label(t("timer.set"))
            .css_classes(["suggested-action"])
            .build();
        header.pack_start(&cancel);
        header.pack_end(&set);
        view.add_top_bar(&header);
        let page = adw::PreferencesPage::new();
        view.set_content(Some(&page));
        dialog.set_child(Some(&view));

        let plan = self.timer_plan.borrow().clone();
        let now = glib::DateTime::now_local().ok();
        let local = |at: i64| glib::DateTime::from_unix_local(at).ok();
        // A switch row with hour and minute spin rows that follow it.
        let clock_group = |title: &str, on: bool, hour: i32, minute: i32, hour_max: f64| {
            let group = adw::PreferencesGroup::new();
            let switch = adw::SwitchRow::builder().title(title).active(on).build();
            let hours = adw::SpinRow::with_range(0.0, hour_max, 1.0);
            hours.set_title(t("timer.hours_row"));
            hours.set_value(f64::from(hour));
            let minutes = adw::SpinRow::with_range(0.0, 59.0, 1.0);
            minutes.set_title(t("timer.minutes_row"));
            minutes.set_value(f64::from(minute));
            for row in [&hours, &minutes] {
                switch
                    .bind_property("active", row, "sensitive")
                    .sync_create()
                    .build();
            }
            group.add(&switch);
            group.add(&hours);
            group.add(&minutes);
            (group, switch, hours, minutes)
        };

        let default_length = plan
            .max_secs
            .map_or(i64::from(settings::load_timer_minutes()), |s| s / 60);
        let (length_group, length_on, length_h, length_m) = clock_group(
            t("timer.stop_after"),
            plan.max_secs.is_some(),
            (default_length / 60) as i32,
            (default_length % 60) as i32,
            24.0,
        );
        page.add(&length_group);
        let default_time = |at: Option<i64>, hours_ahead: i32| {
            at.and_then(local)
                .map(|w| (w.hour(), w.minute()))
                .or_else(|| now.as_ref().map(|n| ((n.hour() + hours_ahead) % 24, 0)))
                .unwrap_or((9, 0))
        };
        let (start_hour, start_minute) = default_time(plan.start_at, 1);
        let (start_group, start_on, start_h, start_m) = clock_group(
            t("timer.start_at"),
            plan.start_at.is_some(),
            start_hour,
            start_minute,
            23.0,
        );
        page.add(&start_group);
        let (stop_hour, stop_minute) = default_time(plan.stop_at, 2);
        let (stop_group, stop_on, stop_h, stop_m) = clock_group(
            t("timer.stop_at"),
            plan.stop_at.is_some(),
            stop_hour,
            stop_minute,
            23.0,
        );
        stop_group.set_description(Some(t("timer.times_hint")));
        page.add(&stop_group);

        let close = dialog.clone();
        cancel.connect_clicked(move |_| {
            close.close();
        });
        let this = self.clone();
        let close = dialog.clone();
        set.connect_clicked(move |_| {
            let value = |row: &adw::SpinRow| row.value().round() as i64;
            let mut plan = momr_core::timer::Plan::default();
            if length_on.is_active() {
                let secs = value(&length_h) * 3600 + value(&length_m) * 60;
                if secs <= 0 {
                    this.toast(t("timer.needs_length"));
                    return;
                }
                plan.max_secs = Some(secs);
                let _ = settings::save_timer_minutes((secs / 60) as u32);
            }
            let now = momr_core::ipc::now();
            let clock = |h: &adw::SpinRow, m: &adw::SpinRow, after: i64| {
                momr_core::timer::next_occurrence(value(h) as u32, value(m) as u32, after)
            };
            if start_on.is_active() {
                plan.start_at = clock(&start_h, &start_m, now);
                if plan.start_at.is_none() {
                    this.toast(t("timer.no_such_time"));
                    return;
                }
            }
            if stop_on.is_active() {
                // After the start when there is one, so a stop before the
                // start means the day after it, DST changes included.
                plan.stop_at = clock(&stop_h, &stop_m, plan.start_at.unwrap_or(now));
                if plan.stop_at.is_none() {
                    this.toast(t("timer.no_such_time"));
                    return;
                }
            }
            this.set_timer(plan);
            close.close();
        });
        dialog.present(Some(&self.window));
    }

    /// Puts a timer plan in force and shows it on the ready page.
    fn set_timer(&self, plan: momr_core::timer::Plan) {
        self.timer_row.set_subtitle(&plan.describe());
        self.timer_warned.set(false);
        *self.timer_plan.borrow_mut() = plan;
        self.status_label.set_label(&self.status_text());
    }

    fn show_about(&self) {
        let dialog = adw::AboutDialog::builder()
            .application_name("MOM Recorder")
            .application_icon("io.github.riobahtiar.MOMRecorder")
            .version(env!("CARGO_PKG_VERSION"))
            .developer_name("Rio Bahtiar")
            .copyright("© 2026 Rio Bahtiar")
            .comments(t("about.comments"))
            .website("https://github.com/riobahtiar/mo-meeting-recorder")
            .issue_url("https://github.com/riobahtiar/mo-meeting-recorder/issues")
            .license_type(gtk::License::MitX11)
            .build();
        dialog.add_acknowledgement_section(
            Some(t("about.transcription_credit")),
            &[
                "whisper.cpp through whisper-rs",
                "Nemotron 3 Diarization (ONNX community export)",
                t("about.based_on"),
            ],
        );
        dialog.present(Some(&self.window));
    }

    fn show_help(&self) {
        let uri = "https://github.com/riobahtiar/mo-meeting-recorder";
        let _ = gio::AppInfo::launch_default_for_uri(uri, None::<&gio::AppLaunchContext>);
    }
    /// What leaves the Mac per transcription provider, for the Settings row.
    fn provider_privacy(provider: Provider) -> &'static str {
        match provider {
            Provider::Local => t("prefs.provider_local_note"),
            Provider::Cloud(Cloud::ElevenLabs) => t("prefs.provider_eleven_note"),
            Provider::Cloud(Cloud::Google) => t("prefs.provider_google_note"),
            Provider::Cloud(Cloud::OpenRouter) => t("prefs.provider_openrouter_note"),
        }
    }

    /// Tells the user a setting did not stick, for the rows below: a toast
    /// with the reason, since the row itself already shows the new value.
    fn saved(weak: &std::rc::Weak<Self>, result: std::io::Result<()>) {
        if let (Err(e), Some(r)) = (result, weak.upgrade()) {
            r.toast(&momr_core::locales::tf(
                "prefs.save_failed",
                &[&e.to_string()],
            ));
        }
    }

    /// App settings (⌘,), as pages: General (interface language, appearance,
    /// menu bar item), Transcription (model, language, provider, keys),
    /// Recording (format, your name, meetings folder, timer default), Audio
    /// (microphone, computer audio and the apps it records) and Storage
    /// (cache, models, reset). Most values apply at once; the interface
    /// language and the menu bar item on the next launch, as their rows say.
    /// The ready page re-reads the model banner and the language on close.
    fn show_preferences(self: &Rc<Self>) {
        let dialog = adw::PreferencesDialog::builder()
            .title(t("prefs.title"))
            .content_width(820)
            .content_height(640)
            .build();
        dialog.add_css_class("macos");
        Self::hide_prefs_close(&dialog);
        dialog.add(&self.prefs_general());
        dialog.add(&self.prefs_transcription());
        dialog.add(&self.prefs_recording());
        dialog.add(&self.prefs_audio());
        dialog.add(&self.prefs_storage(&dialog));
        let weak = Rc::downgrade(self);
        dialog.connect_closed(move |_| {
            let Some(r) = weak.upgrade() else { return };
            r.settings_changed();
        });
        dialog.present(Some(&self.window));
    }

    /// Hides the × libadwaita packs at the start of a PreferencesDialog
    /// header. The dialog window already has traffic lights, so the extra
    /// button only duplicates Close while stealing width from the page
    /// switcher, whose longer titles ("Transkripsi", "Penyimpanan") then
    /// ellipsize. Matches by icon, so a future libadwaita that moves the
    /// button only means it stays visible, never a crash.
    fn hide_prefs_close(dialog: &adw::PreferencesDialog) {
        fn walk(widget: &gtk::Widget) {
            if let Ok(button) = widget.clone().downcast::<gtk::Button>()
                && button.icon_name().as_deref() == Some("window-close-symbolic")
            {
                button.set_visible(false);
            }
            let mut next = widget.first_child();
            while let Some(child) = next {
                walk(&child);
                next = child.next_sibling();
            }
        }
        if let Some(child) = dialog.first_child() {
            walk(&child);
        }
    }

    /// Re-reads what the ready page shows from the settings files, after
    /// Settings closes or a reset.
    fn settings_changed(&self) {
        self.update_model_banner();
        let saved = settings::load_language();
        if let Some(index) = LANGUAGE_CODES.iter().position(|code| *code == saved) {
            self.loading.set(true);
            self.language_row.set_selected(index as u32);
            self.again_language_row.set_selected(index as u32);
            self.loading.set(false);
        }
        let format = settings::load_format();
        if let Some(index) = Format::ALL.iter().position(|f| *f == format) {
            self.loading.set(true);
            self.format_row.set_selected(index as u32);
            self.loading.set(false);
        }
    }

    fn prefs_page(title: &str, icon: &str) -> adw::PreferencesPage {
        adw::PreferencesPage::builder()
            .title(title)
            .icon_name(icon)
            .build()
    }

    fn prefs_general(self: &Rc<Self>) -> adw::PreferencesPage {
        let page = Self::prefs_page(t("prefs.page_general"), "preferences-system-symbolic");
        let interface = adw::PreferencesGroup::builder()
            .title(t("prefs.interface"))
            .description(t("prefs.ui_language_hint"))
            .build();
        let ui_language = adw::ComboRow::builder()
            .title(t("prefs.ui_language"))
            .model(&gtk::StringList::new(&["English", "Bahasa Indonesia"]))
            .selected(match momr_core::locales::current() {
                Lang::English => 0,
                Lang::Indonesian => 1,
            })
            .build();
        let weak = Rc::downgrade(self);
        ui_language.connect_selected_notify(move |row| {
            let lang = if row.selected() == 1 {
                Lang::Indonesian
            } else {
                Lang::English
            };
            Self::saved(&weak, momr_core::settings::save_ui_language(lang));
        });
        interface.add(&ui_language);
        let appearance_labels = [
            t("prefs.appearance_system"),
            t("prefs.appearance_light"),
            t("prefs.appearance_dark"),
        ];
        let appearance = adw::ComboRow::builder()
            .title(t("prefs.appearance"))
            .model(&gtk::StringList::new(&appearance_labels))
            .selected(
                settings::Appearance::ALL
                    .iter()
                    .position(|a| *a == settings::load_appearance())
                    .unwrap_or(0) as u32,
            )
            .build();
        let weak = Rc::downgrade(self);
        appearance.connect_selected_notify(move |row| {
            let choice = settings::Appearance::ALL[row.selected() as usize];
            crate::theme::apply_appearance(choice, crate::theme::macos_prefers_dark);
            Self::saved(&weak, settings::save_appearance(choice));
        });
        interface.add(&appearance);
        page.add(&interface);

        let menubar_group = adw::PreferencesGroup::builder()
            .title(t("prefs.menubar"))
            .description(t("prefs.menubar_restart"))
            .build();
        let menubar_row = adw::SwitchRow::builder()
            .title(t("prefs.menubar_show"))
            .active(momr_core::models::menubar_enabled())
            .build();
        let weak = Rc::downgrade(self);
        menubar_row.connect_active_notify(move |row| {
            Self::saved(
                &weak,
                momr_core::models::save_menubar_enabled(row.is_active()),
            );
        });
        menubar_group.add(&menubar_row);
        page.add(&menubar_group);
        page
    }

    fn prefs_transcription(self: &Rc<Self>) -> adw::PreferencesPage {
        let page = Self::prefs_page(t("prefs.transcription"), "audio-input-microphone-symbolic");
        let transcription = adw::PreferencesGroup::builder()
            .title(t("prefs.transcription"))
            .build();
        let model_names: Vec<&str> = momr_core::models::MODELS.iter().map(|m| m.name).collect();
        let model_row = adw::ComboRow::builder()
            .title(t("prefs.model"))
            .model(&gtk::StringList::new(&model_names))
            .build();
        let current_model = momr_core::models::configured();
        model_row.set_selected(
            momr_core::models::MODELS
                .iter()
                .position(|m| m.name == current_model)
                .unwrap_or(0) as u32,
        );
        let model_subtitle = |name: &str| {
            let model = momr_core::models::MODELS.iter().find(|m| m.name == name);
            let downloaded = model.is_some_and(|m| {
                momr_core::transcribe::models_dir()
                    .join(format!("ggml-{}.bin", m.name))
                    .is_file()
            });
            match (downloaded, model) {
                (true, _) => t("prefs.model_present").to_owned(),
                (_, Some(m)) if m.size_mb >= 1000 => t("prefs.model_size_gb")
                    .replace("{size}", &format!("{:.1}", f64::from(m.size_mb) / 1000.0)),
                (_, Some(m)) => t("prefs.model_size_mb").replace("{size}", &m.size_mb.to_string()),
                (_, None) => String::new(),
            }
        };
        model_row.set_subtitle(&model_subtitle(&current_model));
        let weak = Rc::downgrade(self);
        model_row.connect_selected_notify(move |row| {
            let Some(r) = weak.upgrade() else { return };
            let name = model_names[row.selected() as usize];
            Self::saved(
                &Rc::downgrade(&r),
                momr_core::models::save_config_value("model", name),
            );
            row.set_subtitle(&model_subtitle(name));
            r.update_model_banner();
        });
        transcription.add(&model_row);
        let language_labels: Vec<&str> = LANGUAGE_CODES
            .iter()
            .map(|code| language_label(code))
            .collect();
        let prefs_language = adw::ComboRow::builder()
            .title(t("prefs.language"))
            .model(&gtk::StringList::new(&language_labels))
            .build();
        let saved_language = settings::load_language();
        prefs_language.set_selected(
            LANGUAGE_CODES
                .iter()
                .position(|code| *code == saved_language)
                .unwrap_or(0) as u32,
        );
        let weak = Rc::downgrade(self);
        prefs_language.connect_selected_notify(move |row| {
            let Some(r) = weak.upgrade() else { return };
            let code = LANGUAGE_CODES[row.selected() as usize];
            Self::saved(&Rc::downgrade(&r), settings::save_language(code));
            // The ready page owns the default: keep its dropdown in step.
            r.loading.set(true);
            r.language_row.set_selected(row.selected());
            r.again_language_row.set_selected(row.selected());
            r.loading.set(false);
        });
        transcription.add(&prefs_language);
        let provider_names: Vec<&str> = Provider::ALL.iter().map(|p| p.label()).collect();
        let provider_row = adw::ComboRow::builder()
            .title(t("prefs.provider"))
            .model(&gtk::StringList::new(&provider_names))
            .build();
        // An id in config.toml that names no provider shows as local here and
        // says so, rather than looking like a choice that was made.
        let (current_provider, provider_problem) = match momr_core::provider::configured() {
            Ok(provider) => (provider, None),
            Err(e) => (Provider::Local, Some(e)),
        };
        provider_row.set_selected(
            Provider::ALL
                .iter()
                .position(|p| *p == current_provider)
                .unwrap_or(0) as u32,
        );
        provider_row.set_subtitle(
            provider_problem
                .as_deref()
                .unwrap_or(Self::provider_privacy(current_provider)),
        );
        let weak = Rc::downgrade(self);
        provider_row.connect_selected_notify(move |row| {
            let provider = Provider::ALL[row.selected() as usize];
            Self::saved(&weak, momr_core::provider::save_configured(provider));
            row.set_subtitle(Self::provider_privacy(provider));
        });
        transcription.add(&provider_row);
        page.add(&transcription);

        // One expander per provider: the status in the subtitle, the key
        // field and the where-to-get-it hint inside, in full.
        let keys = adw::PreferencesGroup::builder()
            .title(t("prefs.keys"))
            .description(t("prefs.keys_about"))
            .build();
        for (cloud, title, hint) in [
            (
                Cloud::ElevenLabs,
                t("prefs.eleven_key"),
                t("prefs.key_hint_eleven"),
            ),
            (
                Cloud::Google,
                t("prefs.google_key"),
                t("prefs.key_hint_google"),
            ),
            (
                Cloud::OpenRouter,
                t("prefs.openrouter_key"),
                t("prefs.key_hint_openrouter"),
            ),
        ] {
            let state = match momr_core::provider::key_status(cloud) {
                Ok(()) => t("prefs.key_saved").to_owned(),
                Err(momr_core::provider::KeyError::Missing(_)) => t("prefs.key_none").to_owned(),
                Err(e) => e.to_string(),
            };
            let expander = adw::ExpanderRow::builder()
                .title(title)
                .subtitle(&state)
                .build();
            let key_row = adw::PasswordEntryRow::builder()
                .title(t("prefs.key_paste"))
                .show_apply_button(true)
                .build();
            let hint_row = adw::ActionRow::builder()
                .title(t("prefs.key_where"))
                .subtitle(hint)
                .subtitle_lines(0)
                .build();
            let weak = Rc::downgrade(self);
            let status = expander.clone();
            key_row.connect_apply(move |row| {
                let key = row.text().to_string();
                row.set_text("");
                match momr_core::provider::save_api_key(cloud, &key) {
                    Ok(()) => {
                        status.set_subtitle(t("prefs.key_saved"));
                        if let Some(r) = weak.upgrade() {
                            r.toast(t("prefs.key_saved_toast"));
                        }
                    }
                    Err(e) => {
                        if let Some(r) = weak.upgrade() {
                            r.toast(&e);
                        }
                    }
                }
            });
            expander.add_row(&key_row);
            expander.add_row(&hint_row);
            keys.add(&expander);
        }
        page.add(&keys);

        let chapters = adw::PreferencesGroup::builder()
            .title(t("prefs.chapters"))
            .description(t("prefs.chapters_about"))
            .build();
        let agents = momr_core::agent::installed_agents();
        let mut agent_names = vec![t("prefs.agent_none")];
        agent_names.extend(agents.iter().map(|a| a.name));
        let agent_row = adw::ComboRow::builder()
            .title(t("prefs.agent"))
            .model(&gtk::StringList::new(&agent_names))
            .build();
        let current_agent = momr_core::agent::configured_id().unwrap_or_default();
        agent_row.set_selected(
            agents
                .iter()
                .position(|a| a.id == current_agent)
                .map(|i| i + 1)
                .unwrap_or(0) as u32,
        );
        let weak = Rc::downgrade(self);
        agent_row.connect_selected_notify(move |row| {
            let selected = row.selected() as usize;
            let id = match selected {
                0 => "",
                n => agents[n - 1].id,
            };
            Self::saved(&weak, momr_core::agent::save_configured_id(id));
        });
        chapters.add(&agent_row);
        page.add(&chapters);
        page
    }

    fn prefs_recording(self: &Rc<Self>) -> adw::PreferencesPage {
        let page = Self::prefs_page(t("prefs.recording"), "media-record-symbolic");
        let recording = adw::PreferencesGroup::builder()
            .title(t("prefs.recording"))
            .build();
        let format_labels: Vec<&str> = Format::ALL.iter().map(|f| f.label()).collect();
        let format_row = adw::ComboRow::builder()
            .title(t("prefs.format"))
            .model(&gtk::StringList::new(&format_labels))
            .build();
        format_row.set_selected(
            Format::ALL
                .iter()
                .position(|f| *f == settings::load_format())
                .unwrap_or(0) as u32,
        );
        let weak = Rc::downgrade(self);
        format_row.connect_selected_notify(move |row| {
            Self::saved(
                &weak,
                settings::save_format(Format::ALL[row.selected() as usize]),
            );
        });
        recording.add(&format_row);
        let name_row = adw::EntryRow::builder()
            .title(t("prefs.name"))
            .text(settings::load_your_name())
            .show_apply_button(true)
            .build();
        let weak = Rc::downgrade(self);
        name_row.connect_apply(move |row| {
            Self::saved(&weak, settings::save_your_name(&row.text()));
        });
        recording.add(&name_row);
        let meetings_path = settings::meetings_dir();
        let meetings_row = adw::ActionRow::builder()
            .title(t("prefs.meetings"))
            .subtitle(meetings_path.display().to_string())
            .build();
        let choose = gtk::Button::builder()
            .label(t("prefs.meetings_choose"))
            .valign(gtk::Align::Center)
            .build();
        meetings_row.add_suffix(&choose);
        let this = self.clone();
        let row = meetings_row.clone();
        choose.connect_clicked(move |_| {
            let folders = gtk::FileDialog::builder()
                .title(t("prefs.meetings"))
                .build();
            let (row, window, this) = (row.clone(), this.window.clone(), this.clone());
            folders.select_folder(Some(&window), gio::Cancellable::NONE, move |result| {
                if let Some(path) = result.ok().and_then(|f| f.path()) {
                    match settings::save_meetings_dir(&path) {
                        Ok(()) => row.set_subtitle(&path.display().to_string()),
                        Err(e) => this.toast(&momr_core::locales::tf(
                            "prefs.save_failed",
                            &[&e.to_string()],
                        )),
                    }
                }
            });
        });
        recording.add(&meetings_row);
        page.add(&recording);

        let timer_group = adw::PreferencesGroup::builder()
            .title(t("timer.title"))
            .description(t("prefs.timer_about"))
            .build();
        let minutes = adw::SpinRow::with_range(1.0, 1440.0, 5.0);
        minutes.set_title(t("prefs.timer_default"));
        minutes.set_value(f64::from(settings::load_timer_minutes()));
        let weak = Rc::downgrade(self);
        minutes.connect_value_notify(move |row| {
            Self::saved(
                &weak,
                settings::save_timer_minutes(row.value().round() as u32),
            );
        });
        timer_group.add(&minutes);
        page.add(&timer_group);
        page
    }

    fn prefs_audio(self: &Rc<Self>) -> adw::PreferencesPage {
        let page = Self::prefs_page(t("prefs.audio"), "audio-speakers-symbolic");
        let audio_status = match momr_core::helper::path() {
            Some(helper) => momr_core::helper::list_info(&helper),
            None => Err(t("banner.audio_helper_missing").to_owned()),
        };

        // The microphone: the system default, or one input by name.
        let mic_group = adw::PreferencesGroup::builder()
            .title(t("prefs.mic"))
            .build();
        let inputs = audio_status
            .as_ref()
            .map(|d| d.inputs.clone())
            .unwrap_or_default();
        let mut mic_names = vec![t("prefs.mic_default").to_owned()];
        mic_names.extend(inputs.iter().map(|d| d.name.clone()));
        let mic_name_refs: Vec<&str> = mic_names.iter().map(String::as_str).collect();
        let mic_row = adw::ComboRow::builder()
            .title(t("prefs.mic"))
            .model(&gtk::StringList::new(&mic_name_refs))
            .build();
        let chosen_mic = settings::load_mic_device();
        mic_row.set_selected(
            chosen_mic
                .as_deref()
                .and_then(|uid| inputs.iter().position(|d| d.uid == uid))
                .map_or(0, |i| i as u32 + 1),
        );
        mic_row.set_subtitle(&match &audio_status {
            Ok(_) if inputs.is_empty() => t("prefs.mic_none").to_owned(),
            Ok(_) => t("prefs.mic_hint").to_owned(),
            Err(e) => momr_core::locales::tf("prefs.devices_unknown", &[e]),
        });
        let weak = Rc::downgrade(self);
        let mic_inputs = inputs.clone();
        mic_row.connect_selected_notify(move |row| {
            let Some(r) = weak.upgrade() else { return };
            let uid = match row.selected() {
                0 => None,
                n => mic_inputs.get(n as usize - 1).map(|d| d.uid.as_str()),
            };
            Self::saved(&Rc::downgrade(&r), settings::save_mic_device(uid));
            r.mic.restart();
        });
        mic_group.add(&mic_row);
        page.add(&mic_group);

        // The computer audio: every app, or only the ones switched on below.
        let computer_group = adw::PreferencesGroup::builder()
            .title(t("prefs.computer"))
            .build();
        let scope_row = adw::ComboRow::builder()
            .title(t("prefs.computer_scope"))
            .subtitle(t("prefs.computer_scope_hint"))
            .model(&gtk::StringList::new(&[
                t("prefs.computer_all"),
                t("prefs.computer_chosen"),
            ]))
            .build();
        let saved_sources = settings::load_computer_sources();
        scope_row.set_selected(u32::from(!saved_sources.is_empty()));
        computer_group.add(&scope_row);
        let status_row = adw::ActionRow::builder()
            .title(t("prefs.computer_status"))
            .build();
        match &audio_status {
            Ok(devices) if devices.tap && devices.tap_denied => {
                status_row.set_subtitle(t("prefs.computer_tap_denied"));
                let open = gtk::Button::builder()
                    .label(t("prefs.open_privacy"))
                    .valign(gtk::Align::Center)
                    .build();
                open.connect_clicked(|_| {
                    let _ = gio::AppInfo::launch_default_for_uri(
                        "x-apple.systempreferences:com.apple.preference.security?Privacy_AudioCapture",
                        None::<&gio::AppLaunchContext>,
                    );
                });
                status_row.add_suffix(&open);
            }
            Ok(devices) if devices.tap => {
                status_row.set_subtitle(t("prefs.computer_tap"));
            }
            Ok(momr_core::helper::AudioDevices {
                blackhole: Some(device),
                ..
            }) => {
                status_row.set_subtitle(&momr_core::locales::tf(
                    "prefs.computer_blackhole",
                    &[device],
                ));
            }
            _ => {
                status_row.set_subtitle(t("prefs.computer_unavailable"));
                let install = gtk::Button::builder()
                    .label(t("prefs.blackhole_how"))
                    .valign(gtk::Align::Center)
                    .build();
                install.connect_clicked(|_| {
                    let _ = gio::AppInfo::launch_default_for_uri(
                        "https://github.com/ExistentialAudio/BlackHole",
                        None::<&gio::AppLaunchContext>,
                    );
                });
                status_row.add_suffix(&install);
            }
        }
        computer_group.add(&status_row);
        page.add(&computer_group);

        // One switch per app with audio, plus the saved ones that are not
        // running now, so a chosen app is not lost between calls.
        let apps_group = adw::PreferencesGroup::builder()
            .title(t("prefs.apps"))
            .description(t("prefs.apps_about"))
            .visible(!saved_sources.is_empty())
            .build();
        scope_row
            .bind_property("selected", &apps_group, "visible")
            .transform_to(|_, selected: u32| Some(selected == 1))
            .sync_create()
            .build();
        let mut processes = audio_status
            .as_ref()
            .map(|d| d.processes.clone())
            .unwrap_or_default();
        processes.sort_by_key(|a| a.name.to_lowercase());
        processes.dedup_by(|a, b| a.bundle == b.bundle);
        let mut offered: Vec<(String, String, String)> = processes
            .iter()
            .map(|p| {
                let subtitle = if p.playing {
                    format!("{} · {}", p.bundle, t("prefs.app_playing"))
                } else {
                    p.bundle.clone()
                };
                (p.bundle.clone(), p.name.clone(), subtitle)
            })
            .collect();
        for bundle in &saved_sources {
            if !offered.iter().any(|(b, _, _)| b == bundle) {
                offered.push((
                    bundle.clone(),
                    bundle.clone(),
                    t("prefs.app_not_running").to_owned(),
                ));
            }
        }
        if offered.is_empty() {
            apps_group.add(
                &adw::ActionRow::builder()
                    .title(t("prefs.apps_none"))
                    .build(),
            );
        }
        // The chosen bundles, kept in step with the switches; saved only
        // while the scope is "chosen apps", so "all apps" clears them.
        let chosen: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(saved_sources.clone()));
        let save_sources = {
            let weak = Rc::downgrade(self);
            let (chosen, scope_row) = (chosen.clone(), scope_row.clone());
            Rc::new(move || {
                let Some(r) = weak.upgrade() else { return };
                let list = if scope_row.selected() == 1 {
                    chosen.borrow().clone()
                } else {
                    Vec::new()
                };
                Self::saved(&Rc::downgrade(&r), settings::save_computer_sources(&list));
                r.system.restart();
            })
        };
        for (bundle, name, subtitle) in offered {
            let row = adw::SwitchRow::builder()
                .title(&name)
                .subtitle(&subtitle)
                .active(saved_sources.contains(&bundle))
                .build();
            let (chosen, save_sources) = (chosen.clone(), save_sources.clone());
            row.connect_active_notify(move |row| {
                let mut list = chosen.borrow_mut();
                list.retain(|b| *b != bundle);
                if row.is_active() {
                    list.push(bundle.clone());
                }
                drop(list);
                save_sources();
            });
            apps_group.add(&row);
        }
        let save_on_scope = save_sources.clone();
        scope_row.connect_selected_notify(move |_| save_on_scope());
        page.add(&apps_group);
        page
    }

    /// Storage: what the app has accumulated, with a button each and a full
    /// reset. Meetings and Keychain keys are never touched, and each
    /// confirmation says so.
    fn prefs_storage(self: &Rc<Self>, dialog: &adw::PreferencesDialog) -> adw::PreferencesPage {
        let page = Self::prefs_page(t("prefs.storage"), "drive-harddisk-symbolic");
        let group = adw::PreferencesGroup::builder()
            .title(t("prefs.storage"))
            .description(t("storage.about"))
            .build();
        let cache_dir = momr_platform::paths::cache();
        let models_dir = momr_core::transcribe::models_dir();
        let settings_files = [
            momr_platform::paths::settings_file(),
            momr_platform::paths::config_file(),
        ];

        let cache_row = adw::ActionRow::builder().title(t("storage.cache")).build();
        let models_row = adw::ActionRow::builder().title(t("storage.models")).build();
        let settings_row = adw::ActionRow::builder()
            .title(t("storage.settings"))
            .subtitle(t("storage.settings_hint"))
            .build();
        let refresh = {
            let (cache_row, models_row) = (cache_row.clone(), models_row.clone());
            let (cache_dir, models_dir) = (cache_dir.clone(), models_dir.clone());
            Rc::new(move || {
                let unfinished = unfinished_recordings().len();
                let mut subtitle = momr_core::cleanup::human(momr_core::cleanup::size(&cache_dir));
                if unfinished > 0 {
                    subtitle = format!(
                        "{subtitle} · {}",
                        tf("storage.unfinished", &[&unfinished.to_string()])
                    );
                }
                cache_row.set_subtitle(&subtitle);
                models_row.set_subtitle(&momr_core::cleanup::human(
                    momr_core::cleanup::models_size(&models_dir),
                ));
            })
        };
        refresh();

        // What the live app must keep: the recording in progress, the socket.
        let keep = {
            let weak = Rc::downgrade(self);
            move || -> Vec<PathBuf> {
                let mut keep = vec![momr_core::ipc::socket_path()];
                if let Some(r) = weak.upgrade()
                    && let Some(staging) = r.staging.borrow().clone()
                {
                    keep.push(staging);
                }
                keep
            }
        };
        let confirm = |this: &Rc<Self>, title: &str, body: &str, label: &str, run: Rc<dyn Fn()>| {
            let dialog = adw::AlertDialog::new(Some(title), Some(body));
            dialog.add_response("cancel", t("import.cancel"));
            dialog.add_response("go", label);
            dialog.set_response_appearance("go", adw::ResponseAppearance::Destructive);
            dialog.set_default_response(Some("cancel"));
            dialog.set_close_response("cancel");
            dialog.connect_response(Some("go"), move |_, _| run());
            dialog.present(Some(&this.window));
        };

        let clear_cache = {
            let weak = Rc::downgrade(self);
            let (cache_dir, keep, refresh) = (cache_dir.clone(), keep.clone(), refresh.clone());
            Rc::new(move || {
                let Some(r) = weak.upgrade() else { return };
                match momr_core::cleanup::clear_cache(&cache_dir, &keep()) {
                    Ok(cleared) => r.toast(&tf(
                        "storage.cleared",
                        &[&momr_core::cleanup::human(cleared.bytes)],
                    )),
                    Err(e) => r.toast(&tf("storage.failed", &[&e.to_string()])),
                }
                refresh();
            })
        };
        let clear_button = gtk::Button::builder()
            .label(t("storage.clear"))
            .valign(gtk::Align::Center)
            .build();
        let this = self.clone();
        let run = clear_cache.clone();
        clear_button.connect_clicked(move |_| {
            let unfinished = unfinished_recordings().len();
            let body = if unfinished > 0 {
                format!(
                    "{} {}",
                    t("storage.cache_body"),
                    tf("storage.cache_unfinished", &[&unfinished.to_string()])
                )
            } else {
                t("storage.cache_body").to_owned()
            };
            confirm(
                &this,
                t("storage.cache_title"),
                &body,
                t("storage.clear"),
                run.clone(),
            );
        });
        cache_row.add_suffix(&clear_button);
        group.add(&cache_row);

        let delete_models = {
            let weak = Rc::downgrade(self);
            let (models_dir, refresh) = (models_dir.clone(), refresh.clone());
            Rc::new(move || {
                let Some(r) = weak.upgrade() else { return };
                match momr_core::cleanup::delete_models(&models_dir) {
                    Ok(freed) => {
                        r.toast(&tf("storage.cleared", &[&momr_core::cleanup::human(freed)]))
                    }
                    Err(e) => r.toast(&tf("storage.failed", &[&e.to_string()])),
                }
                r.update_model_banner();
                refresh();
            })
        };
        let models_button = gtk::Button::builder()
            .label(t("storage.delete"))
            .valign(gtk::Align::Center)
            .build();
        let this = self.clone();
        let run = delete_models.clone();
        models_button.connect_clicked(move |_| {
            confirm(
                &this,
                t("storage.models_title"),
                t("storage.models_body"),
                t("storage.delete"),
                run.clone(),
            );
        });
        models_row.add_suffix(&models_button);
        group.add(&models_row);

        let reset_settings = {
            let weak = Rc::downgrade(self);
            let (files, dialog) = (settings_files.clone(), dialog.clone());
            Rc::new(move || {
                let Some(r) = weak.upgrade() else { return };
                match momr_core::cleanup::reset_settings(&files) {
                    Ok(()) => {
                        r.after_settings_reset();
                        r.toast(t("storage.settings_reset"));
                        // The rows still show the old values: reopen to see the defaults.
                        dialog.close();
                    }
                    Err(e) => r.toast(&tf("storage.failed", &[&e.to_string()])),
                }
            })
        };
        let settings_button = gtk::Button::builder()
            .label(t("storage.reset"))
            .valign(gtk::Align::Center)
            .build();
        let this = self.clone();
        let run = reset_settings.clone();
        settings_button.connect_clicked(move |_| {
            confirm(
                &this,
                t("storage.settings_title"),
                t("storage.settings_body"),
                t("storage.reset"),
                run.clone(),
            );
        });
        settings_row.add_suffix(&settings_button);
        group.add(&settings_row);
        page.add(&group);

        let everything = adw::PreferencesGroup::builder()
            .description(t("storage.everything_about"))
            .build();
        let reset_all = adw::ButtonRow::builder()
            .title(t("storage.everything"))
            .css_classes(["destructive-action"])
            .build();
        let this = self.clone();
        reset_all.connect_activated(move |_| {
            let (cache, models, settings) = (
                clear_cache.clone(),
                delete_models.clone(),
                reset_settings.clone(),
            );
            confirm(
                &this,
                t("storage.everything_title"),
                t("storage.everything_body"),
                t("storage.everything"),
                Rc::new(move || {
                    cache();
                    models();
                    settings();
                }),
            );
        });
        everything.add(&reset_all);
        page.add(&everything);
        page
    }

    /// After the settings files are gone: every live choice back to its
    /// default, without a relaunch where that is possible.
    fn after_settings_reset(&self) {
        crate::theme::apply_appearance(
            settings::Appearance::System,
            crate::theme::macos_prefers_dark,
        );
        self.settings_changed();
        // The capture children read their selection when they start.
        self.mic.restart();
        self.system.restart();
    }

    fn selected_format(&self) -> Format {
        Format::ALL
            .get(self.format_row.selected() as usize)
            .copied()
            .unwrap_or(Format::Mono)
    }

    fn selected_language(&self) -> &'static str {
        LANGUAGE_CODES
            .get(self.language_row.selected() as usize)
            .copied()
            .unwrap_or("auto")
    }

    fn title(&self) -> String {
        let typed = self.title_row.text().trim().to_owned();
        if typed.is_empty() {
            t("done.fallback_title").to_owned()
        } else {
            typed
        }
    }

    fn set_state(&self, state: State) {
        self.state.set(state);
        let mut shared = self.shared.lock().unwrap();
        shared.state = if state == State::Recording && self.paused.get() {
            "paused"
        } else {
            state.key()
        };
        shared.title = self.title_row.text().to_string();
        shared.started_at = self.started_at.get();
        shared.paused_secs = self.paused_secs.get();
        shared.pause_began = if self.paused.get() {
            self.pause_began.get()
        } else {
            0
        };
        if state != State::Transcribing {
            shared.progress = 0.0;
        }
        drop(shared);
        self.render();
    }

    fn render(&self) {
        let state = self.state.get();
        let recording = state == State::Recording;
        self.live.set(recording);
        self.dot.set_visible(recording);
        self.compact_button
            .set_visible(recording && !self.compact.get());
        self.strip_pause.set_icon_name(if self.paused.get() {
            "media-playback-start-symbolic"
        } else {
            "media-playback-pause-symbolic"
        });
        self.strip_pause
            .set_tooltip_text(Some(if self.paused.get() {
                t("ready.resume")
            } else {
                t("ready.pause")
            }));
        self.language_row
            .set_sensitive(!matches!(state, State::Stopping | State::Transcribing));
        self.button.set_sensitive(matches!(
            state,
            State::Idle | State::Recording | State::Done
        ));
        self.show_page();
        self.button.remove_css_class("suggested-action");
        self.button.remove_css_class("destructive-action");
        self.pause_button.set_visible(recording);
        self.import_button.set_visible(state == State::Idle);
        self.pause_button.set_label(if self.paused.get() {
            t("ready.resume")
        } else {
            t("ready.pause")
        });
        if recording {
            self.button.set_label(t("ready.stop"));
            self.button.add_css_class("destructive-action");
        } else {
            self.button.set_label(match state {
                State::Stopping => t("ready.button_saving"),
                State::Transcribing => t("ready.button_transcribing"),
                _ => t("ready.start"),
            });
            self.button.add_css_class("suggested-action");
        }

        let tracks_kept = self.result_dir.borrow().as_ref().is_some_and(|dir| {
            let (mic, computer) = export::tracks(dir);
            (mic.is_file() && computer.is_file()) || source_track(dir).is_file()
        });
        self.again_button.set_sensitive(tracks_kept);
        self.again_button.set_tooltip_text(Some(if tracks_kept {
            t("done.transcribe_again_hint")
        } else {
            t("done.transcribe_again_gone")
        }));

        self.status_label.set_label(&self.status_text());
        self.update_actions();
    }

    /// Shows the page for the current state, edge to edge while transcribing.
    /// The window keeps its size; only the compact strip changes it.
    fn show_page(&self) {
        if self.compact.get() {
            return;
        }
        let state = self.state.get();
        let page = match state {
            State::Stopping | State::Transcribing => "transcribing",
            State::Done => "done",
            _ => "record",
        };
        self.layout.set_visible_child_name(page);
        let immersive = matches!(state, State::Stopping | State::Transcribing);
        self.view.set_extend_content_to_top_edge(immersive);
        if immersive {
            self.window.add_css_class("immersive");
        } else {
            self.window.remove_css_class("immersive");
        }
        self.sidebar_toggle
            .set_visible(state == State::Done && self.split.is_collapsed());
    }

    /// Keeps the window size for the next launch: the current one, or the
    /// one the strip will restore while compact.
    fn remember_size(&self) {
        let (width, height) = if self.compact.get() {
            self.full_size.get()
        } else {
            self.window.default_size()
        };
        if width >= MIN_SIZE.0 && height >= MIN_SIZE.1 {
            let _ = settings::save_window_size(width, height);
        }
    }

    /// The line under the clock: what is happening, and what the timer will
    /// do about it.
    fn status_text(&self) -> String {
        let plan = self.timer_plan.borrow();
        match self.state.get() {
            State::Idle => match plan.start_at {
                Some(at) => tf(
                    "timer.status_scheduled",
                    &[&momr_core::timer::clock_time(at)],
                ),
                None => t("ready.status_idle").to_owned(),
            },
            State::Recording if self.paused.get() => t("ready.status_paused").to_owned(),
            State::Recording => match plan.remaining(momr_core::ipc::now(), self.elapsed()) {
                Some(left) => tf(
                    "timer.status_countdown",
                    &[&momr_core::timer::countdown(left)],
                ),
                None => t("ready.status_recording").to_owned(),
            },
            State::Stopping => t("ready.status_stopping").to_owned(),
            // Where the audio goes is a privacy question: say it.
            State::Transcribing => match momr_core::provider::configured() {
                Ok(Provider::Cloud(cloud)) => {
                    tf("ready.status_transcribing_cloud", &[cloud.name()])
                }
                _ => t("ready.status_transcribing").to_owned(),
            },
            State::Done => String::new(),
        }
    }

    fn tick(self: &Rc<Self>) {
        let now = momr_core::ipc::now();
        if self.state.get() == State::Recording {
            let elapsed = self.elapsed();
            let clock = format_elapsed(elapsed);
            let opacity = if self.paused.get() {
                0.35
            } else if elapsed % 2 == 0 {
                1.0
            } else {
                0.3
            };
            self.timer.set_label(&clock);
            self.compact_timer.set_label(&clock);
            self.dot.set_opacity(opacity);
            self.compact_dot.set_opacity(opacity);
            // The timer: stop when its time is up, count down before that,
            // and say so once a minute ahead.
            let plan = self.timer_plan.borrow().clone();
            if plan.due_stop(now, elapsed) {
                self.stop();
                return;
            }
            if let Some(left) = plan.remaining(now, elapsed) {
                self.status_label.set_label(&self.status_text());
                if left <= 60 && !self.timer_warned.replace(true) {
                    self.toast(t("timer.one_minute"));
                }
            }
        } else if self.state.get() == State::Idle && self.timer_plan.borrow().due_start(now) {
            self.start();
            return;
        }
        // Either source may fail or fall back (a refused permission, no
        // device, a tap refused and BlackHole used instead, a full disk);
        // say so under the meters until it captures again.
        let notes: Vec<String> = [self.mic.note(), self.system.note()]
            .into_iter()
            .flatten()
            .collect();
        match (!notes.is_empty()).then(|| notes.join(" ")) {
            Some(note) => {
                if self.audio_banner.title() != note {
                    self.audio_banner.set_title(&note);
                }
                self.audio_banner.set_revealed(true);
            }
            None => self.audio_banner.set_revealed(false),
        }
    }

    /// Switches between the full layout and the compact strip. The strip
    /// keeps the header bar: the clock becomes the title, Pause, Stop and
    /// Expand sit at its end, and the traffic lights stay where they are, so
    /// the title bar drags the strip the way it drags any window.
    fn set_compact(&self, compact: bool) {
        if compact == self.compact.get() || (compact && self.state.get() != State::Recording) {
            return;
        }
        self.compact.set(compact);
        if compact {
            let (w, h) = self.window.default_size();
            if w >= MIN_SIZE.0 && h >= MIN_SIZE.1 {
                self.full_size.set((w, h));
            }
            self.layout.set_visible_child_name("compact");
            self.header.set_title_widget(Some(&self.compact_title));
            self.strip_buttons.set_visible(true);
            self.gear.set_visible(false);
            self.sidebar_toggle.set_visible(false);
            // Not resizable: the window takes the strip's natural size.
            self.window.set_size_request(COMPACT_SIZE.0, COMPACT_SIZE.1);
            self.window.set_resizable(false);
            self.window.set_default_size(COMPACT_SIZE.0, COMPACT_SIZE.1);
        } else {
            self.header.set_title_widget(None::<&gtk::Widget>);
            self.strip_buttons.set_visible(false);
            self.gear.set_visible(true);
            self.window.set_resizable(true);
            self.window.set_size_request(MIN_SIZE.0, MIN_SIZE.1);
            let (w, h) = self.full_size.get();
            self.window.set_default_size(w, h);
            self.show_page();
        }
        self.window.queue_resize();
        self.render();
    }

    /// Recorded time so far, without the pauses.
    fn elapsed(&self) -> i64 {
        let until = if self.paused.get() {
            self.pause_began.get()
        } else {
            momr_core::ipc::now()
        };
        (until - self.started_at.get() - self.paused_secs.get()).max(0)
    }

    fn toggle_pause(self: &Rc<Self>) {
        if self.state.get() != State::Recording {
            return;
        }
        if self.paused.get() {
            self.paused_secs
                .set(self.paused_secs.get() + momr_core::ipc::now() - self.pause_began.get());
            self.paused.set(false);
        } else {
            self.pause_began.set(momr_core::ipc::now());
            self.paused.set(true);
        }
        self.mic.set_paused(self.paused.get());
        self.system.set_paused(self.paused.get());
        self.freeze_meters(self.paused.get());
        self.set_state(State::Recording);
        self.tick();
    }

    fn freeze_meters(&self, frozen: bool) {
        for (cell, source) in self.frozen.iter().zip([&self.mic, &self.system]) {
            *cell.borrow_mut() = frozen.then(|| source.levels());
        }
        for meter in self
            .meters
            .iter()
            .chain(std::iter::once(&self.compact_wave))
        {
            meter.queue_draw();
        }
    }

    /// Asks for the language and the number of speakers, then imports.
    fn confirm_import(self: &Rc<Self>, path: PathBuf) {
        if matches!(
            self.state.get(),
            State::Recording | State::Stopping | State::Transcribing
        ) {
            self.toast(t("help.finish_first"));
            return;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dialog = adw::AlertDialog::new(Some(t("import.dialog_title")), Some(&name));
        let group = adw::PreferencesGroup::new();
        let language_labels: Vec<&str> = LANGUAGE_CODES
            .iter()
            .map(|code| language_label(code))
            .collect();
        let language = adw::ComboRow::builder()
            .title(t("done.language_again"))
            .model(&gtk::StringList::new(&language_labels))
            .selected(self.language_row.selected())
            .build();
        let mut speaker_choices = vec![t("import.auto_speakers")];
        speaker_choices.extend(SPEAKER_NUMBERS);
        let speakers = adw::ComboRow::builder()
            .title(t("done.speakers"))
            .subtitle(t("import.subtitle_speakers"))
            .model(&gtk::StringList::new(&speaker_choices))
            .build();
        group.add(&language);
        group.add(&speakers);
        dialog.set_extra_child(Some(&group));
        dialog.add_response("cancel", t("import.cancel"));
        dialog.add_response("import", t("import.confirm"));
        dialog.set_response_appearance("import", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("import"));
        dialog.set_close_response("cancel");
        let this = self.clone();
        dialog.connect_response(None, move |_, response| {
            if response != "import" {
                return;
            }
            let code = LANGUAGE_CODES
                .get(language.selected() as usize)
                .copied()
                .unwrap_or("auto");
            let count = match speakers.selected() {
                0 => None,
                n => Some(n as usize),
            };
            this.import_file(path.clone(), code, count);
        });
        dialog.present(Some(&self.window));
    }

    /// Makes a meeting of an audio file: a folder with the levelled audio and
    /// the source kept for transcribing again, then the transcript.
    fn import_file(
        self: &Rc<Self>,
        path: PathBuf,
        language: &'static str,
        speakers: Option<usize>,
    ) {
        let title = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| t("done.imported_audio").to_owned());
        // The file's own date says when the meeting was, better than now.
        let started_at = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or_else(momr_core::ipc::now, |d| d.as_secs() as i64);
        let out = meeting::unused_folder_for(started_at, &title);

        self.player.unload();
        self.title_row.set_text(&title);
        self.started_at.set(started_at);
        *self.result_dir.borrow_mut() = Some(out.clone());
        *self.manifest.borrow_mut() = Some(Manifest {
            title: title.clone(),
            started_at,
            duration_secs: 0,
            format: Format::Mono,
            language: language.to_owned(),
            speakers: Vec::new(),
            imported: path.file_name().map(|n| n.to_string_lossy().into_owned()),
            speaker_count: speakers,
            model: None,
            provider: None,
            chapters: Vec::new(),
            chapters_by: None,
        });
        self.animation_since.set(Some(std::time::Instant::now()));
        self.animation.reset();
        self.animation.set_stage(t("import.importing"));
        self.animation.set_running(true);
        self.set_state(State::Stopping);

        let this = self.clone();
        glib::spawn_future_local(async move {
            let (source, target) = (path.clone(), out.clone());
            let staging =
                momr_platform::paths::cache().join(format!("import-{}", momr_core::ipc::now()));
            let converted = gio::spawn_blocking(move || import_audio(&source, &target, &staging))
                .await
                .unwrap_or_else(|_| Err(t("help.stopped_unexpectedly").into()));
            let result = match converted {
                Ok(duration) => {
                    if let Some(m) = this.manifest.borrow_mut().as_mut() {
                        m.duration_secs = duration;
                        let _ = meeting::write(&out, m);
                    }
                    this.run_transcription(Tracks::Single(source_track(&out), speakers), language)
                        .await
                }
                Err(message) => Err(message),
            };
            this.hold_animation(&result).await;
            this.finished(result.is_ok(), result);
        });
    }

    /// After a crash: offers to save recordings that were never stopped.
    fn offer_recovery(self: &Rc<Self>) {
        let current = self.staging.borrow().clone();
        let Some(staging) = unfinished_recordings()
            .into_iter()
            .find(|dir| Some(dir) != current.as_ref())
        else {
            return;
        };
        let note = finish::read_note(&staging);
        let started_at = note.as_ref().map_or(0, |n| n.started_at);
        let when = glib::DateTime::from_unix_local(started_at)
            .and_then(|t| t.format("%A %H:%M"))
            .map(|s| s.to_string())
            .unwrap_or_default();
        let length = format_elapsed(raw_duration(&staging));
        let dialog = adw::AlertDialog::new(
            Some(t("done.recovery_title")),
            Some(
                &t("done.recovery_body")
                    .replacen("{}", &when, 1)
                    .replacen("{}", &length, 1),
            ),
        );
        dialog.add_response("discard", t("done.recovery_discard"));
        dialog.add_response("later", t("done.recovery_later"));
        dialog.add_response("save", t("done.recovery_save"));
        dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
        dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("save"));
        dialog.set_close_response("later");
        let this = self.clone();
        dialog.connect_response(None, move |_, response| match response {
            "discard" => {
                let _ = std::fs::remove_dir_all(&staging);
            }
            "save" => this.recover(staging.clone(), note.clone()),
            _ => {}
        });
        dialog.present(Some(&self.window));
    }

    /// Picks an unfinished recording up where it stopped and finishes it like Stop does.
    fn recover(self: &Rc<Self>, staging: PathBuf, note: Option<RecordingNote>) {
        if self.state.get() != State::Idle {
            self.toast(t("help.finish_first"));
            return;
        }
        let note = note.unwrap_or_else(|| RecordingNote {
            title: t("done.recovered_title").to_owned(),
            started_at: momr_core::ipc::now() - raw_duration(&staging),
            format: None,
            language: None,
        });
        self.title_row.set_text(&note.title);
        let (format, language) = (note.format(), note.language());
        if let Some(i) = Format::ALL.iter().position(|f| *f == format) {
            self.format_row.set_selected(i as u32);
        }
        if let Some(i) = LANGUAGE_CODES.iter().position(|code| **code == language) {
            self.language_row.set_selected(i as u32);
        }
        self.started_at.set(note.started_at);
        self.paused.set(false);
        self.paused_secs.set(0);
        *self.staging.borrow_mut() = Some(staging);
        *self.result_dir.borrow_mut() = None;
        // Straight into what Stop does from here.
        self.set_state(State::Recording);
        self.stop();
    }

    /// Says so when the speech model is not on disk yet, with a button to get it.
    fn update_model_banner(&self) {
        if self.model_downloading.get() {
            return;
        }
        match momr_core::models::missing() {
            Some((name, size_mb)) => {
                let size = if size_mb >= 1000 {
                    format!("{:.1} GB", f64::from(size_mb) / 1000.0)
                } else {
                    format!("{size_mb} MB")
                };
                self.model_banner.set_title(
                    &t("ready.model_needed")
                        .replacen("{}", &name, 1)
                        .replacen("{}", &size, 1),
                );
                self.model_banner
                    .set_button_label(Some(t("ready.model_download")));
                self.model_banner.set_revealed(true);
            }
            None => self.model_banner.set_revealed(false),
        }
    }

    /// Fetches the speech model in the background; recording can go on.
    fn download_model(self: &Rc<Self>) {
        if self.model_downloading.replace(true) {
            return;
        }
        self.model_banner.set_button_label(None);
        self.model_banner.set_title(t("ready.model_downloading"));
        let (events_tx, events_rx) = async_channel::unbounded::<Event>();
        let (done_tx, done_rx) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let result = momr_core::models::ensure(&events_tx, &Abort::default());
            let _ = done_tx.send_blocking(result);
            let _ = events_tx.send_blocking(Event::Finished);
        });
        let this = self.clone();
        glib::spawn_future_local(async move {
            while let Ok(event) = events_rx.recv().await {
                match event {
                    Event::Progress(progress) => {
                        this.model_banner
                            .set_title(&t("ready.model_progress").replacen(
                                "{:.0}",
                                &format!("{:.0}", progress * 100.0),
                                1,
                            ))
                    }
                    Event::Finished => break,
                    _ => {}
                }
            }
            let result = done_rx
                .recv()
                .await
                .unwrap_or_else(|_| Err(t("help.download_stopped").into()));
            this.model_downloading.set(false);
            match result {
                Ok(_) => this.toast(t("help.model_ready")),
                Err(message) => {
                    this.toast(&t("help.model_download_failed").replace("{}", &message))
                }
            }
            this.update_model_banner();
        });
    }

    /// Back to the recording page, ready for the next meeting.
    fn ready(&self) {
        self.player.unload();
        *self.result_dir.borrow_mut() = None;
        *self.manifest.borrow_mut() = None;
        self.title_row.set_text("");
        self.timer.set_label("00:00");
        self.compact_timer.set_label("00:00");
        self.set_state(State::Idle);
        self.update_model_banner();
    }

    fn start(self: &Rc<Self>) {
        // A name typed before starting is kept; otherwise one from the time.
        if self.title_row.text().trim().is_empty() {
            let title = glib::DateTime::now_local()
                .and_then(|now| now.format("%H:%M"))
                .map(|time| format!("{} {time}", t("done.fallback_title")))
                .unwrap_or_else(|_| t("done.fallback_title").to_owned());
            self.title_row.set_text(&title);
        }
        self.paused.set(false);
        self.paused_secs.set(0);
        self.pause_began.set(0);
        let started_at = momr_core::ipc::now();
        let staging = momr_platform::paths::cache().join(started_at.to_string());
        if let Err(e) = std::fs::create_dir_all(&staging)
            .and_then(|_| self.mic.start_recording(&staging.join("mic.raw")))
            .and_then(|_| self.system.start_recording(&staging.join("system.raw")))
        {
            let _ = self.mic.stop_recording();
            let _ = self.system.stop_recording();
            self.status_label
                .set_label(&t("help.could_not_start").replace("{}", &e.to_string()));
            return;
        }
        write_recording_note(
            &staging,
            &self.title(),
            started_at,
            self.selected_format(),
            self.selected_language(),
        );
        *self.staging.borrow_mut() = Some(staging);
        *self.result_dir.borrow_mut() = None;
        self.player.unload();
        // A scheduled start has happened, or been overtaken by hand; the
        // stop limits stay.
        let mut plan = self.timer_plan.borrow().clone();
        plan.start_at = None;
        self.set_timer(plan);
        self.started_at.set(started_at);
        self.timer.set_label("00:00");
        self.compact_timer.set_label("00:00");
        self.set_state(State::Recording);
        // Keep the Mac from idle-sleeping while recording. `-w` ties the
        // assertion to this process, so it dies with the app even on a crash.
        match std::process::Command::new("caffeinate")
            .args(["-i", "-w", &std::process::id().to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(child) => *self.caffeinate.borrow_mut() = Some(child),
            // Recording still works; only idle sleep is no longer held off.
            Err(e) => eprintln!("{APP_NAME}: caffeinate: {e}"),
        }
    }

    fn stop(self: &Rc<Self>) {
        if self.state.get() != State::Recording {
            return;
        }
        if self.paused.get() {
            self.paused_secs
                .set(self.paused_secs.get() + momr_core::ipc::now() - self.pause_began.get());
            self.paused.set(false);
        }
        self.freeze_meters(false);
        self.animation_since.set(Some(std::time::Instant::now()));
        // A write that failed mid-recording (a full disk) lost audio; say so
        // now, since the meeting is saved from what did reach the disk.
        let length = self.elapsed();
        let lost = [self.mic.stop_recording(), self.system.stop_recording()];
        if let Some(e) = lost.iter().flatten().next() {
            self.toast(&tf("banner.audio_write_failed", &[e]));
        } else if length >= SILENT_HINT_SECS
            && !self.system.heard_anything()
            && !self.silent_hint_shown.replace(true)
        {
            // A refused tap records digital silence, which no check can tell
            // from a Mac that played nothing; ask, once per launch.
            self.toast(t("banner.computer_silent"));
        }
        if let Some(mut caffeinate) = self.caffeinate.borrow_mut().take() {
            let _ = caffeinate.kill();
            let _ = caffeinate.wait();
        }
        // The timer did its job, or was overtaken by hand: one plan per recording.
        self.set_timer(momr_core::timer::Plan::default());
        let Some(staging) = self.staging.borrow().clone() else {
            return;
        };
        self.set_compact(false);
        // Straight to the animation: the waves would suggest it is still recording.
        self.animation.reset();
        self.animation.set_stage(t("help.stages_saving"));
        self.animation.set_running(true);
        self.set_state(State::Stopping);

        let format = self.selected_format();
        let language = self.selected_language();
        let out = meeting::unused_folder_for(self.started_at.get(), &self.title());
        let this = self.clone();
        glib::spawn_future_local(async move {
            let (audio_out, audio_staging) = (out.clone(), staging.clone());
            let saved = gio::spawn_blocking(move || {
                let _ = std::fs::create_dir_all(&audio_out);
                let (mic, system) = (
                    audio_staging.join("mic.raw"),
                    audio_staging.join("system.raw"),
                );
                let audio = export_audio(&mic, &system, &audio_out, format);
                let tracks = export_tracks(&mic, &system, &audio_out);
                (audio, tracks)
            })
            .await
            .unwrap_or((false, false));
            let manifest = Manifest {
                title: this.title(),
                started_at: this.started_at.get(),
                duration_secs: raw_duration(&staging),
                format,
                language: language.to_owned(),
                speakers: vec![
                    settings::load_your_name(),
                    meeting::default_remote().to_owned(),
                ],
                imported: None,
                speaker_count: None,
                model: None,
                provider: None,
                chapters: Vec::new(),
                chapters_by: None,
            };
            let _ = meeting::write(&out, &manifest);
            *this.manifest.borrow_mut() = Some(manifest);
            *this.result_dir.borrow_mut() = Some(out);

            let result = this
                .run_transcription(Tracks::Raw(staging.clone()), language)
                .await;
            // The kept tracks are enough to transcribe again; the raw files can go.
            if saved == (true, true) {
                let _ = std::fs::remove_dir_all(&staging);
            }
            this.hold_animation(&result).await;
            this.finished(saved.0, result);
        });
    }

    /// Transcribes into `result_dir/transcript.md`, driving the animation.
    async fn run_transcription(
        &self,
        tracks: Tracks,
        language: &'static str,
    ) -> Result<(), String> {
        let Some(out) = self.result_dir.borrow().clone() else {
            return Err(t("help.no_meeting_folder").into());
        };
        self.set_compact(false);
        if self.animation_since.get().is_none() {
            self.animation_since.set(Some(std::time::Instant::now()));
        }
        self.set_state(State::Transcribing);
        self.animation.reset();
        self.animation
            .set_stage(momr_core::locales::t("stage.loading_audio"));
        self.animation.set_progress(0.0);
        self.animation.set_running(true);

        // Read once per run: the thread and the manifest must agree on it.
        let provider = momr_core::provider::configured()?;
        let abort = Abort::default();
        *self.abort.borrow_mut() = Some(abort.clone());
        let (events_tx, events_rx) = async_channel::unbounded::<Event>();
        let (done_tx, done_rx) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let result = match tracks {
                Tracks::Single(path, speakers) => transcribe::load_track(&path).and_then(|track| {
                    transcribe::transcribe_single(
                        &track, language, speakers, provider, &events_tx, &abort,
                    )
                }),
                Tracks::Raw(dir) | Tracks::Kept(dir) => {
                    let (mic_path, computer_path) = if dir.join("mic.raw").exists() {
                        (dir.join("mic.raw"), dir.join("system.raw"))
                    } else {
                        export::tracks(&dir)
                    };
                    transcribe::load_track(&mic_path).and_then(|mic| {
                        let computer = transcribe::load_track(&computer_path)?;
                        transcribe::transcribe(
                            &mic, &computer, language, provider, &events_tx, &abort,
                        )
                    })
                }
            };
            let _ = done_tx.send_blocking(result);
            let _ = events_tx.send_blocking(Event::Finished);
        });

        while let Ok(event) = events_rx.recv().await {
            match event {
                Event::Stage(stage) => self.animation.set_stage(&stage),
                Event::Progress(progress) => {
                    self.animation.set_progress(progress);
                    self.shared.lock().unwrap().progress = progress;
                }
                Event::Segment(text) => {
                    // The transcription labels speakers You/Remote or Speaker N;
                    // show this meeting's names.
                    let text = match (self.manifest.borrow().as_ref(), text.split_once(": ")) {
                        (Some(m), Some((label, rest))) => {
                            let labels = m.default_labels();
                            match labels.iter().position(|l| l == label) {
                                Some(i) if i < m.speakers.len() => {
                                    format!("{}: {rest}", m.speakers[i])
                                }
                                _ => text.clone(),
                            }
                        }
                        _ => text.clone(),
                    };
                    self.animation.push_text(&text);
                }
                Event::Finished => break,
            }
        }
        let result = done_rx
            .recv()
            .await
            .unwrap_or_else(|_| Err(t("help.transcription_stopped").into()));
        *self.abort.borrow_mut() = None;

        let transcript = result?;
        // The name as it is now; it may have been edited while transcribing.
        let out = self.result_dir.borrow().clone().unwrap_or(out);
        let date = meeting::date_line(self.started_at.get());
        let mut markdown = transcribe::to_markdown(&self.title(), &date, &transcript);
        if let Some(manifest) = self.manifest.borrow_mut().as_mut() {
            markdown = meeting::fit_speakers(manifest, &markdown);
            manifest.language = language.to_owned();
            // The model only means something for a local transcript.
            manifest.model = (provider == Provider::Local).then(momr_core::models::configured);
            manifest.provider = Some(provider.id().to_owned());
            manifest.title = self.title();
            // Chapters of a previous transcript would point at lines that are gone.
            manifest.chapters.clear();
            manifest.chapters_by = None;
            let _ = meeting::write(&out, manifest);
        }
        std::fs::write(out.join("transcript.md"), markdown)
            .map_err(|e| momr_core::locales::t("help.write_failed").replace("{}", &e.to_string()))
    }

    /// Keeps the animation on screen for at least ten seconds, also after a
    /// short recording, so it reads as a step rather than a flicker. Skipped
    /// when it was cancelled or the window is already gone.
    async fn hold_animation(&self, result: &Result<(), String>) {
        const MINIMUM: Duration = Duration::from_secs(10);
        if let Some(since) = self.animation_since.take() {
            let shown = since.elapsed();
            let cancelled = matches!(result, Err(message) if message == CANCELLED);
            if shown < MINIMUM && !cancelled && self.window.is_visible() {
                self.animation.set_progress(1.0);
                self.animation.set_stage(t("help.stages_done"));
                glib::timeout_future(MINIMUM - shown).await;
            }
        }
        self.animation.set_running(false);
    }

    fn finished(self: &Rc<Self>, audio_ok: bool, transcript: Result<(), String>) {
        self.done_title_row.set_text(&self.title_row.text());
        self.set_state(State::Done);
        // Follow a name that was edited while the transcription ran.
        self.apply_title();

        let problem = match (&transcript, audio_ok) {
            (Err(message), _) if message == CANCELLED => Some(t("help.cancelled").to_owned()),
            (Err(message), _) => Some(t("help.failed").replace("{}", message)),
            (Ok(()), false) => Some(t("help.no_audio").to_owned()),
            _ => None,
        };
        let text = self
            .result_dir
            .borrow()
            .as_ref()
            .and_then(|dir| std::fs::read_to_string(dir.join("transcript.md")).ok());
        if let Some(dir) = self.result_dir.borrow().as_ref() {
            self.player.load(dir);
        }
        self.show_transcript(text.as_deref(), problem.as_deref());
        self.transcript_scroll.vadjustment().set_value(0.0);
        let ok = problem.is_none();
        if let Some(problem) = problem {
            eprintln!("{APP_NAME}: {problem}");
        } else {
            self.window.set_default_widget(Some(&self.copy_button));
            self.copy_button.grab_focus();
            // A fresh transcript gets chapters when an agent is around.
            if self.can_have_chapters() {
                self.generate_chapters();
            }
        }
        // Finished while the window was elsewhere: say so. Only fires from
        // the .app (plan 08); from a terminal it is a no-op.
        if ok && !self.window.is_active() {
            let note = gio::Notification::new(t("notify.transcribed"));
            note.set_body(Some(&self.title()));
            if let Some(app) = self.window.application() {
                app.send_notification(Some("transcribed"), &note);
            }
        }
        if self.quit_when_done.get()
            && let Some(app) = self.window.application()
        {
            app.quit();
        }
    }
    fn open_meeting(self: &Rc<Self>, path: &std::path::Path) {
        if matches!(
            self.state.get(),
            State::Recording | State::Stopping | State::Transcribing
        ) {
            self.toast(t("help.finish_first"));
            return;
        }
        let Some((dir, manifest)) = meeting::open(path) else {
            self.toast(t("import.not_openable"));
            return;
        };
        // Folders from before manifests existed get one now.
        if meeting::find(&dir).is_none() {
            let _ = meeting::write(&dir, &manifest);
        }

        self.loading.set(true);
        if let Some(i) = Format::ALL.iter().position(|f| *f == manifest.format) {
            self.format_row.set_selected(i as u32);
        }
        if let Some(i) = LANGUAGE_CODES
            .iter()
            .position(|code| **code == manifest.language)
        {
            self.language_row.set_selected(i as u32);
        }
        self.loading.set(false);

        self.started_at.set(manifest.started_at);
        self.title_row.set_text(&manifest.title);
        self.done_title_row.set_text(&manifest.title);
        *self.manifest.borrow_mut() = Some(manifest);
        *self.result_dir.borrow_mut() = Some(dir.clone());
        self.set_state(State::Done);
        let text = std::fs::read_to_string(dir.join("transcript.md")).ok();
        let problem = text.is_none().then_some(t("help.no_transcript_yet"));
        self.player.load(&dir);
        self.show_transcript(text.as_deref(), problem);
        self.transcript_scroll.vadjustment().set_value(0.0);
        // Never leave the focus in the name: typing would rename the meeting.
        self.window.set_default_widget(Some(&self.copy_button));
        self.copy_button.grab_focus();
    }

    /// Fills the done page from transcript.md: the meta line and the readable transcript.
    fn show_transcript(self: &Rc<Self>, markdown: Option<&str>, problem: Option<&str>) {
        let ok = problem.is_none() && markdown.is_some();
        self.done_heading.set_label(if ok {
            t("done.transcript_ready")
        } else {
            t("done.meeting_saved")
        });
        self.done_icon.set_icon_name(Some(if ok {
            "object-select-symbolic"
        } else {
            "dialog-warning-symbolic"
        }));
        self.copy_button.set_sensitive(markdown.is_some());

        while let Some(row) = self.transcript_list.row_at_index(0) {
            self.transcript_list.remove(&row);
        }
        let mut meta = Vec::new();
        let mut segments = Vec::new();
        let mut notes = Vec::new();
        self.current_line.set(-1);
        if let Some(problem) = problem {
            notes.push(problem.to_owned());
        }
        let mut paragraphs = Paragraphs::default();
        for (index, line) in markdown.unwrap_or("").lines().enumerate() {
            if let Some(value) = line.strip_prefix("- **Duration:** ") {
                meta.push(value.trim().to_owned());
            } else if let Some(value) = line.strip_prefix("- **Language:** ") {
                // Nothing was said, so nothing was detected.
                if !value.trim().eq_ignore_ascii_case("unknown") {
                    meta.push(value.trim().to_owned());
                }
            } else if let Some((time, speaker, text)) = parse_segment(line) {
                paragraphs.add(time, speaker, text, index);
            } else if let Some(note) = line.strip_prefix('_').and_then(|l| l.strip_suffix('_')) {
                notes.push(note.to_owned());
            }
        }
        // Colours follow the speaker order of the meeting, then first appearance.
        let mut order: Vec<String> = self
            .manifest
            .borrow()
            .as_ref()
            .map(|m| m.speakers.clone())
            .unwrap_or_default();
        for paragraph in &paragraphs.list {
            if !order.contains(&paragraph.speaker) {
                order.push(paragraph.speaker.clone());
            }
        }
        let chapters = self
            .manifest
            .borrow()
            .as_ref()
            .map(|m| m.chapters.clone())
            .unwrap_or_default();
        for note in &notes {
            self.transcript_list.append(&note_row(note));
        }
        // The speaker column is as wide as the longest name.
        let speaker_width = paragraphs
            .list
            .iter()
            .map(|p| text_width(&self.transcript_list, &p.speaker))
            .max()
            .unwrap_or(0);
        let long = paragraphs
            .list
            .last()
            .is_some_and(|p| p.start_ms >= 3_600_000);
        let time_width = text_width(
            &self.transcript_list,
            if long { "0:00:00" } else { "00:00" },
        );
        let mut next_chapter = chapters.iter().peekable();
        let mut lines = Vec::new();
        for paragraph in paragraphs.list {
            while let Some(chapter) = next_chapter.next_if(|c| c.start_ms <= paragraph.start_ms) {
                let row = chapter_row(&chapter.title, chapter.start_ms);
                segments.push((row_count(&self.transcript_list), chapter.start_ms));
                self.transcript_list.append(&row);
            }
            let index = order
                .iter()
                .position(|s| *s == paragraph.speaker)
                .unwrap_or(0);
            let row = self.paragraph_row(&paragraph, index, time_width, speaker_width);
            segments.push((row_count(&self.transcript_list), paragraph.start_ms));
            self.transcript_list.append(&row);
            lines.push((paragraph.start_ms, paragraph.speaker, paragraph.text));
        }
        if let Some(manifest) = self.manifest.borrow().as_ref() {
            meta.push(manifest.format.short_label().to_owned());
        }
        *self.segments.borrow_mut() = segments;
        *self.lines.borrow_mut() = lines;
        self.player.set_chapters(
            chapters
                .iter()
                .map(|c| (c.start_ms, c.title.clone()))
                .collect(),
        );
        self.show_chapters(&chapters);
        self.show_speakers();
        let meta = meta.join("  ·  ");
        self.done_meta.set_label(&meta);
        self.done_meta.set_visible(!meta.is_empty());
    }

    fn default_agent(&self) -> Option<Agent> {
        self.agent.get_or_init(agent::default_agent).clone()
    }

    /// Chapters need an agent and a meeting long enough to divide.
    fn can_have_chapters(&self) -> bool {
        let long_enough = self
            .lines
            .borrow()
            .last()
            .is_some_and(|(start, _, _)| *start >= chapters::MIN_DURATION_MS);
        long_enough && self.default_agent().is_some()
    }

    /// Fills the chapters list and its header. The list shows whatever
    /// chapters the meeting has; the button only exists with an agent.
    fn show_chapters(&self, list: &[Chapter]) {
        while let Some(row) = self.chapters_list.row_at_index(0) {
            self.chapters_list.remove(&row);
        }
        for chapter in list {
            let row = adw::ActionRow::builder()
                .title(&chapter.title)
                .title_lines(2)
                .activatable(true)
                .build();
            row.add_prefix(
                &gtk::Label::builder()
                    .label(chapters::clock(chapter.start_ms))
                    .css_classes(["numeric", "dim-label"])
                    .build(),
            );
            self.chapters_list.append(&row);
        }
        *self.chapter_starts.borrow_mut() = list.iter().map(|c| c.start_ms).collect();
        self.update_chapters_header(!list.is_empty());
    }

    fn update_chapters_header(&self, has_chapters: bool) {
        let agent = self.default_agent();
        self.chapters_group
            .set_visible(agent.is_some() || has_chapters);
        self.chapters_button.set_visible(agent.is_some());
        self.chapters_spinner.set_visible(self.generating.get());
        self.chapters_button
            .set_sensitive(!self.generating.get() && self.can_have_chapters());
        self.chapters_button.set_label(if has_chapters {
            t("chapters.redo")
        } else {
            t("chapters.generate")
        });
        let description = match &agent {
            Some(agent) if self.generating.get() => {
                t("done.chapters_writing").replace("{}", agent.name())
            }
            Some(agent) if has_chapters => t("chapters.made_with").replace("{}", agent.name()),
            Some(agent) if self.can_have_chapters() => {
                t("chapters.let_divide").replace("{}", agent.name())
            }
            Some(_) => t("done.chapters_hint").to_owned(),
            None => String::new(),
        };
        self.chapters_group
            .set_description((!description.is_empty()).then_some(description.as_str()));
    }

    /// Asks the default agent for chapters in the background. The meeting
    /// works the same without them; a failure only shows up in the header.
    fn generate_chapters(self: &Rc<Self>) {
        if self.generating.get() {
            return;
        }
        let (Some(agent), Some(dir)) = (self.default_agent(), self.result_dir.borrow().clone())
        else {
            return;
        };
        let lines = self.lines.borrow().clone();
        self.generating.set(true);
        self.update_chapters_header(!self.chapter_starts.borrow().is_empty());
        let this = self.clone();
        glib::spawn_future_local(async move {
            let asked = agent.clone();
            let result = gio::spawn_blocking(move || {
                let lines: Vec<chapters::Line> = lines
                    .iter()
                    .map(|(start_ms, speaker, text)| chapters::Line {
                        start_ms: *start_ms,
                        speaker,
                        text,
                    })
                    .collect();
                chapters::generate(&asked, &lines)
            })
            .await
            .unwrap_or_else(|_| Err(t("help.agent_stopped").into()));
            this.generating.set(false);
            // The folder may have been renamed meanwhile (same timestamp
            // prefix); a different meeting is left alone.
            let prefix = |p: &std::path::Path| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.chars().take(12).collect::<String>())
            };
            let current = this.result_dir.borrow().clone();
            let Some(dir) = current.filter(|d| prefix(d) == prefix(&dir)) else {
                return;
            };
            match result {
                Ok(list) => this.store_chapters(&dir, &list, &agent),
                Err(message) => {
                    eprintln!("{APP_NAME}: chapters: {message}");
                    this.update_chapters_header(!this.chapter_starts.borrow().is_empty());
                    this.chapters_group.set_description(Some(
                        &t("chapters.could_not").replace("{}", agent.name()),
                    ));
                }
            }
        });
    }

    fn store_chapters(self: &Rc<Self>, dir: &std::path::Path, list: &[Chapter], agent: &Agent) {
        if let Some(manifest) = self.manifest.borrow_mut().as_mut() {
            manifest.chapters = list.to_vec();
            manifest.chapters_by = Some(agent.id().to_owned());
            let _ = meeting::write(dir, manifest);
        }
        let transcript = dir.join("transcript.md");
        if let Ok(text) = std::fs::read_to_string(&transcript) {
            let _ = std::fs::write(&transcript, chapters::apply_to_markdown(&text, list));
        }
        let text = std::fs::read_to_string(&transcript).ok();
        self.show_transcript(text.as_deref(), None);
        self.toast(&t("done.chapters_added").replace("{}", &list.len().to_string()));
    }

    /// One paragraph of the transcript: time, speaker and text in columns, with
    /// edit, swap-speaker and delete buttons that show on hover.
    fn paragraph_row(
        self: &Rc<Self>,
        paragraph: &Paragraph,
        speaker_index: usize,
        time_width: i32,
        speaker_width: i32,
    ) -> gtk::ListBoxRow {
        let time = gtk::Label::builder()
            .label(&paragraph.time)
            .xalign(0.0)
            .valign(gtk::Align::Start)
            .width_request(time_width)
            .css_classes(["numeric", "dim-label"])
            .build();
        let speaker = gtk::Label::builder()
            .label(&paragraph.speaker)
            .xalign(0.0)
            .valign(gtk::Align::Start)
            .width_request(speaker_width)
            .css_classes([format!("speaker-{}", speaker_index % 6)])
            .build();
        let text = gtk::Label::builder()
            .label(&paragraph.text)
            .xalign(0.0)
            .valign(gtk::Align::Start)
            .hexpand(true)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .natural_wrap_mode(gtk::NaturalWrapMode::None)
            .build();
        let action = |icon: &str, tooltip: &str| {
            gtk::Button::builder()
                .icon_name(icon)
                .tooltip_text(tooltip)
                .valign(gtk::Align::Start)
                .css_classes(["flat", "circular"])
                .build()
        };
        let edit = action("document-edit-symbolic", t("row.edit"));
        let swap = action("object-flip-horizontal-symbolic", t("row.next_speaker"));
        let delete = action("user-trash-symbolic", t("row.delete"));
        let actions = gtk::Box::builder()
            .spacing(2)
            .valign(gtk::Align::Start)
            .css_classes(["row-actions"])
            .build();
        actions.append(&edit);
        actions.append(&swap);
        actions.append(&delete);

        let content = gtk::Box::builder().spacing(14).build();
        content.append(&time);
        content.append(&speaker);
        content.append(&text);
        content.append(&actions);
        let row = gtk::ListBoxRow::builder()
            .child(&content)
            .tooltip_text(t("row.play_from").replace("{}", &paragraph.time))
            .build();
        row.set_cursor_from_name(Some("pointer"));

        let (sources, stamp, who) = (
            paragraph.sources.clone(),
            paragraph.time.clone(),
            paragraph.speaker.clone(),
        );
        let weak = Rc::downgrade(self);
        let delete_sources = sources.clone();
        delete.connect_clicked(move |_| {
            if let Some(r) = weak.upgrade() {
                r.delete_paragraph(&delete_sources);
            }
        });
        let weak = Rc::downgrade(self);
        let (swap_sources, swap_who) = (sources.clone(), who.clone());
        swap.connect_clicked(move |_| {
            if let Some(r) = weak.upgrade() {
                r.swap_speaker(&swap_sources, &swap_who);
            }
        });
        let weak = Rc::downgrade(self);
        let original = paragraph.text.clone();
        let (edit_content, edit_speaker, edit_row) =
            (content.clone(), speaker.clone(), row.clone());
        edit.connect_clicked(move |button| {
            let Some(r) = weak.upgrade() else { return };
            r.start_editing(
                &edit_row,
                &edit_content,
                &edit_speaker,
                &text,
                button,
                &sources,
                &stamp,
                &who,
                &original,
            );
        });
        row
    }

    /// Swaps the text of a row for an editor: Enter saves, Escape cancels,
    /// leaving the field saves too.
    #[allow(clippy::too_many_arguments)]
    fn start_editing(
        self: &Rc<Self>,
        row: &gtk::ListBoxRow,
        content: &gtk::Box,
        speaker: &gtk::Label,
        label: &gtk::Label,
        edit_button: &gtk::Button,
        sources: &[usize],
        time: &str,
        who: &str,
        original: &str,
    ) {
        row.set_activatable(false);
        row.add_css_class("editing");
        // Start at the size of the text it replaces: without a width the first
        // frame wraps every letter and the field flashes up very tall.
        let (width, height) = (label.width().max(120), label.height().max(1));
        label.set_visible(false);
        edit_button.set_sensitive(false);
        let editor = gtk::TextView::builder()
            .wrap_mode(gtk::WrapMode::WordChar)
            .hexpand(true)
            .valign(gtk::Align::Start)
            .width_request(width)
            .height_request(height)
            .css_classes(["transcript-editor"])
            .build();
        editor.buffer().set_text(original);
        content.insert_child_after(&editor, Some(speaker));
        editor.grab_focus();

        let done = Rc::new(Cell::new(false));
        let finish = {
            let (weak, editor, label, row, button) = (
                Rc::downgrade(self),
                editor.clone(),
                label.clone(),
                row.clone(),
                edit_button.clone(),
            );
            let (sources, time, who, original) = (
                sources.to_vec(),
                time.to_owned(),
                who.to_owned(),
                original.to_owned(),
            );
            let done = done.clone();
            move |save: bool| {
                if done.replace(true) {
                    return;
                }
                let buffer = editor.buffer();
                let text = buffer
                    .text(&buffer.start_iter(), &buffer.end_iter(), false)
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                if let Some(parent) = editor.parent().and_downcast::<gtk::Box>() {
                    parent.remove(&editor);
                }
                label.set_visible(true);
                row.set_activatable(true);
                row.remove_css_class("editing");
                button.set_sensitive(true);
                if save
                    && text != original
                    && let Some(r) = weak.upgrade()
                {
                    // Deferred: saving rebuilds the list this row lives in.
                    let (sources, time, who) = (sources.clone(), time.clone(), who.clone());
                    glib::idle_add_local_once(move || {
                        if text.is_empty() {
                            r.delete_paragraph(&sources);
                        } else {
                            r.edit_paragraph(&sources, &time, &who, &text);
                        }
                    });
                }
            }
        };
        let finish = Rc::new(finish);
        let keys = gtk::EventControllerKey::new();
        let on_key = finish.clone();
        keys.connect_key_pressed(move |_, key, _, modifiers| match key {
            gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter
                if !modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK) =>
            {
                on_key(true);
                glib::Propagation::Stop
            }
            gtk::gdk::Key::Escape => {
                on_key(false);
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        });
        editor.add_controller(keys);
        let focus = gtk::EventControllerFocus::new();
        let on_leave = finish.clone();
        focus.connect_leave(move |_| on_leave(true));
        editor.add_controller(focus);
    }

    /// Changes transcript.md line by line and shows the result, keeping the
    /// scroll position. Returns the text as it was, for undo.
    fn rewrite_transcript(
        self: &Rc<Self>,
        change: impl FnOnce(&mut Vec<String>),
    ) -> Option<String> {
        let dir = self.result_dir.borrow().clone()?;
        let path = dir.join("transcript.md");
        let before = std::fs::read_to_string(&path).ok()?;
        let mut lines: Vec<String> = before.lines().map(str::to_owned).collect();
        change(&mut lines);
        // Removing lines leaves their blank separators behind; keep one.
        let mut tidy: Vec<String> = Vec::new();
        for line in lines {
            if line.trim().is_empty() && tidy.last().is_some_and(|l| l.trim().is_empty()) {
                continue;
            }
            tidy.push(line);
        }
        let after = tidy.join("\n") + "\n";
        if std::fs::write(&path, &after).is_err() {
            self.toast(t("help.could_not_save_transcript"));
            return None;
        }
        self.redraw_transcript(&after);
        Some(before)
    }

    fn redraw_transcript(self: &Rc<Self>, markdown: &str) {
        let adjustment = self.transcript_scroll.vadjustment();
        let scroll = adjustment.value();
        self.show_transcript(Some(markdown), None);
        glib::idle_add_local_once(move || adjustment.set_value(scroll));
    }

    fn edit_paragraph(self: &Rc<Self>, sources: &[usize], time: &str, who: &str, text: &str) {
        let (first, rest) = match sources.split_first() {
            Some(split) => split,
            None => return,
        };
        let (first, rest) = (*first, rest.to_vec());
        let line = format!("**[{time}] {who}:** {text}");
        if self
            .rewrite_transcript(move |lines| {
                if let Some(slot) = lines.get_mut(first) {
                    *slot = line;
                }
                for index in rest.into_iter().rev() {
                    if index < lines.len() {
                        lines.remove(index);
                    }
                }
            })
            .is_some()
        {
            self.toast(t("help.saved"));
        }
    }

    fn delete_paragraph(self: &Rc<Self>, sources: &[usize]) {
        let mut sources = sources.to_vec();
        sources.sort_unstable();
        let Some(before) = self.rewrite_transcript(move |lines| {
            for index in sources.into_iter().rev() {
                if index < lines.len() {
                    lines.remove(index);
                }
            }
        }) else {
            return;
        };
        let toast = adw::Toast::builder()
            .title(t("help.line_deleted"))
            .button_label(t("misc.undo"))
            .build();
        let weak = Rc::downgrade(self);
        toast.connect_button_clicked(move |_| {
            let Some(r) = weak.upgrade() else { return };
            if let Some(dir) = r.result_dir.borrow().clone()
                && std::fs::write(dir.join("transcript.md"), &before).is_ok()
            {
                r.redraw_transcript(&before);
            }
        });
        self.toasts.add_toast(toast);
    }

    /// Gives a paragraph to the other speaker, for when whisper guessed wrong.
    fn swap_speaker(self: &Rc<Self>, sources: &[usize], who: &str) {
        let speakers = self
            .manifest
            .borrow()
            .as_ref()
            .map(|m| m.speakers.clone())
            .unwrap_or_default();
        if speakers.len() < 2 {
            return;
        }
        let next = speakers
            .iter()
            .position(|s| s == who)
            .map_or(0, |i| (i + 1) % speakers.len());
        let other = speakers[next].clone();
        let sources = sources.to_vec();
        self.rewrite_transcript(move |lines| {
            for index in sources {
                if let Some(line) = lines.get_mut(index)
                    && let Some((time, _, text)) = parse_segment(line)
                {
                    *line = format!("**[{time}] {other}:** {text}");
                }
            }
        });
    }

    /// Marks the transcript line that is playing at `ms`, and its chapter.
    fn highlight(&self, ms: i64) {
        let chapter = self
            .chapter_starts
            .borrow()
            .iter()
            .rposition(|start| *start <= ms);
        let selected = self
            .chapters_list
            .selected_row()
            .map(|r| r.index() as usize);
        if chapter != selected {
            match chapter.and_then(|i| self.chapters_list.row_at_index(i as i32)) {
                Some(row) => self.chapters_list.select_row(Some(&row)),
                None => self.chapters_list.unselect_all(),
            }
        }
        let line = self
            .segments
            .borrow()
            .iter()
            .rev()
            .find(|(_, start)| *start <= ms)
            .map(|(line, _)| *line)
            .unwrap_or(-1);
        let previous = self.current_line.replace(line);
        if line == previous {
            return;
        }
        if let Some(row) = self.transcript_list.row_at_index(previous) {
            row.remove_css_class("current");
        }
        let Some(row) = self.transcript_list.row_at_index(line) else {
            return;
        };
        row.add_css_class("current");
        // Follow along while playing, keeping the row a third from the top.
        if self.player.is_playing()
            && let Some(bounds) = row.compute_bounds(&self.transcript_list)
        {
            let adjustment = self.transcript_scroll.vadjustment();
            let (top, page) = (adjustment.value(), adjustment.page_size());
            let (y, h) = (f64::from(bounds.y()), f64::from(bounds.height()));
            if y < top || y + h > top + page {
                adjustment.set_value((y - page * 0.3).max(0.0));
            }
        }
    }

    /// One name row per speaker under the meeting name.
    fn show_speakers(self: &Rc<Self>) {
        for row in self.speaker_rows.borrow_mut().drain(..) {
            self.done_group.remove(&row);
        }
        let Some(manifest) = self.manifest.borrow().clone() else {
            return;
        };
        let mut rows = Vec::new();
        for (i, name) in manifest.speakers.iter().enumerate() {
            let title = match (manifest.imported.is_some(), i) {
                (false, 0) => t("speaker.row_mic").to_owned(),
                (false, _) if manifest.speakers.len() > 2 => {
                    t("speaker.row_computer_n").replace("{}", &i.to_string())
                }
                (false, _) => t("speaker.row_computer").to_owned(),
                (true, _) => tf("speaker.row_import", &[&(i + 1).to_string()]),
            };
            let row = adw::EntryRow::builder()
                .title(title)
                .text(name)
                .show_apply_button(true)
                .build();
            let weak = Rc::downgrade(self);
            row.connect_apply(move |_| {
                if let Some(r) = weak.upgrade() {
                    r.apply_speakers();
                }
            });
            let focus = gtk::EventControllerFocus::new();
            let weak = Rc::downgrade(self);
            focus.connect_leave(move |_| {
                if let Some(r) = weak.upgrade() {
                    // Deferred: saving rebuilds these rows.
                    glib::idle_add_local_once(move || r.apply_speakers());
                }
            });
            row.add_controller(focus);
            self.done_group.add(&row);
            rows.push(row);
        }
        *self.speaker_rows.borrow_mut() = rows;
    }

    /// Renames the speakers in transcript.md and the manifest. Your own name is
    /// also remembered for the next recordings.
    fn apply_speakers(self: &Rc<Self>) {
        if self.state.get() != State::Done {
            return;
        }
        let Some(dir) = self.result_dir.borrow().clone() else {
            return;
        };
        let Some(manifest) = self.manifest.borrow().clone() else {
            return;
        };
        let labels = manifest.default_labels();
        let names: Vec<String> = self
            .speaker_rows
            .borrow()
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let text = row.text().trim().replace(['*', '[', ']', ':', '\n'], "");
                if text.is_empty() {
                    labels.get(i).cloned().unwrap_or_else(|| {
                        t("speaker.empty_fallback").replace("{}", &(i + 1).to_string())
                    })
                } else {
                    text
                }
            })
            .collect();
        if names.is_empty() || names == manifest.speakers {
            return;
        }
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        if unique.len() != names.len() {
            self.toast(t("help.every_speaker"));
            self.show_speakers();
            return;
        }
        let renames: Vec<(String, String)> = manifest
            .speakers
            .iter()
            .cloned()
            .zip(names.iter().cloned())
            .filter(|(from, to)| from != to)
            .collect();
        let transcript = dir.join("transcript.md");
        if let Ok(text) = std::fs::read_to_string(&transcript) {
            let _ = std::fs::write(&transcript, meeting::relabel_all(&text, &renames));
        }
        let you_changed = manifest.imported.is_none() && names.first() != manifest.speakers.first();
        if let Some(m) = self.manifest.borrow_mut().as_mut() {
            m.speakers = names.clone();
            let _ = meeting::write(&dir, m);
        }
        if you_changed
            && let Some(you) = names.first()
            && let Err(e) = settings::save_your_name(you)
        {
            self.toast(&tf("prefs.save_failed", &[&e.to_string()]));
        }
        let text = std::fs::read_to_string(&transcript).ok();
        self.redraw_transcript(text.as_deref().unwrap_or(""));
        self.toast(t("help.saved"));
    }

    /// Renames the meeting folder after the name and rewrites the transcript heading.
    fn apply_title(&self) {
        if self.state.get() != State::Done {
            return;
        }
        let Some(current) = self.result_dir.borrow().clone() else {
            return;
        };
        let title = self.title();
        let target = meeting::folder_for(self.started_at.get(), &title);
        // "202609241400 Weekly 2" is still the folder of "Weekly": an import
        // got a number when the name was taken. Only a new name renames.
        let renamed = !folder_is_for(&current, &target);
        if renamed {
            if target.exists() {
                self.toast(t("help.folder_exists"));
                return;
            }
            if let Err(e) = std::fs::rename(&current, &target) {
                self.toast(&t("help.rename_failed").replace("{}", &e.to_string()));
                return;
            }
            *self.result_dir.borrow_mut() = Some(target.clone());
            self.render();
        }
        // The folder the meeting is in now: renamed, or the numbered one it had.
        let target = if renamed { target } else { current };
        let mut retitled = false;
        if let Some(manifest) = self.manifest.borrow_mut().as_mut()
            && manifest.title != title
        {
            manifest.title = title.clone();
            let _ = meeting::write(&target, manifest);
            retitled = true;
        }
        if !renamed && !retitled {
            return;
        }
        let transcript = target.join("transcript.md");
        if let Ok(text) = std::fs::read_to_string(&transcript)
            && let Some(rest) = text.strip_prefix("# ")
        {
            let body = rest.split_once('\n').map(|(_, body)| body).unwrap_or("");
            let _ = std::fs::write(&transcript, format!("# {title}\n{body}"));
        }
        self.toast(t("help.saved"));
    }

    fn close_when_done(&self) {
        self.quit_when_done.set(true);
        self.window.set_visible(false);
    }

    fn confirm_close_recording(self: &Rc<Self>) {
        let dialog = adw::AlertDialog::new(
            Some(t("done.close_recording_title")),
            Some(t("done.close_recording_body")),
        );
        dialog.add_response("keep", t("done.close_recording_keep"));
        dialog.add_response("stop", t("done.close_recording_stop"));
        dialog.set_response_appearance("stop", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("keep"));
        dialog.set_close_response("keep");
        let this = self.clone();
        dialog.connect_response(None, move |_, response| {
            if response == "stop" {
                this.stop();
                this.close_when_done();
            }
        });
        dialog.present(Some(&self.window));
    }

    fn confirm_close_transcribing(self: &Rc<Self>) {
        let dialog = adw::AlertDialog::new(
            Some(t("done.close_transcribing_title")),
            Some(t("done.close_transcribing_body")),
        );
        dialog.add_response("keep", t("done.close_transcribing_keep"));
        dialog.add_response("later", t("done.close_transcribing_later"));
        dialog.add_response("cancel", t("close.cancel_transcription"));
        dialog.set_response_appearance("cancel", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("keep"));
        dialog.set_close_response("keep");
        let this = self.clone();
        dialog.connect_response(None, move |_, response| match response {
            "later" => this.close_when_done(),
            "cancel" => {
                if let Some(abort) = this.abort.borrow().as_ref() {
                    abort.store(true, Ordering::Relaxed);
                }
                this.close_when_done();
            }
            _ => {}
        });
        dialog.present(Some(&self.window));
    }
}

/// Whether `folder` is `expected`, or `expected` with a number after it.
fn folder_is_for(folder: &std::path::Path, expected: &std::path::Path) -> bool {
    let (Some(name), Some(want)) = (
        folder.file_name().and_then(|n| n.to_str()),
        expected.file_name().and_then(|n| n.to_str()),
    ) else {
        return folder == expected;
    };
    folder.parent() == expected.parent()
        && name.strip_prefix(want).is_some_and(|rest| {
            rest.is_empty()
                || rest
                    .strip_prefix(' ')
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
}

fn row_count(list: &gtk::ListBox) -> i32 {
    let mut count = 0;
    while list.row_at_index(count).is_some() {
        count += 1;
    }
    count
}

/// Width in pixels of `text` in the font of `widget`.
fn text_width(widget: &impl IsA<gtk::Widget>, text: &str) -> i32 {
    widget.create_pango_layout(Some(text)).pixel_size().0
}

fn chapter_row(title: &str, start_ms: i64) -> gtk::ListBoxRow {
    let label = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .wrap(true)
        .css_classes(["chapter-heading"])
        .build();
    let row = gtk::ListBoxRow::builder()
        .child(&label)
        .tooltip_text(t("chapter.play_from").replace("{}", &chapters::clock(start_ms)))
        .css_classes(["chapter"])
        .build();
    row.set_cursor_from_name(Some("pointer"));
    row
}

fn note_row(text: &str) -> gtk::ListBoxRow {
    gtk::ListBoxRow::builder()
        .child(
            &gtk::Label::builder()
                .label(text)
                .xalign(0.0)
                .wrap(true)
                .css_classes(["dim-label"])
                .build(),
        )
        .activatable(false)
        .build()
}

/// Transcript lines grouped into one paragraph per turn for reading. Older
/// transcripts have a line per sentence; a line by the same speaker that
/// follows closely on the previous one joins it.
#[derive(Default)]
struct Paragraphs {
    list: Vec<Paragraph>,
}

struct Paragraph {
    /// The lines of transcript.md this paragraph was made of.
    sources: Vec<usize>,
    time: String,
    start_ms: i64,
    speaker: String,
    text: String,
    /// Start and length of the last line added, to estimate when it ended.
    last_start_ms: i64,
    last_chars: usize,
}

impl Paragraphs {
    const MAX_MS: i64 = 90_000;

    fn add(&mut self, time: &str, speaker: &str, text: &str, source: usize) {
        let start_ms = clock_to_ms(time);
        if let Some(last) = self.list.last_mut() {
            // About 15 characters a second, plus a pause of up to 3 seconds.
            let ended = last.last_start_ms + last.last_chars as i64 * 1000 / 15;
            if last.speaker == speaker
                && start_ms - ended.max(last.last_start_ms + 3000) <= 3000
                && start_ms - last.start_ms < Self::MAX_MS
            {
                last.text.push(' ');
                last.text.push_str(text);
                last.last_start_ms = start_ms;
                last.last_chars = text.chars().count();
                last.sources.push(source);
                return;
            }
        }
        self.list.push(Paragraph {
            sources: vec![source],
            time: time.to_owned(),
            start_ms,
            speaker: speaker.to_owned(),
            text: text.to_owned(),
            last_start_ms: start_ms,
            last_chars: text.chars().count(),
        });
    }
}

/// Repaints a widget and everything in it, for custom drawing after a theme switch.
fn redraw(widget: &impl IsA<gtk::Widget>) {
    widget.queue_draw();
    let mut child = widget.first_child();
    while let Some(current) = child {
        redraw(&current);
        child = current.next_sibling();
    }
}

/// Decodes any audio or video file with ffmpeg into the meeting folder: the
/// levelled `audio.ogg` to listen to and `.tracks/source.ogg` to transcribe
/// again. Returns the duration in seconds.
fn import_audio(
    source: &std::path::Path,
    out: &std::path::Path,
    staging: &std::path::Path,
) -> Result<i64, String> {
    std::fs::create_dir_all(out.join(export::TRACKS_DIR)).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(staging).map_err(|e| e.to_string())?;
    let raw = staging.join("mic.raw");
    let silence = staging.join("system.raw");
    let decoded = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-nostdin", "-i"])
        .arg(source)
        .args(["-vn", "-f", "s16le", "-ar", "48000", "-ac", "2"])
        .arg(&raw)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let bytes = std::fs::metadata(&raw).map(|m| m.len()).unwrap_or(0);
    if !decoded || bytes == 0 {
        let _ = std::fs::remove_dir_all(staging);
        return Err(t("import.no_audio").into());
    }
    let _ = std::fs::write(&silence, []);
    let listened = export_audio(&raw, &silence, out, Format::Mono);
    let kept = std::process::Command::new("ffmpeg")
        .args([
            "-v", "error", "-y", "-nostdin", "-f", "s16le", "-ar", "48000", "-ac", "2", "-i",
        ])
        .arg(&raw)
        .args(["-ac", "1", "-c:a", "libopus", "-b:a", "48k"])
        .arg(source_track(out))
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let _ = std::fs::remove_dir_all(staging);
    if !listened || !kept {
        return Err(t("import.no_convert").into());
    }
    Ok((bytes / (48_000 * 2 * 2)) as i64)
}

/// Keeps the staging note current: the title, start, format and language
/// a recovery needs if this recording never reaches Stop.
fn write_recording_note(
    staging: &std::path::Path,
    title: &str,
    started_at: i64,
    format: Format,
    language: &str,
) {
    finish::write_note(
        staging,
        &RecordingNote {
            title: title.to_owned(),
            started_at,
            format: Some(format),
            language: Some(language.to_owned()),
        },
    );
}

/// Recording staging folders left behind, with some audio in them.
fn unfinished_recordings() -> Vec<PathBuf> {
    let root = momr_platform::paths::cache();
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        // Either track counts: a machine without a microphone still records
        // the computer audio.
        .filter(|dir| raw_duration(dir) > 0)
        .collect();
    found.sort();
    found
}

/// `01:23` or `1:02:03` to milliseconds.
fn clock_to_ms(clock: &str) -> i64 {
    clock
        .split(':')
        .filter_map(|part| part.parse::<i64>().ok())
        .fold(0, |total, part| total * 60 + part)
        * 1000
}

fn format_elapsed(secs: i64) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

/// What a meter shows while the recording is paused: the levels at the
/// moment of pausing, dimmed and still.
type Frozen = Rc<RefCell<Option<Vec<f32>>>>;

fn meter(
    source: &Source,
    (key, fallback): (&'static str, (f64, f64, f64)),
    height: i32,
    frozen: &Frozen,
    live: &Rc<Cell<bool>>,
) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_height(height)
        .hexpand(true)
        .build();
    let source = source.clone();
    let frozen = frozen.clone();
    let live = live.clone();
    area.set_draw_func(move |area, cr, width, height| {
        let (r, g, b) = crate::theme::color(key, fallback);
        let (width, height) = (f64::from(width), f64::from(height));
        let paused = frozen.borrow().is_some();
        if !live.get() && !paused {
            // Not recording yet: no moving wave, just a line that thickens
            // with the sound coming in, as a level check. Thick is loud.
            let (r, g, b) = crate::theme::color(key, fallback);
            let level = to_meter(source.recent_peak(4));
            // The big meters keep their top strip for the caption; the line
            // grows inside the space below it and never reaches the text.
            let (top, bottom) = if height >= 40.0 {
                (22.0, height - 4.0)
            } else {
                (2.0, height - 2.0)
            };
            let thickness = 1.0 + level * (bottom - top - 1.0);
            cr.set_source_rgba(r, g, b, 0.3 + 0.6 * level);
            cr.rectangle(0.0, (top + bottom - thickness) / 2.0, width, thickness);
            let _ = cr.fill();
            if height >= 40.0 {
                let ink = area.color();
                cr.set_source_rgba(
                    f64::from(ink.red()),
                    f64::from(ink.green()),
                    f64::from(ink.blue()),
                    0.45,
                );
                cr.select_font_face(
                    "sans-serif",
                    gtk::cairo::FontSlant::Normal,
                    gtk::cairo::FontWeight::Normal,
                );
                cr.set_font_size(12.0);
                let text = t("canvas.not_recording");
                let w = cr.text_extents(text).map(|e| e.x_advance()).unwrap_or(0.0);
                // In the top part, clear of the line at its thickest.
                cr.move_to((width - w) / 2.0, 15.0);
                let _ = cr.show_text(text);
            }
            return;
        }
        let levels = frozen.borrow().clone().unwrap_or_else(|| source.levels());
        let dim = if paused { 0.3 } else { 1.0 };
        let mid = height / 2.0;
        let step = width / HISTORY as f64;
        let bar = (step * 0.6).max(1.0);
        cr.set_source_rgba(r, g, b, 0.15);
        cr.rectangle(0.0, mid - 0.5, width, 1.0);
        let _ = cr.fill();
        for (i, peak) in levels.into_iter().enumerate() {
            let h = (to_meter(peak) * (height - 4.0)).max(1.0);
            cr.set_source_rgba(r, g, b, (0.35 + 0.65 * i as f64 / HISTORY as f64) * dim);
            cr.rectangle(i as f64 * step, mid - h / 2.0, bar, h);
            let _ = cr.fill();
        }
        // A pause sign in the middle of the big meters, so it is obvious.
        if paused && height >= 40.0 {
            let ink = area.color();
            let (ir, ig, ib) = (
                f64::from(ink.red()),
                f64::from(ink.green()),
                f64::from(ink.blue()),
            );
            let (cx, cy) = (width / 2.0, mid);
            cr.set_source_rgba(ir, ig, ib, 0.9);
            cr.rectangle(cx - 42.0, cy - 8.0, 5.0, 16.0);
            cr.rectangle(cx - 33.0, cy - 8.0, 5.0, 16.0);
            let _ = cr.fill();
            cr.select_font_face(
                "sans-serif",
                gtk::cairo::FontSlant::Normal,
                gtk::cairo::FontWeight::Bold,
            );
            cr.set_font_size(13.0);
            cr.move_to(cx - 20.0, cy + 5.0);
            let _ = cr.show_text(t("canvas.paused"));
        }
    });
    area
}

/// The strip's wave: the microphone history above the midline and the
/// computer history below it, the way the player draws a meeting, so the
/// strip reads as the same app. Paused, the last levels stay, dimmed.
fn strip_wave(
    mic: &Source,
    system: &Source,
    frozen: &[Frozen; 2],
    live: &Rc<Cell<bool>>,
) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_height(44)
        .width_request(COMPACT_SIZE.0 - 24)
        .hexpand(true)
        .css_classes(["strip-wave"])
        .build();
    let (mic, system) = (mic.clone(), system.clone());
    let (frozen_mic, frozen_system) = (frozen[0].clone(), frozen[1].clone());
    let live = live.clone();
    area.set_draw_func(move |_, cr, width, height| {
        let (width, height) = (f64::from(width), f64::from(height));
        let mid = height / 2.0;
        let step = width / HISTORY as f64;
        let bar = (step * 0.6).max(1.0);
        let paused = frozen_mic.borrow().is_some();
        let dim = if paused || !live.get() { 0.3 } else { 1.0 };
        let lanes = [
            (
                crate::theme::color("blue", MIC_COLOR),
                frozen_mic.borrow().clone().unwrap_or_else(|| mic.levels()),
                -1.0,
            ),
            (
                crate::theme::color("orange", SYSTEM_COLOR),
                frozen_system
                    .borrow()
                    .clone()
                    .unwrap_or_else(|| system.levels()),
                1.0,
            ),
        ];
        for ((r, g, b), levels, direction) in lanes {
            cr.set_source_rgba(r, g, b, 0.15);
            cr.rectangle(0.0, mid - 0.5, width, 1.0);
            let _ = cr.fill();
            for (i, peak) in levels.into_iter().enumerate() {
                let h = (to_meter(peak) * (mid - 2.0)).max(1.0);
                cr.set_source_rgba(r, g, b, (0.35 + 0.65 * i as f64 / HISTORY as f64) * dim);
                let y = if direction < 0.0 { mid - h } else { mid };
                cr.rectangle(i as f64 * step, y, bar, h);
                let _ = cr.fill();
            }
        }
    });
    area
}

fn meter_block(name: &str, meter: &gtk::DrawingArea) -> gtk::Box {
    let block = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    block.append(
        &gtk::Label::builder()
            .label(name)
            .xalign(0.0)
            .css_classes(["heading"])
            .build(),
    );
    block.append(
        &gtk::Frame::builder()
            .child(meter)
            .css_classes(["card"])
            .build(),
    );
    block
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `win.*` action the window registers in `install_actions`.
    const WIN_ACTIONS: &[&str] = &[
        "win.new-recording",
        "win.open-meeting",
        "win.import",
        "win.reveal",
        "win.start",
        "win.pause",
        "win.stop",
        "win.timer",
        "win.compact",
        "win.copy-transcript",
        "win.fullscreen",
        "win.transcribe-again",
    ];

    fn actions_in(model: &gio::MenuModel, out: &mut Vec<String>) {
        for i in 0..model.n_items() {
            if let Some(action) = model
                .item_attribute_value(i, "action", None)
                .and_then(|v| v.get::<String>())
            {
                // A `win.transcribe-again::"en"` target still names its action.
                let base = action.split("::").next().unwrap_or(&action).to_owned();
                out.push(base);
            }
            let links = model.iterate_item_links(i);
            while let Some((_, linked)) = links.next() {
                actions_in(&linked, out);
            }
        }
    }

    #[test]
    fn menu_items_name_registered_actions() {
        let bar = menu_model();
        let mut actions = Vec::new();
        actions_in(bar.upcast_ref::<gio::MenuModel>(), &mut actions);
        assert!(!actions.is_empty());
        for action in &actions {
            let known = action.starts_with("win.")
                || action.starts_with("app.")
                || action.starts_with("window.")
                || action.starts_with("text.")
                || action.starts_with("clipboard.")
                || action.starts_with("selection.");
            assert!(known, "menu names unknown action {action}");
            if action.starts_with("win.") {
                assert!(
                    WIN_ACTIONS.contains(&action.as_str()),
                    "menu names unregistered {action}"
                );
            }
        }
        for expected in WIN_ACTIONS {
            assert!(
                actions.iter().any(|a| a == expected),
                "menu misses {expected}"
            );
        }
    }
}
