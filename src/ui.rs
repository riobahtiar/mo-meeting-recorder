//! The recorder window. It has one page per phase: recording (which can shrink
//! to a compact strip with only the waves and the clock), transcribing (the
//! animation, edge to edge) and done (the transcript and what to do with it).

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use adw::prelude::*;
use gtk::{gio, glib};

use crate::agent::{self, Agent};
use crate::animation::TranscribeAnimation;
use crate::audio::{Device, HISTORY, Source, to_meter};
use crate::chapters::{self, Chapter};
use crate::export::{self, Format, export_audio, export_tracks};
use crate::ipc::{self, SharedStatus, Status};
use crate::meeting::{self, Manifest};
use crate::player::Player;
use crate::transcribe::{self, Abort, CANCELLED, Event, LANGUAGES};
use crate::{APP_ID, APP_NAME, settings};

const MIC_COLOR: (f64, f64, f64) = (0.0, 0.478, 1.0);
const SYSTEM_COLOR: (f64, f64, f64) = (1.0, 0.584, 0.0);
const FULL_SIZE: (i32, i32) = (480, 700);
const COMPACT_SIZE: (i32, i32) = (300, 84);
const DONE_SIZE: (i32, i32) = (1100, 760);

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
const SPEAKER_CHOICES: [&str; 7] = ["Automatic", "1", "2", "3", "4", "5", "6"];

/// The menu structure alone, so tests can walk every item without an app.
fn menu_model() -> gio::Menu {
    use gio::Menu;
    let item = |label: &str, action: &str| gio::MenuItem::new(Some(label), Some(action));

    let file = Menu::new();
    file.append_item(&item("New Recording", "win.new-recording"));
    file.append_item(&item("Open Meeting…", "win.open-meeting"));
    file.append_item(&item("Import Audio File…", "win.import"));
    let finder = Menu::new();
    finder.append_item(&item("Reveal in Finder", "win.reveal"));
    file.append_section(None, &finder);
    let close = Menu::new();
    close.append_item(&item("Close Window", "window.close"));
    file.append_section(None, &close);

    let edit = Menu::new();
    for (label, action) in [("Undo", "text.undo"), ("Redo", "text.redo")] {
        edit.append_item(&item(label, action));
    }
    let clipboard = Menu::new();
    for (label, action) in [
        ("Cut", "clipboard.cut"),
        ("Copy", "clipboard.copy"),
        ("Paste", "clipboard.paste"),
        ("Select All", "selection.select-all"),
    ] {
        clipboard.append_item(&item(label, action));
    }
    edit.append_section(None, &clipboard);
    let transcript = Menu::new();
    transcript.append_item(&item("Copy Transcript", "win.copy-transcript"));
    edit.append_section(None, &transcript);

    let recording = Menu::new();
    recording.append_item(&item("Start Recording", "win.start"));
    recording.append_item(&item("Pause / Resume", "win.pause"));
    recording.append_item(&item("Stop Recording", "win.stop"));

    let view = Menu::new();
    view.append_item(&item("Compact Strip", "win.compact"));
    view.append_item(&item("Enter Full Screen", "win.fullscreen"));
    let again = Menu::new();
    for (code, name) in LANGUAGES {
        let entry = gio::MenuItem::new(Some(name), None);
        entry.set_action_and_target_value(
            Some("win.transcribe-again"),
            Some(&glib::Variant::from(code)),
        );
        again.append_item(&entry);
    }
    view.append_submenu(Some("Transcribe Again"), &again);

    // macOS manages Minimize, Zoom and the window list itself.
    let window = Menu::new();
    let help = Menu::new();
    help.append_item(&item("MOM Recorder Help", "app.help"));

    let bar = Menu::new();
    bar.append_submenu(Some("File"), &file);
    bar.append_submenu(Some("Edit"), &edit);
    bar.append_submenu(Some("Recording"), &recording);
    bar.append_submenu(Some("View"), &view);
    bar.append_submenu(Some("Window"), &window);
    bar.append_submenu(Some("Help"), &help);
    bar
}

/// The native menu bar: GTK's quartz backend turns this `GMenuModel` into the
/// NSMenu bar, and adds the standard app menu when `app.about`,
/// `app.preferences` and `app.quit` exist. Every item is a `GAction` from
/// `install_actions`, so enabled state is shared with the buttons.
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
        load_css();
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
        install_menubar(app);
        app.set_accels_for_action("win.new-recording", &["<Primary>n"]);
        app.set_accels_for_action("win.open-meeting", &["<Primary>o"]);
        app.set_accels_for_action("win.import", &["<Primary><Shift>i"]);
        app.set_accels_for_action("win.reveal", &["<Primary><Alt>r"]);
        app.set_accels_for_action("window.close", &["<Primary>w"]);
        app.set_accels_for_action("win.start", &["<Primary>r"]);
        app.set_accels_for_action("win.pause", &["<Primary><Shift>r"]);
        app.set_accels_for_action("win.stop", &["<Primary>period"]);
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
/// config.toml disables it; a missing binary is silently skipped.
fn spawn_menubar(slot: &Rc<RefCell<Option<std::process::Child>>>) {
    if crate::models::config_value("menubar").as_deref() == Some("false") {
        return;
    }
    let Some(binary) = crate::helper::menubar_path() else {
        return;
    };
    if let Ok(child) = std::process::Command::new(binary)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        *slot.borrow_mut() = Some(child);
    }
}

struct Recorder {
    window: adw::ApplicationWindow,
    view: adw::ToolbarView,
    toasts: adw::ToastOverlay,
    layout: gtk::Stack,
    compact_action: gio::SimpleAction,
    compact_button: gtk::Button,
    title_row: adw::EntryRow,
    format_row: adw::ComboRow,
    language_row: adw::ComboRow,
    animation: TranscribeAnimation,
    meters: [gtk::DrawingArea; 2],
    compact_meters: [gtk::DrawingArea; 2],
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
    /// The size the window was last fitted to, per page.
    fitted: Cell<(i32, i32)>,
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
    /// Fullscreen toggled through the View menu, for the label.
    fullscreen: Cell<bool>,
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
        ipc::serve(shared.clone(), mic.clone(), system.clone(), commands_tx);

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("MOM Recorder")
            .default_width(FULL_SIZE.0)
            .default_height(FULL_SIZE.1)
            .build();
        window.add_css_class("macos");
        let view = adw::ToolbarView::new();
        let header = adw::HeaderBar::new();
        let compact_button = gtk::Button::builder()
            .icon_name("view-restore-symbolic")
            .tooltip_text("Compact strip (⇧⌘M)")
            .action_name("win.compact")
            .build();
        header.pack_start(&compact_button);
        view.add_top_bar(&header);
        // Under the header bar, full width, while the speech model still has
        // to be downloaded.
        let model_banner = adw::Banner::builder().revealed(false).build();
        view.add_top_bar(&model_banner);
        let toasts = adw::ToastOverlay::new();
        view.set_content(Some(&toasts));
        window.set_content(Some(&view));

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
            .title("Meeting name")
            .show_apply_button(true)
            .build();
        let format_labels: Vec<&str> = Format::ALL.iter().map(|f| f.label()).collect();
        let format_row = adw::ComboRow::builder()
            .title("Audio file")
            .subtitle("Can be changed during the call")
            .model(&gtk::StringList::new(&format_labels))
            .build();
        let saved = settings::load_format();
        format_row.set_selected(Format::ALL.iter().position(|f| *f == saved).unwrap_or(0) as u32);
        let language_labels: Vec<&str> = LANGUAGES.iter().map(|(_, label)| *label).collect();
        let language_row = adw::ComboRow::builder()
            .title("Language")
            .subtitle("Used for the transcript after the call")
            .model(&gtk::StringList::new(&language_labels))
            .build();
        let saved = settings::load_language();
        language_row.set_selected(
            LANGUAGES
                .iter()
                .position(|(code, _)| *code == saved)
                .unwrap_or(0) as u32,
        );
        group.add(&title_row);
        group.add(&format_row);
        group.add(&language_row);
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
        meters_box.append(&meter_block("You (microphone)", &meters[0]));
        meters_box.append(&meter_block("Computer audio", &meters[1]));

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
            .css_classes(["title-1", "numeric"])
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
            .label("Pause")
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
            .label("Import an audio file, or drop one here")
            .halign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        content.append(&import_button);

        // Transcribing: the animation fills the whole window.
        let animation = TranscribeAnimation::new();

        // Done: a wide page. Left the meeting and what to do with it, right the
        // player and the transcript.
        let left = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(16)
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
            .label("Meeting saved")
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
            .title("Meeting name")
            .show_apply_button(true)
            .build();
        done_group.add(&done_title_row);
        left.append(&done_group);

        // Chapters: a list to jump through, with the agent's button in the header.
        let chapters_spinner = adw::Spinner::builder().visible(false).build();
        let chapters_button = gtk::Button::builder()
            .label("Generate")
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        let chapters_suffix = gtk::Box::builder().spacing(6).build();
        chapters_suffix.append(&chapters_spinner);
        chapters_suffix.append(&chapters_button);
        let chapters_group = adw::PreferencesGroup::builder()
            .title("Chapters")
            .header_suffix(&chapters_suffix)
            .vexpand(true)
            .build();
        let chapters_list = gtk::ListBox::builder()
            .css_classes(["boxed-list"])
            .selection_mode(gtk::SelectionMode::Single)
            .valign(gtk::Align::Start)
            .build();
        chapters_list.set_placeholder(Some(
            &gtk::Label::builder()
                .label("No chapters yet")
                .css_classes(["dim-label"])
                .margin_top(14)
                .margin_bottom(14)
                .build(),
        ));
        let chapters_scroll = gtk::ScrolledWindow::builder()
            .child(&chapters_list)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        chapters_group.add(&chapters_scroll);
        left.append(&chapters_group);

        let copy_button = gtk::Button::builder()
            .label("Copy transcript")
            .css_classes(["pill", "suggested-action"])
            .build();
        left.append(&copy_button);

        let actions = gtk::Box::builder().spacing(8).homogeneous(true).build();
        let open_button = gtk::Button::builder()
            .label("Reveal in Finder")
            .css_classes(["pill"])
            .action_name("win.reveal")
            .build();
        let new_button = gtk::Button::builder()
            .label("New recording")
            .css_classes(["pill"])
            .action_name("win.new-recording")
            .build();
        actions.append(&open_button);
        actions.append(&new_button);
        left.append(&actions);

        let again_group = adw::PreferencesGroup::new();
        let again_language_row = adw::ComboRow::builder()
            .title("Language")
            .subtitle("Transcribe again")
            .model(&gtk::StringList::new(&language_labels))
            .selected(language_row.selected())
            .build();
        let again_button = gtk::Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text("Transcribe again")
            .valign(gtk::Align::Center)
            .css_classes(["flat", "circular"])
            .build();
        again_language_row.add_suffix(&again_button);
        again_group.add(&again_language_row);
        left.append(&again_group);

        let right = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .hexpand(true)
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

        let done = gtk::Box::builder()
            .spacing(24)
            .margin_top(12)
            .margin_bottom(24)
            .margin_start(24)
            .margin_end(24)
            .build();
        // A fixed, narrow column on the left; the transcript takes the rest.
        let left_column = adw::Clamp::builder()
            .child(&left)
            .maximum_size(320)
            .tightening_threshold(320)
            .width_request(320)
            .hexpand(false)
            .build();
        done.append(&left_column);
        done.append(&right);

        // Compact mode: only the two waves and the clock.
        let compact_meters = [
            meter(&mic, ("blue", MIC_COLOR), 26, &frozen[0], &live),
            meter(&system, ("orange", SYSTEM_COLOR), 26, &frozen[1], &live),
        ];
        let compact_dot = gtk::Label::builder()
            .label("●")
            .css_classes(["error"])
            .build();
        let compact_timer = gtk::Label::builder()
            .label("00:00")
            .css_classes(["numeric", "heading"])
            .build();
        let compact_clock = gtk::Box::builder()
            .spacing(6)
            .valign(gtk::Align::Center)
            .build();
        compact_clock.append(&compact_dot);
        compact_clock.append(&compact_timer);
        let compact_waves = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();
        compact_waves.append(&compact_meters[0]);
        compact_waves.append(&compact_meters[1]);
        let compact_strip = gtk::Box::builder()
            .spacing(12)
            .margin_top(10)
            .margin_bottom(10)
            .margin_start(14)
            .margin_end(8)
            .build();
        let expand_button = gtk::Button::builder()
            .icon_name("view-fullscreen-symbolic")
            .tooltip_text("Expand (Ctrl+M)")
            .action_name("win.compact")
            .valign(gtk::Align::Center)
            .css_classes(["flat", "circular"])
            .build();
        compact_strip.append(&compact_clock);
        compact_strip.append(&compact_waves);
        compact_strip.append(&expand_button);
        // The whole strip drags the window around, like a title bar.
        let compact = gtk::WindowHandle::builder()
            .child(&compact_strip)
            .tooltip_text("Drag to move, Ctrl+M to expand")
            .build();

        // Not homogeneous, so the window can shrink to the compact page.
        let layout = gtk::Stack::builder()
            .hhomogeneous(false)
            .vhomogeneous(false)
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(250)
            .build();
        layout.add_named(&content, Some("record"));
        layout.add_named(animation.widget(), Some("transcribing"));
        layout.add_named(&done, Some("done"));
        layout.add_named(&compact, Some("compact"));
        let drop_hint = gtk::Label::builder()
            .label("Drop to import")
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
            toasts,
            layout,
            compact_action,
            compact_button,
            title_row,
            format_row,
            language_row,
            animation,
            meters,
            compact_meters,
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
            fitted: Cell::new(FULL_SIZE),
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
            full_size: Cell::new(FULL_SIZE),
            started_at: Cell::new(0),
            paused: Cell::new(false),
            frozen,
            live,
            animation_since: Cell::new(None),
            paused_secs: Cell::new(0),
            pause_began: Cell::new(0),
            fullscreen: Cell::new(false),
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
        // File managers offer a file list (Nautilus), some a single file.
        let drop = gtk::DropTarget::new(glib::Type::INVALID, gtk::gdk::DragAction::COPY);
        drop.set_types(&[gtk::gdk::FileList::static_type(), gio::File::static_type()]);
        // GTK's own check turns the drag down when the compositor offers the
        // source's preferred action (move) rather than copy, so decide on
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
                settings::save_format(r.selected_format());
            }
        });

        // The two language rows (recording page, done page) are one setting.
        let weak = Rc::downgrade(self);
        self.language_row.connect_selected_notify(move |row| {
            if let Some(r) = weak.upgrade() {
                if !r.loading.get() {
                    settings::save_language(r.selected_language());
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

        let weak = Rc::downgrade(self);
        self.window.connect_close_request(move |_| {
            let Some(r) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
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
                        r.compact_meters.iter().for_each(|m| m.queue_draw());
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
        Self::on(&act("copy-transcript"), self, |r| r.copy_transcript());
        Self::on(&act("fullscreen"), self, |r| r.toggle_fullscreen());
        let again = gio::SimpleAction::new("transcribe-again", Some(glib::VariantTy::STRING));
        let weak = Rc::downgrade(self);
        again.connect_activate(move |_, param| {
            let Some(r) = weak.upgrade() else { return };
            let language = param.and_then(|p| p.get::<String>()).unwrap_or_default();
            r.transcribe_again(&language);
        });
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
        filter.set_name(Some("Audio and video"));
        filter.add_mime_type("audio/*");
        filter.add_mime_type("video/*");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title("Import an audio file")
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
        filter.set_name(Some("Meeting recordings"));
        filter.add_suffix("meeting-recorder");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title("Open a meeting (.meeting-recorder file)")
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
        let target = crate::meeting::find(&dir).unwrap_or(dir);
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
                self.toast("Copied to clipboard");
            }
            Err(_) => self.toast("No transcript found"),
        }
    }

    /// Transcribes the done meeting again in `language` (a code from
    /// `LANGUAGES`, falling back to the selected one).
    fn transcribe_again(self: &Rc<Self>, language: &str) {
        if self.state.get() != State::Done {
            return;
        }
        let language: &'static str = crate::transcribe::LANGUAGES
            .iter()
            .find(|(code, _)| *code == language)
            .map(|(code, _)| *code)
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

    fn toggle_fullscreen(&self) {
        let fullscreen = !self.fullscreen.get();
        self.fullscreen.set(fullscreen);
        if fullscreen {
            self.window.fullscreen();
        } else {
            self.window.unfullscreen();
        }
    }

    fn show_about(&self) {
        let dialog = adw::AboutDialog::builder()
            .application_name("MOM Recorder")
            .application_icon("io.github.riobahtiar.MOMRecorder")
            .version(env!("CARGO_PKG_VERSION"))
            .developer_name("Rio Bahtiar")
            .copyright("© 2026 Rio Bahtiar")
            .comments("Two-track meeting recorder: your microphone and the computer audio, transcribed on this Mac.")
            .website("https://github.com/riobahtiar/mo-meeting-recorder")
            .issue_url("https://github.com/riobahtiar/mo-meeting-recorder/issues")
            .license_type(gtk::License::MitX11)
            .build();
        dialog.add_acknowledgement_section(
            Some("Transcription"),
            &[
                "whisper.cpp through whisper-rs",
                "Nemotron 3 Diarization (ONNX community export)",
                "Based on Meeting Recorder by Jankees van Woezik",
            ],
        );
        dialog.present(Some(&self.window));
    }

    fn show_help(&self) {
        let uri = "https://github.com/riobahtiar/mo-meeting-recorder";
        let _ = gio::AppInfo::launch_default_for_uri(uri, None::<&gio::AppLaunchContext>);
    }

    /// App settings (⌘,): model, agent, format, language, name, meetings
    /// folder and audio status. Values apply at once; the ready page re-reads
    /// the model banner and the language on close.
    fn show_preferences(self: &Rc<Self>) {
        let dialog = adw::PreferencesDialog::builder().title("Settings").build();
        let page = adw::PreferencesPage::builder().title("Settings").build();
        dialog.add(&page);
        let transcription = adw::PreferencesGroup::builder()
            .title("Transcription")
            .build();
        let model_names: Vec<&str> = crate::models::MODELS.iter().map(|m| m.name).collect();
        let model_row = adw::ComboRow::builder()
            .title("Speech model")
            .model(&gtk::StringList::new(&model_names))
            .build();
        let current_model = crate::models::configured();
        model_row.set_selected(
            crate::models::MODELS
                .iter()
                .position(|m| m.name == current_model)
                .unwrap_or(0) as u32,
        );
        let model_subtitle = |name: &str| {
            let model = crate::models::MODELS.iter().find(|m| m.name == name);
            let downloaded = model.is_some_and(|m| {
                crate::transcribe::models_dir()
                    .join(format!("ggml-{}.bin", m.name))
                    .is_file()
            });
            match (downloaded, model) {
                (true, _) => "On this Mac".to_owned(),
                (_, Some(m)) if m.size_mb >= 1000 => {
                    format!(
                        "About {:.1} GB, downloads on first use",
                        f64::from(m.size_mb) / 1000.0
                    )
                }
                (_, Some(m)) => format!("About {} MB, downloads on first use", m.size_mb),
                (_, None) => String::new(),
            }
        };
        model_row.set_subtitle(&model_subtitle(&current_model));
        let weak = Rc::downgrade(self);
        model_row.connect_selected_notify(move |row| {
            let Some(r) = weak.upgrade() else { return };
            let name = model_names[row.selected() as usize];
            crate::models::save_config_value("model", name);
            row.set_subtitle(&model_subtitle(name));
            r.update_model_banner();
        });
        transcription.add(&model_row);
        let language_labels: Vec<&str> = LANGUAGES.iter().map(|(_, label)| *label).collect();
        let prefs_language = adw::ComboRow::builder()
            .title("Default language")
            .model(&gtk::StringList::new(&language_labels))
            .build();
        let saved_language = settings::load_language();
        prefs_language.set_selected(
            LANGUAGES
                .iter()
                .position(|(code, _)| *code == saved_language)
                .unwrap_or(0) as u32,
        );
        let weak = Rc::downgrade(self);
        prefs_language.connect_selected_notify(move |row| {
            let Some(r) = weak.upgrade() else { return };
            let code = LANGUAGES[row.selected() as usize].0;
            settings::save_language(code);
            // The ready page owns the default: keep its dropdown in step.
            r.loading.set(true);
            r.language_row.set_selected(row.selected());
            r.again_language_row.set_selected(row.selected());
            r.loading.set(false);
        });
        transcription.add(&prefs_language);
        page.add(&transcription);
        let chapters = adw::PreferencesGroup::builder()
            .title("Chapters")
            .description("A coding agent with every tool switched off writes the chapter titles.")
            .build();
        let agents = crate::agent::installed_agents();
        let mut agent_names = vec!["None"];
        agent_names.extend(agents.iter().map(|a| a.name));
        let agent_row = adw::ComboRow::builder()
            .title("Agent")
            .model(&gtk::StringList::new(&agent_names))
            .build();
        let current_agent = crate::models::config_value("agent").unwrap_or_default();
        agent_row.set_selected(
            agents
                .iter()
                .position(|a| a.id == current_agent)
                .map(|i| i + 1)
                .unwrap_or(0) as u32,
        );
        agent_row.connect_selected_notify(move |row| {
            let selected = row.selected() as usize;
            let id = if selected == 0 {
                String::new()
            } else {
                agents[selected - 1].id.clone()
            };
            crate::models::save_config_value("agent", &id);
        });
        chapters.add(&agent_row);
        page.add(&chapters);
        let recording = adw::PreferencesGroup::builder().title("Recording").build();
        let format_labels: Vec<&str> = Format::ALL.iter().map(|f| f.label()).collect();
        let format_row = adw::ComboRow::builder()
            .title("Default audio format")
            .model(&gtk::StringList::new(&format_labels))
            .build();
        format_row.set_selected(
            Format::ALL
                .iter()
                .position(|f| *f == settings::load_format())
                .unwrap_or(0) as u32,
        );
        format_row.connect_selected_notify(|row| {
            settings::save_format(Format::ALL[row.selected() as usize]);
        });
        recording.add(&format_row);
        let name_row = adw::EntryRow::builder()
            .title("Your name in transcripts")
            .text(settings::load_your_name())
            .build();
        name_row.connect_apply(|row| {
            settings::save_your_name(&row.text());
        });
        recording.add(&name_row);
        let meetings_path = settings::load_meetings_dir().unwrap_or_else(crate::paths::meetings);
        let meetings_row = adw::ActionRow::builder()
            .title("Meetings folder")
            .subtitle(meetings_path.display().to_string())
            .build();
        let choose = gtk::Button::builder()
            .label("Choose…")
            .valign(gtk::Align::Center)
            .build();
        meetings_row.add_suffix(&choose);
        let this = self.clone();
        let row = meetings_row.clone();
        choose.connect_clicked(move |_| {
            let folders = gtk::FileDialog::builder().title("Meetings folder").build();
            let (row, window) = (row.clone(), this.window.clone());
            folders.select_folder(Some(&window), gio::Cancellable::NONE, move |result| {
                if let Some(path) = result.ok().and_then(|f| f.path()) {
                    settings::save_meetings_dir(&path);
                    row.set_subtitle(&path.display().to_string());
                }
            });
        });
        recording.add(&meetings_row);
        page.add(&recording);
        let audio = adw::PreferencesGroup::builder().title("Audio").build();
        let audio_status = crate::helper::path().and_then(|h| crate::helper::list_info(&h));
        let mic_row = adw::ActionRow::builder().title("Microphone").build();
        let mic_subtitle = match audio_status {
            Some((_, _, inputs, _)) if inputs > 0 => {
                format!("{inputs} input devices, default follows the system")
            }
            _ => "No input device found".to_owned(),
        };
        mic_row.set_subtitle(&mic_subtitle);
        audio.add(&mic_row);
        let computer_row = adw::ActionRow::builder().title("Computer audio").build();
        match audio_status {
            Some((true, _, _, _)) => {
                computer_row.set_subtitle("Records what the Mac plays");
            }
            Some((false, Some(device), _, _)) => {
                computer_row.set_subtitle(&format!("Through {device}"));
            }
            _ => {
                computer_row.set_subtitle("Unavailable: install BlackHole");
                let install = gtk::Button::builder()
                    .label("How to set up BlackHole")
                    .valign(gtk::Align::Center)
                    .build();
                install.connect_clicked(|_| {
                    let _ = gio::AppInfo::launch_default_for_uri(
                        "https://github.com/ExistentialAudio/BlackHole",
                        None::<&gio::AppLaunchContext>,
                    );
                });
                computer_row.add_suffix(&install);
            }
        }
        audio.add(&computer_row);
        page.add(&audio);

        let menubar_group = adw::PreferencesGroup::builder().title("Menu Bar").build();
        let menubar_row = adw::SwitchRow::builder()
            .title("Show recording status")
            .subtitle("Takes effect on the next launch")
            .active(crate::models::config_value("menubar").as_deref() != Some("false"))
            .build();
        menubar_row.connect_active_notify(|row| {
            crate::models::save_config_value(
                "menubar",
                if row.is_active() { "true" } else { "false" },
            );
        });
        menubar_group.add(&menubar_row);
        page.add(&menubar_group);
        let weak = Rc::downgrade(self);
        dialog.connect_closed(move |_| {
            let Some(r) = weak.upgrade() else { return };
            r.update_model_banner();
            let saved = settings::load_language();
            if let Some(index) = LANGUAGES.iter().position(|(code, _)| *code == saved) {
                r.loading.set(true);
                r.language_row.set_selected(index as u32);
                r.again_language_row.set_selected(index as u32);
                r.loading.set(false);
            }
        });
        dialog.present(Some(&self.window));
    }

    fn selected_format(&self) -> Format {
        Format::ALL
            .get(self.format_row.selected() as usize)
            .copied()
            .unwrap_or(Format::Mono)
    }

    fn selected_language(&self) -> &'static str {
        LANGUAGES
            .get(self.language_row.selected() as usize)
            .map(|(code, _)| *code)
            .unwrap_or("auto")
    }

    fn title(&self) -> String {
        let typed = self.title_row.text().trim().to_owned();
        if typed.is_empty() {
            "Meeting".to_owned()
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
        self.compact_button.set_visible(recording);
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
        self.pause_button
            .set_label(if self.paused.get() { "Resume" } else { "Pause" });
        if recording {
            self.button.set_label("Stop recording");
            self.button.add_css_class("destructive-action");
        } else {
            self.button.set_label(match state {
                State::Stopping => "Saving…",
                State::Transcribing => "Transcribing…",
                _ => "Start recording",
            });
            self.button.add_css_class("suggested-action");
        }

        let tracks_kept = self.result_dir.borrow().as_ref().is_some_and(|dir| {
            let (mic, computer) = export::tracks(dir);
            (mic.is_file() && computer.is_file()) || source_track(dir).is_file()
        });
        self.again_button.set_sensitive(tracks_kept);
        self.again_button.set_tooltip_text(Some(if tracks_kept {
            "Transcribe the meeting again with the selected language"
        } else {
            "The separate tracks of this meeting are gone, so it cannot be transcribed again"
        }));

        let text = match state {
            State::Idle => "Ready. Press Start recording when the meeting begins.".to_owned(),
            State::Recording if self.paused.get() => {
                "Paused. Nothing is recorded until you resume.".to_owned()
            }
            State::Recording => {
                "Recording. Name, audio file and language can still be changed.".to_owned()
            }
            State::Stopping => "Saving the audio…".to_owned(),
            State::Transcribing => "Transcribing the meeting on this computer…".to_owned(),
            State::Done => String::new(),
        };
        self.status_label.set_label(&text);
        self.update_actions();
    }

    /// Shows the page for the current state, edge to edge while transcribing.
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
        // Recording and transcribing share one size, so stopping does not jump.
        self.fit_window(if page == "done" { DONE_SIZE } else { FULL_SIZE });
        let immersive = matches!(state, State::Stopping | State::Transcribing);
        self.view.set_extend_content_to_top_edge(immersive);
        if immersive {
            self.window.add_css_class("immersive");
        } else {
            self.window.remove_css_class("immersive");
        }
    }

    /// Grows the window for the wide done page and shrinks it back for the
    /// others. Only when the page asks for another size than last time, so a
    /// window the user resized is left alone otherwise.
    fn fit_window(&self, size: (i32, i32)) {
        if self.fitted.replace(size) == size {
            return;
        }
        let minimum = if size == DONE_SIZE {
            (820, 560)
        } else {
            (360, 200)
        };
        self.window.set_size_request(minimum.0, minimum.1);
        self.window.set_default_size(size.0, size.1);
        self.window.queue_resize();
    }

    fn tick(&self) {
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
        }
        // Computer capture may fall back to BlackHole or go idle (tap refused,
        // helper missing); say so under the meters until it captures again.
        match self.system.note() {
            Some(note) => {
                if self.audio_banner.title() != note {
                    self.audio_banner.set_title(&note);
                }
                self.audio_banner.set_revealed(true);
            }
            None => self.audio_banner.set_revealed(false),
        }
    }

    /// Switches between the full layout and the compact one with only the waves.
    fn set_compact(&self, compact: bool) {
        if compact == self.compact.get() || (compact && self.state.get() != State::Recording) {
            return;
        }
        self.compact.set(compact);
        let size = if compact {
            let (w, h) = (self.window.width(), self.window.height());
            if w > COMPACT_SIZE.0 && h > COMPACT_SIZE.1 {
                self.full_size.set((w, h));
            }
            self.layout.set_visible_child_name("compact");
            self.view.set_reveal_top_bars(false);
            // libadwaita keeps every window at least 360x200 unless told otherwise.
            self.window.set_size_request(COMPACT_SIZE.0, COMPACT_SIZE.1);
            COMPACT_SIZE
        } else {
            self.view.set_reveal_top_bars(true);
            self.show_page();
            self.window.set_size_request(360, 200);
            self.full_size.get()
        };
        self.window.set_default_size(size.0, size.1);
        self.window.queue_resize();
    }

    /// Recorded time so far, without the pauses.
    fn elapsed(&self) -> i64 {
        let until = if self.paused.get() {
            self.pause_began.get()
        } else {
            ipc::now()
        };
        (until - self.started_at.get() - self.paused_secs.get()).max(0)
    }

    fn toggle_pause(&self) {
        if self.state.get() != State::Recording {
            return;
        }
        if self.paused.get() {
            self.paused_secs
                .set(self.paused_secs.get() + ipc::now() - self.pause_began.get());
            self.paused.set(false);
        } else {
            self.pause_began.set(ipc::now());
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
        for meter in self.meters.iter().chain(self.compact_meters.iter()) {
            meter.queue_draw();
        }
    }

    /// Asks for the language and the number of speakers, then imports.
    fn confirm_import(self: &Rc<Self>, path: PathBuf) {
        if matches!(
            self.state.get(),
            State::Recording | State::Stopping | State::Transcribing
        ) {
            self.toast("Finish the current recording first");
            return;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dialog = adw::AlertDialog::new(Some("Import audio"), Some(&name));
        let group = adw::PreferencesGroup::new();
        let language_labels: Vec<&str> = LANGUAGES.iter().map(|(_, label)| *label).collect();
        let language = adw::ComboRow::builder()
            .title("Language")
            .model(&gtk::StringList::new(&language_labels))
            .selected(self.language_row.selected())
            .build();
        let speakers = adw::ComboRow::builder()
            .title("Speakers")
            .subtitle("Recognized by their voices")
            .model(&gtk::StringList::new(&SPEAKER_CHOICES))
            .build();
        group.add(&language);
        group.add(&speakers);
        dialog.set_extra_child(Some(&group));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("import", "Import");
        dialog.set_response_appearance("import", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("import"));
        dialog.set_close_response("cancel");
        let this = self.clone();
        dialog.connect_response(None, move |_, response| {
            if response != "import" {
                return;
            }
            let code = LANGUAGES
                .get(language.selected() as usize)
                .map(|(code, _)| *code)
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
            .unwrap_or_else(|| "Imported audio".to_owned());
        // The file's own date says when the meeting was, better than now.
        let started_at = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or_else(ipc::now, |d| d.as_secs() as i64);
        let mut out = output_dir(started_at, &title);
        let mut n = 2;
        while out.exists() {
            out = output_dir(started_at, &format!("{title} {n}"));
            n += 1;
        }

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
            chapters: Vec::new(),
            chapters_by: None,
        });
        self.animation_since.set(Some(std::time::Instant::now()));
        self.animation.reset();
        self.animation.set_stage("Importing audio");
        self.animation.set_running(true);
        self.set_state(State::Stopping);

        let this = self.clone();
        glib::spawn_future_local(async move {
            let (source, target) = (path.clone(), out.clone());
            let staging = crate::paths::cache().join(format!("import-{}", ipc::now()));
            let converted = gio::spawn_blocking(move || import_audio(&source, &target, &staging))
                .await
                .unwrap_or_else(|_| Err("the import stopped unexpectedly".into()));
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
        let note = read_recording_note(&staging);
        let started_at = note.as_ref().map_or(0, |n| n.started_at);
        let when = glib::DateTime::from_unix_local(started_at)
            .and_then(|t| t.format("%A %H:%M"))
            .map(|s| s.to_string())
            .unwrap_or_default();
        let length = format_elapsed(raw_duration(&staging));
        let dialog = adw::AlertDialog::new(
            Some("Unfinished recording found"),
            Some(&format!(
                "A recording from {when} ({length}) was not stopped properly, probably because the app quit. Save it as a meeting?"
            )),
        );
        dialog.add_response("discard", "Discard");
        dialog.add_response("later", "Later");
        dialog.add_response("save", "Save");
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
            self.toast("Finish the current recording first");
            return;
        }
        let note = note.unwrap_or_else(|| RecordingNote {
            title: "Recovered recording".to_owned(),
            started_at: ipc::now() - raw_duration(&staging),
            format: settings::load_format(),
            language: settings::load_language().to_owned(),
        });
        self.title_row.set_text(&note.title);
        if let Some(i) = Format::ALL.iter().position(|f| *f == note.format) {
            self.format_row.set_selected(i as u32);
        }
        if let Some(i) = LANGUAGES
            .iter()
            .position(|(code, _)| *code == note.language)
        {
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
        match crate::models::missing() {
            Some((name, size_mb)) => {
                let size = if size_mb >= 1000 {
                    format!("{:.1} GB", f64::from(size_mb) / 1000.0)
                } else {
                    format!("{size_mb} MB")
                };
                self.model_banner.set_title(&format!(
                    "The speech model ({name}, {size}) is needed to transcribe"
                ));
                self.model_banner.set_button_label(Some("Download"));
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
        self.model_banner.set_title("Downloading the speech model…");
        let (events_tx, events_rx) = async_channel::unbounded::<Event>();
        let (done_tx, done_rx) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let result = crate::models::ensure(&events_tx, &Abort::default());
            let _ = done_tx.send_blocking(result);
            let _ = events_tx.send_blocking(Event::Finished);
        });
        let this = self.clone();
        glib::spawn_future_local(async move {
            while let Ok(event) = events_rx.recv().await {
                match event {
                    Event::Progress(progress) => this.model_banner.set_title(&format!(
                        "Downloading the speech model… {:.0}%",
                        progress * 100.0
                    )),
                    Event::Finished => break,
                    _ => {}
                }
            }
            let result = done_rx
                .recv()
                .await
                .unwrap_or_else(|_| Err("the download stopped".into()));
            this.model_downloading.set(false);
            match result {
                Ok(_) => this.toast("Speech model ready"),
                Err(message) => this.toast(&format!("Could not download the model: {message}")),
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
                .and_then(|now| now.format("Meeting %H:%M"))
                .map(|s| s.to_string())
                .unwrap_or_else(|_| "Meeting".to_owned());
            self.title_row.set_text(&title);
        }
        self.paused.set(false);
        self.paused_secs.set(0);
        self.pause_began.set(0);
        let started_at = ipc::now();
        let staging = crate::paths::cache().join(started_at.to_string());
        if let Err(e) = std::fs::create_dir_all(&staging)
            .and_then(|_| self.mic.start_recording(&staging.join("mic.raw")))
            .and_then(|_| self.system.start_recording(&staging.join("system.raw")))
        {
            self.mic.stop_recording();
            self.system.stop_recording();
            self.status_label
                .set_label(&format!("Could not start recording: {e}"));
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
        self.started_at.set(started_at);
        self.timer.set_label("00:00");
        self.compact_timer.set_label("00:00");
        self.set_state(State::Recording);
        // Keep the Mac from idle-sleeping while recording. `-w` ties the
        // assertion to this process, so it dies with the app even on a crash.
        if let Ok(child) = std::process::Command::new("caffeinate")
            .args(["-i", "-w", &std::process::id().to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            *self.caffeinate.borrow_mut() = Some(child);
        }
    }

    fn stop(self: &Rc<Self>) {
        if self.state.get() != State::Recording {
            return;
        }
        if self.paused.get() {
            self.paused_secs
                .set(self.paused_secs.get() + ipc::now() - self.pause_began.get());
            self.paused.set(false);
        }
        self.freeze_meters(false);
        self.animation_since.set(Some(std::time::Instant::now()));
        self.mic.stop_recording();
        self.system.stop_recording();
        if let Some(mut caffeinate) = self.caffeinate.borrow_mut().take() {
            let _ = caffeinate.kill();
            let _ = caffeinate.wait();
        }
        let Some(staging) = self.staging.borrow().clone() else {
            return;
        };
        self.set_compact(false);
        // Straight to the animation: the waves would suggest it is still recording.
        self.animation.reset();
        self.animation.set_stage("Saving audio");
        self.animation.set_running(true);
        self.set_state(State::Stopping);

        let format = self.selected_format();
        let language = self.selected_language();
        let out = output_dir(self.started_at.get(), &self.title());
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
                    meeting::DEFAULT_REMOTE.to_owned(),
                ],
                imported: None,
                speaker_count: None,
                model: None,
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
            return Err("no meeting folder".into());
        };
        self.set_compact(false);
        if self.animation_since.get().is_none() {
            self.animation_since.set(Some(std::time::Instant::now()));
        }
        self.set_state(State::Transcribing);
        self.animation.reset();
        self.animation.set_stage("Loading audio");
        self.animation.set_progress(0.0);
        self.animation.set_running(true);

        let abort = Abort::default();
        *self.abort.borrow_mut() = Some(abort.clone());
        let (events_tx, events_rx) = async_channel::unbounded::<Event>();
        let (done_tx, done_rx) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let result = match tracks {
                Tracks::Single(path, speakers) => transcribe::load_track(&path).and_then(|track| {
                    transcribe::transcribe_single(&track, language, speakers, &events_tx, &abort)
                }),
                Tracks::Raw(dir) | Tracks::Kept(dir) => {
                    let (mic_path, computer_path) = if dir.join("mic.raw").exists() {
                        (dir.join("mic.raw"), dir.join("system.raw"))
                    } else {
                        export::tracks(&dir)
                    };
                    transcribe::load_track(&mic_path).and_then(|mic| {
                        let computer = transcribe::load_track(&computer_path)?;
                        transcribe::transcribe(&mic, &computer, language, &events_tx, &abort)
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
            .unwrap_or_else(|_| Err("transcription stopped unexpectedly".into()));
        *self.abort.borrow_mut() = None;

        let transcript = result?;
        // The name as it is now; it may have been edited while transcribing.
        let out = self.result_dir.borrow().clone().unwrap_or(out);
        let date = glib::DateTime::from_unix_local(self.started_at.get())
            .and_then(|t| t.format("%Y-%m-%d %H:%M"))
            .map(|s| s.to_string())
            .unwrap_or_default();
        let mut markdown = transcribe::to_markdown(&self.title(), &date, &transcript);
        if let Some(manifest) = self.manifest.borrow_mut().as_mut() {
            // An import finds its own number of speakers; keep names already
            // given and number the rest.
            if manifest.imported.is_some() {
                let found = speakers_in(&markdown);
                let mut names = manifest.speakers.clone();
                names.resize_with(found.len(), String::new);
                for (i, name) in names.iter_mut().enumerate() {
                    if name.is_empty() {
                        *name = format!("Speaker {}", i + 1);
                    }
                }
                manifest.speakers = names;
            } else {
                // Several voices on the computer audio come out as Remote 1,
                // Remote 2, ...: one name each, after your own.
                let remotes = speakers_in(&markdown)
                    .iter()
                    .filter_map(|s| s.strip_prefix("Remote ")?.parse::<usize>().ok())
                    .max()
                    .unwrap_or(0);
                if remotes > 1 {
                    let mut names = manifest.speakers.clone();
                    if names.len() <= 2 {
                        names.truncate(1);
                    }
                    while names.len() < remotes + 1 {
                        let n = names.len();
                        names.push(format!("Remote {n}"));
                    }
                    names.truncate(remotes + 1);
                    manifest.speakers = names;
                } else if manifest.speakers.len() > 2 {
                    manifest.speakers.truncate(2);
                    manifest.speakers[1] = meeting::DEFAULT_REMOTE.to_owned();
                }
            }
            // The transcription labels speakers You/Remote or Speaker N; use
            // the names of this meeting.
            let renames: Vec<(String, String)> = manifest
                .default_labels()
                .into_iter()
                .zip(manifest.speakers.iter().cloned())
                .filter(|(label, name)| label != name)
                .collect();
            markdown = meeting::relabel_all(&markdown, &renames);
            manifest.language = language.to_owned();
            manifest.model = Some(crate::models::configured());
            manifest.title = self.title();
            // Chapters of a previous transcript would point at lines that are gone.
            manifest.chapters.clear();
            manifest.chapters_by = None;
            let _ = meeting::write(&out, manifest);
        }
        std::fs::write(out.join("transcript.md"), markdown)
            .map_err(|e| format!("could not write the transcript: {e}"))
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
                self.animation.set_stage("Done");
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
            (Err(message), _) if message == CANCELLED => {
                Some("Transcription cancelled.".to_owned())
            }
            (Err(message), _) => Some(format!("Transcription failed: {message}.")),
            (Ok(()), false) => Some("Could not save the audio.".to_owned()),
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
            let note = gio::Notification::new("Meeting transcribed");
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
            self.toast("Finish the current recording first");
            return;
        }
        let Some((dir, manifest)) = meeting::open(path) else {
            self.toast("This is not a meeting the recorder can open");
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
        if let Some(i) = LANGUAGES
            .iter()
            .position(|(code, _)| *code == manifest.language)
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
        let problem = text
            .is_none()
            .then_some("This meeting has no transcript yet.");
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
            "Transcript ready"
        } else {
            "Meeting saved"
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
        self.chapters_button
            .set_label(if has_chapters { "Redo" } else { "Generate" });
        let description = match &agent {
            Some(agent) if self.generating.get() => {
                format!("Writing chapters with {}…", agent.name)
            }
            Some(agent) if has_chapters => format!("Made with {}", agent.name),
            Some(agent) if self.can_have_chapters() => {
                format!("Let {} divide the meeting into chapters", agent.name)
            }
            Some(_) => "For meetings of three minutes or more".to_owned(),
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
            .unwrap_or_else(|_| Err("the agent stopped unexpectedly".into()));
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
                    this.chapters_group
                        .set_description(Some(&format!("{} could not make chapters", agent.name)));
                }
            }
        });
    }

    fn store_chapters(self: &Rc<Self>, dir: &std::path::Path, list: &[Chapter], agent: &Agent) {
        if let Some(manifest) = self.manifest.borrow_mut().as_mut() {
            manifest.chapters = list.to_vec();
            manifest.chapters_by = Some(agent.id.clone());
            let _ = meeting::write(dir, manifest);
        }
        let transcript = dir.join("transcript.md");
        if let Ok(text) = std::fs::read_to_string(&transcript) {
            let _ = std::fs::write(&transcript, chapters::apply_to_markdown(&text, list));
        }
        let text = std::fs::read_to_string(&transcript).ok();
        self.show_transcript(text.as_deref(), None);
        self.toast(&format!("{} chapters added", list.len()));
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
        let edit = action("document-edit-symbolic", "Edit this line");
        let swap = action("object-flip-horizontal-symbolic", "Next speaker");
        let delete = action("user-trash-symbolic", "Delete this line");
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
            .tooltip_text(format!("Play from {}", paragraph.time))
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
            self.toast("Could not save the transcript");
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
            self.toast("Saved");
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
            .title("Line deleted")
            .button_label("Undo")
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
                (false, 0) => "Speaker on the microphone".to_owned(),
                (false, _) if manifest.speakers.len() > 2 => {
                    format!("Speaker {i} on the computer audio")
                }
                (false, _) => "Speaker on the computer audio".to_owned(),
                (true, _) => format!("Speaker {}", i + 1),
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
                    labels
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| format!("Speaker {}", i + 1))
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
            self.toast("Every speaker needs a different name");
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
        if you_changed && let Some(you) = names.first() {
            settings::save_your_name(you);
        }
        let text = std::fs::read_to_string(&transcript).ok();
        self.redraw_transcript(text.as_deref().unwrap_or(""));
        self.toast("Saved");
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
        let target = output_dir(self.started_at.get(), &title);
        // "202609241400 Weekly 2" is still the folder of "Weekly": an import
        // got a number when the name was taken. Only a new name renames.
        let renamed = !folder_is_for(&current, &target);
        if renamed {
            if target.exists() {
                self.toast("A meeting folder with that name already exists");
                return;
            }
            if let Err(e) = std::fs::rename(&current, &target) {
                self.toast(&format!("Could not rename the folder: {e}"));
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
        self.toast("Saved");
    }

    fn close_when_done(&self) {
        self.quit_when_done.set(true);
        self.window.set_visible(false);
    }

    fn confirm_close_recording(self: &Rc<Self>) {
        let dialog = adw::AlertDialog::new(
            Some("Still recording"),
            Some(
                "Closing stops the meeting. The audio is saved and transcribed first, then the app quits.",
            ),
        );
        dialog.add_response("keep", "Keep recording");
        dialog.add_response("stop", "Stop and close");
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
            Some("Still transcribing"),
            Some("The audio is already saved. The transcript is not finished yet."),
        );
        dialog.add_response("keep", "Keep open");
        dialog.add_response("later", "Close when done");
        dialog.add_response("cancel", "Cancel transcription");
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

fn output_dir(started_at: i64, title: &str) -> PathBuf {
    let stamp = glib::DateTime::from_unix_local(started_at)
        .and_then(|t| t.format("%Y%m%d%H%M"))
        .map(|s| s.to_string())
        .unwrap_or_default();
    let root = settings::load_meetings_dir().unwrap_or_else(crate::paths::meetings);
    root.join(format!("{stamp} {}", safe_name(title)))
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
        .tooltip_text(format!("Play from {}", chapters::clock(start_ms)))
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
        return Err("this file has no audio ffmpeg can read".into());
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
        return Err("could not convert the audio".into());
    }
    Ok((bytes / (48_000 * 2 * 2)) as i64)
}

/// What is known about a recording in progress, next to its audio, so it can
/// be finished after a crash.
#[derive(Clone)]
struct RecordingNote {
    title: String,
    started_at: i64,
    format: Format,
    language: String,
}

fn write_recording_note(
    staging: &std::path::Path,
    title: &str,
    started_at: i64,
    format: Format,
    language: &str,
) {
    let note = serde_json::json!({
        "title": title,
        "started_at": started_at,
        "format": format.key(),
        "language": language,
    });
    let _ = std::fs::write(staging.join("recording.json"), note.to_string());
}

fn read_recording_note(staging: &std::path::Path) -> Option<RecordingNote> {
    let text = std::fs::read_to_string(staging.join("recording.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(RecordingNote {
        title: value["title"]
            .as_str()
            .filter(|t| !t.is_empty())?
            .to_owned(),
        started_at: value["started_at"].as_i64()?,
        format: Format::from_key(value["format"].as_str().unwrap_or("mono")),
        language: value["language"].as_str().unwrap_or("auto").to_owned(),
    })
}

/// Recording staging folders left behind, with some audio in them.
fn unfinished_recordings() -> Vec<PathBuf> {
    let root = crate::paths::cache();
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

/// Recorded seconds in a staging folder, from the size of the longer raw track.
fn raw_duration(staging: &std::path::Path) -> i64 {
    let bytes = ["mic.raw", "system.raw"]
        .iter()
        .map(|name| std::fs::metadata(staging.join(name)).map_or(0, |m| m.len()))
        .max()
        .unwrap_or(0);
    (bytes / (48_000 * 2 * 2)) as i64
}

/// The speaker labels in transcript Markdown, in order of first appearance.
fn speakers_in(markdown: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for line in markdown.lines() {
        if let Some((_, speaker, _)) = parse_segment(line)
            && !found.iter().any(|f| f == speaker)
        {
            found.push(speaker.to_owned());
        }
    }
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

/// Splits `**[01:23] You:** text` into its time, speaker and text.
fn parse_segment(line: &str) -> Option<(&str, &str, &str)> {
    let rest = line.strip_prefix("**[")?;
    let (time, rest) = rest.split_once("] ")?;
    let (speaker, text) = rest.split_once(":** ")?;
    Some((time, speaker, text.trim()))
}

/// The few styles libadwaita does not have: a see-through header bar over the
/// animation, and the transcript card.
fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        "window.immersive { background: #0d0826; }
         window.immersive headerbar { background: transparent; box-shadow: none; color: #f8f5f2; }
         window.immersive headerbar button { color: #f8f5f2; }
         .transcript { background: transparent; padding: 8px; }
         .transcript row { padding: 7px 10px; border-radius: 8px; }
         .transcript row:hover { background: alpha(currentColor, 0.06); }
         .transcript row.current { background: alpha(@accent_bg_color, 0.25); }
         .transcript row.chapter { padding-top: 16px; }
         .transcript row.chapter:first-child { padding-top: 7px; }
         .chapter-heading { font-weight: 800; font-size: 1.08em; }
         .speaker-0, .speaker-1, .speaker-2, .speaker-3, .speaker-4, .speaker-5 { font-weight: 700; }
         .speaker-0 { color: #5a9cf0; } .speaker-1 { color: #f08a3a; } .speaker-2 { color: #57c27a; }
         .speaker-3 { color: #d066c8; } .speaker-4 { color: #3fc4cf; } .speaker-5 { color: #d9b53a; }
         .drop-hint { margin: 10px; border: 3px dashed @accent_color; border-radius: 14px;
                      background: alpha(@window_bg_color, 0.88); color: @accent_color; }
         .row-actions { opacity: 0; transition: opacity 120ms; }
         .transcript row:hover .row-actions, .transcript row.editing .row-actions { opacity: 1; }
         .row-actions button { min-height: 24px; min-width: 24px; padding: 2px; }
         .transcript-editor { background: alpha(currentColor, 0.06); border-radius: 6px; padding: 4px 6px; }
         .transcript-editor text { background: transparent; }
         .done-icon { color: @accent_color; }
         .player { padding: 6px 14px 6px 6px; }",
    );
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

pub fn safe_name(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '-'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches(|c| c == ' ' || c == '.');
    if trimmed.is_empty() {
        "Meeting".to_owned()
    } else {
        trimmed.to_owned()
    }
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
                let text = "Not recording";
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
            let _ = cr.show_text("PAUSED");
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
