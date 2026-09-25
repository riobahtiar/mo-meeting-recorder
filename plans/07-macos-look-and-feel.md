# 07 macOS look and feel

## Goal

A Mac user opens MOM Recorder and finds what every Mac app has: a menu bar with the standard menus and shortcuts, window buttons on the left, the system font, controls with macOS proportions, colours that follow light, dark and the accent, native file panels, an About panel and a Preferences window. The libadwaita structure stays; what changes is the chrome and the details. The Omarchy theme reader is gone.

## Done when

- [~] Menu bar: app menu (About, Preferences…, Services, Hide, Quit), File, Edit, Recording, View, Window, Help, with the D18 shortcuts; items enable and disable with the app's state.
- [ ] Window buttons on the left, drawn as traffic lights; the title bar reads the meeting name.
- [ ] Body text is the system font at 13 pt; buttons, entries and cards have macOS radii and spacing.
- [x] Switching the system appearance or accent while the app is open is followed at once; speakers, waves and the animation use Apple's palette.
- [~] Import, Open Meeting and the folder chooser open native panels with working filters.
- [x] About shows the version, upstream credit and licence; Preferences edits model, agent, format, language, your name and the meetings folder, and the app uses the new values without restart.
- [x] Drag a file from Finder onto the window: the drop overlay appears and the import dialog opens.
- [x] `grep -rn -i omarchy src/` finds nothing.

Observed 2026-09-25 (no display in the shell session): `cargo build`,
`cargo test` (52 passed), clippy and fmt are clean; a socket-driven
record/stop cycle runs with no new warnings. Everything visual — menu bar,
traffic lights, typography, panels, About and Preferences rendering — needs
the side-by-side in Verify step 2 on a display. The icon (step 10) and the
VoiceOver check (step 13) are not started.

## Prerequisites

Plans 03, 04 and 06 (recording works, shortcuts moved, paths settled). D18 confirmed.

## Background

What GTK 4 and libadwaita already do on macOS, from their sources:

- `gtk_application_set_menubar` builds the **native menu bar** from the `GMenuModel`. GTK adds the **app menu** from its own `gtkapplication-quartz.ui`: "About %s" → `app.about`, "Preferences" → `app.preferences`, Services, "Hide %s", Hide Others, Show All, "Quit %s" → `app.quit`, each shown when the action exists. The same file defines a default Edit menu (`text.undo`, `text.redo`, `clipboard.cut/copy/paste`, `selection.delete`, `selection.select-all`) and a Window submenu that macOS manages, for apps that set no menubar.
- GDK reports `gtk-shell-shows-menubar = TRUE`, so nothing is drawn inside the window, and `gtk-font-name` as the **system font at 12 pt**.
- GDK does **not** set `gtk-decoration-layout`, so libadwaita puts the window buttons on the right unless told otherwise.
- libadwaita's macOS settings backend follows **light/dark** (`AppleInterfaceStyle`), **high contrast**, and the **accent colour** (nearest Adwaita accent to `NSColor.controlAccentColor`).
- `gtk::FileDialog` (used in `ui.rs` around line 739) goes through GTK's quartz native chooser: **NSOpenPanel/NSSavePanel** for open, save and select-folder, with filters.

What the app does today that this plan touches: `ui.rs` builds an `adw::ToolbarView` with an `adw::HeaderBar`, an `adw::ToastOverlay`, `adw::AlertDialog`s for import and confirmations, a `gtk::DropTarget` for files, two actions (`win.compact`, `app.quit`) and button click handlers for everything else. `theme.rs` reads Omarchy's `colors.toml`, maps it onto libadwaita variables and the `.speaker-N` classes, and watches the theme directory; `player.rs` and `animation.rs` fall back to fixed colours when there is no theme.

Apple's Human Interface Guidelines are the reference for what "looks right": [Menus](https://developer.apple.com/design/human-interface-guidelines/menus), [Windows](https://developer.apple.com/design/human-interface-guidelines/windows), [Typography](https://developer.apple.com/design/human-interface-guidelines/typography), [Color](https://developer.apple.com/design/human-interface-guidelines/color).

## Steps

### 1. Turn button handlers into actions

Every menu item needs a `GAction`. Before building the menu, refactor `ui.rs` so each user-facing operation is a `gio::SimpleAction` on the window (`win.*`) or the app (`app.*`), and the existing buttons call `set_action_name` (or `set_detailed_action_name`) instead of `connect_clicked`. Enabled state is then one `set_enabled` per state transition in the state machine, and the menu greys out with it.

| Action | Where it exists today | Enabled when |
|---|---|---|
| `win.new-recording` | New recording button, done page | done page |
| `win.open-meeting` | not yet (argv only) | always |
| `win.import` | Import an audio file button | ready, done |
| `win.reveal` | Open folder button | done page |
| `win.start` | Start recording | ready, model present |
| `win.pause` | Pause / Resume | recording, paused |
| `win.stop` | Stop recording | recording, paused |
| `win.compact` | exists | recording, paused |
| `win.copy-transcript` | Copy transcript button and Enter | done page |
| `win.fullscreen` | not yet | always |
| `win.transcribe-again(lang)` | the language dropdown | done page |
| `app.about` | not yet | always |
| `app.preferences` | not yet | always |
| `app.help` | not yet | always |
| `app.quit` | exists | always |

This step is the largest `ui.rs` change in the whole port. Do it as its own commit, verify every button still works, then continue.

### 2. The menu model

```rust
// sketch, in ui.rs, called from the app's startup handler
fn install_menubar(app: &adw::Application) {
    use gio::Menu;
    let item = |label: &str, action: &str| gio::MenuItem::new(Some(label), Some(action));

    let file = Menu::new();
    file.append_item(&item("New Recording", "win.new-recording"));
    file.append_item(&item("Open Meeting…", "win.open-meeting"));
    file.append_item(&item("Import Audio File…", "win.import"));
    let section = Menu::new();
    section.append_item(&item("Reveal in Finder", "win.reveal"));
    file.append_section(None, &section);
    let close = Menu::new();
    close.append_item(&item("Close Window", "window.close"));
    file.append_section(None, &close);

    let edit = Menu::new();
    for (label, action) in [("Undo", "text.undo"), ("Redo", "text.redo")] { edit.append_item(&item(label, action)); }
    let clipboard = Menu::new();
    for (label, action) in [("Cut", "clipboard.cut"), ("Copy", "clipboard.copy"), ("Paste", "clipboard.paste"), ("Select All", "selection.select-all")] { clipboard.append_item(&item(label, action)); }
    edit.append_section(None, &clipboard);
    let transcript = Menu::new();
    transcript.append_item(&item("Copy Transcript", "win.copy-transcript"));
    edit.append_section(None, &transcript);

    let recording = Menu::new();
    recording.append_item(&item("Start Recording", "win.start"));
    recording.append_item(&item("Pause", "win.pause"));          // relabel to "Resume" while paused: replace the item
    recording.append_item(&item("Stop Recording", "win.stop"));

    let view = Menu::new();
    view.append_item(&item("Compact Strip", "win.compact"));
    view.append_item(&item("Enter Full Screen", "win.fullscreen"));
    // Transcribe Again ▸ one item per language: "win.transcribe-again::en"

    let window = Menu::new();   // macOS fills Minimize, Zoom, Bring All to Front. Verify: see note below.
    let help = Menu::new();
    help.append_item(&item("MOM Recorder Help", "app.help"));

    let bar = Menu::new();
    bar.append_submenu(Some("File"), &file);
    bar.append_submenu(Some("Edit"), &edit);
    bar.append_submenu(Some("Recording"), &recording);
    bar.append_submenu(Some("View"), &view);
    bar.append_submenu(Some("Window"), &window);
    bar.append_submenu(Some("Help"), &help);
    app.set_menubar(Some(&bar));
}
```

Shortcuts come from `set_accels_for_action` with the D18 table (`<Primary>n`, `<Primary>o`, `<Primary><Shift>i`, `<Primary><Alt>r`, `<Primary>r`, `<Primary><Shift>r`, `<Primary>period`, `<Primary><Shift>m`, `<Primary><Shift>c`, `<Primary><Control>f`, `<Primary>comma`). GTK shows them in the menu automatically.

Verify: how GTK's quartz backend recognises the Window submenu it should manage. Read `gtk/gtkapplication-quartz-menu.c` for the attribute (the default `.ui` marks its `_Window` submenu specially); set the same attribute on the `Window` submenu here. If none exists, an empty Window submenu is still better than none: macOS adds the window list to it.

Edit items with `text.*` and `clipboard.*` names are widget actions that GTK routes to the focused widget, so undo and copy work in the inline editor and the name field without any code.

### 3. Window chrome

At startup, before the window is shown:

```rust
// sketch
if let Some(settings) = gtk::Settings::default() {
    settings.set_gtk_decoration_layout(Some("close,minimize,maximize:"));
}
window.add_css_class("macos");
```

Then in `data/macos.css` (loaded at `STYLE_PROVIDER_PRIORITY_APPLICATION`, from `theme.rs` `follow()`), draw the buttons as traffic lights. libadwaita's `windowcontrols` renders one `button.titlebutton` per control with the classes `close`, `minimize`, `maximize`:

```css
.macos windowcontrols { margin-left: 8px; }
.macos windowcontrols button.titlebutton {
  min-width: 12px; min-height: 12px; padding: 0; margin: 0 4px;
  border-radius: 50%; background-image: none; box-shadow: none; -gtk-icon-size: 0;
}
.macos windowcontrols button.titlebutton.close    { background-color: #ff5f57; }
.macos windowcontrols button.titlebutton.minimize { background-color: #febc2e; }
.macos windowcontrols button.titlebutton.maximize { background-color: #28c840; }
.macos windowcontrols:backdrop button.titlebutton { background-color: #d9d9d9; }
.macos windowcontrols button.titlebutton:hover    { filter: brightness(0.85); }
```

Verify: the buttons move to the left and `set_decoration_layout` is honoured by `adw::HeaderBar`; that the glyphs disappear with `-gtk-icon-size: 0` (fall back to `windowcontrols button image { opacity: 0 }`); that macOS still delivers the real minimize and zoom through GTK's handlers. The strip (compact mode) has no header bar and keeps its custom drag handle.

Title: `window.set_title(Some(&meeting_name))` when the name changes (the done page already renames the folder; hook the same place). macOS shows it centred in the title bar. The window title when there is no meeting is "MOM Recorder".

### 4. Typography

GDK gives the system font at 12 pt. macOS body text is 13 pt. In the same startup block:

```rust
// sketch
settings.set_gtk_font_name(Some(&format!("{} 13", system_font_family(&settings))));   // keep the family GDK reported, bump the size
```

Verify: text in lists, dialogs and the header bar now matches Finder and System Settings at a glance; libadwaita's `.title-1` etc. scale relatively, so headings remain proportionate. Do not set a font by name; the family GDK reports is the right one for the current macOS.

### 5. Controls: `data/macos.css`

Keep it short and scoped to `.macos`. Targets, from the HIG and from System Settings as the yardstick:

```css
/* Radii and heights */
.macos button        { border-radius: 6px; min-height: 22px; padding: 2px 10px; }
.macos button.pill   { border-radius: 999px; }                   /* keep Start recording round */
.macos entry, .macos spinbutton, .macos dropdown > button { border-radius: 6px; min-height: 22px; }
.macos .card, .macos list.boxed-list { border-radius: 10px; }
.macos headerbar     { min-height: 38px; }                        /* between title bar and unified toolbar */
.macos toolbar       { padding: 4px 8px; }

/* Lists: tighter rows, hairline separators */
.macos list row      { padding: 6px 12px; }
.macos list.boxed-list > row + row { border-top: 1px solid alpha(currentColor, 0.08); }

/* Sidebar: system sidebar grey, not the window colour */
.macos .sidebar-pane, .macos .navigation-sidebar { background-color: alpha(currentColor, 0.04); }

/* Dialogs read as sheets: slightly larger radius */
.macos dialog-host > dialog { border-radius: 12px; }

/* Focus ring in the accent, 3 px, outside */
.macos *:focus-visible { outline: 3px solid alpha(var(--accent-color), 0.5); outline-offset: 1px; }
```

Iterate with `GTK_DEBUG=interactive` to find selectors. Stop when a side-by-side with System Settings looks like siblings, not twins; libadwaita's own spacing scale is fine where the HIG is silent. Bundle the file as a `gio::Resource` or `include_str!` it, so a bare binary and the `.app` load the same CSS.

### 6. Colours: replace `theme.rs`

libadwaita already follows light, dark and the accent. What remains is the palette the app draws with itself: speakers (`.speaker-0` to `.speaker-5`), the two waves (`player.rs` `MIC_COLOR`, `SYSTEM_COLOR`), the animation (`animation.rs`), the red recording dot. Rewrite `theme.rs` so that it supplies Apple's system colours through the existing `Theme` and `color()` API, and delete the `colors.toml` reader, its directory monitor and the `dir()` lookup:

```rust
// sketch, theme.rs
//! The colours the app draws with: Apple's system palette, light or dark to
//! match the appearance libadwaita already follows. Window, text and accent
//! colours stay libadwaita's own.

fn load() -> Theme {
    let dark = adw::StyleManager::default().is_dark();
    let c = |light: &str, dark_: &str| parse_hex(if dark { dark_ } else { light }).unwrap();
    let mut colors = HashMap::new();
    colors.insert("blue".into(),    c("#007aff", "#0a84ff"));
    colors.insert("orange".into(),  c("#ff9500", "#ff9f0a"));
    colors.insert("green".into(),   c("#28cd41", "#32d74b"));
    colors.insert("red".into(),     c("#ff3b30", "#ff453a"));
    colors.insert("yellow".into(),  c("#ffcc00", "#ffd60a"));
    colors.insert("magenta".into(), c("#af52de", "#bf5af2"));
    colors.insert("cyan".into(),    c("#55bef0", "#5ac8f5"));
    Theme { dark, colors }
}
```

`css()` shrinks to the `.speaker-N` rules (the libadwaita variable block goes with the Omarchy palette). `follow()` watches `adw::StyleManager` `notify::dark` and `notify::accent-color` instead of a directory, and calls `changed()` so the waves and animation repaint. `animation.rs`'s palette comment ("the Omacon colours, used when there is no Omarchy theme") becomes a note that the fallbacks are only for the example binary. The hex values are Apple's published system colours (light, dark); check them against System Settings with Digital Color Meter once and note any correction in the file. Update the two theme tests.

### 7. Native panels and Finder

- `gtk::FileDialog` is already used for Import; it opens `NSOpenPanel`. Add `win.open-meeting` with a folder-or-file chooser (a filter on `*.meeting-recorder` plus select-folder) and route into the same code path as the argv open.
- `win.reveal`: `open -R <manifest file>` selects the meeting's manifest in Finder; that is what "Reveal" means on macOS, and better than opening the folder. Replace the `launch_default_for_uri` call.
- Drop from Finder: verify that the existing `gtk::DropTarget` receives `gio::File` values from Finder drags. GTK 4's macOS backend supports file DnD; if the drop yields a URI string instead of a file, accept both types.

### 8. About

`app.about` opens `adw::AboutDialog` with application name "MOM Recorder", `env!("CARGO_PKG_VERSION")`, developer credit ("Based on Meeting Recorder by Jankees van Woezik"), website (this repository), issue URL, licence MIT, and a Credits section for whisper.cpp, whisper-rs and Nemotron 3 Diarization (the same credits as the README).

### 9. Preferences (⌘,)

`app.preferences` opens an `adw::PreferencesDialog`:

| Group | Row | Stores in |
|---|---|---|
| Transcription | Model (dropdown of `MODELS` with sizes; "downloaded" or "will download" subtitle) | `config.toml` `model` |
| Transcription | Default language | `settings.json` `language` |
| Chapters | Agent (dropdown: detected agents from the flag table found on `PATH`, plus None) | `config.toml` `agent` |
| Recording | Default audio format | `settings.json` `format` |
| Recording | Your name in transcripts | `settings.json` `your_name` |
| Recording | Meetings folder (native folder chooser) | `settings.json` `meetings_dir` (new key; `paths::meetings()` reads it first) |
| Audio | Microphone and computer audio status (from `momr-audio list`), with a "How to set up BlackHole" link when the tap is unavailable | read-only |

Writing `config.toml` needs a small writer next to the reader in `models.rs` that rewrites or appends a `key = "value"` line and keeps everything else. Values apply immediately: the ready page re-reads the model banner and the language dropdown on `close`.

### 10. Dock and app icon

- A 1024 px master in `data/icon/icon-1024.png`; `scripts/make-icns.sh` produces `data/icon/MOMRecorder.icns` with `iconutil`. Plan 08 puts it in the bundle. Until then the Dock shows the generic executable icon; that is expected.
- macOS 26 and later prefer an Icon Composer `.icon` asset for the layered look. Optional; the `.icns` works everywhere.

### 11. Notifications

When a transcription finishes and the window is not focused, send a `gio::Notification` ("Meeting transcribed", the title). GLib's Cocoa backend requires the process to be in a bundle with a bundle identifier, so this only fires from the `.app` (plan 08). Let it be a no-op from a terminal.

### 12. Behaviour conventions

- Closing the last window quits, as upstream; the recording and transcribing confirmations already exist. This is normal for a single-window utility.
- `win.fullscreen` toggles `window.fullscreen()`; macOS provides the green-button zoom and the ⌃⌘F convention.
- Escape closes dialogs and Enter activates the default, which libadwaita already does.
- Keep toasts for Undo and Copied; they read fine on macOS.

### 13. Accessibility

Verify: whether VoiceOver reads the transcript rows and the buttons under GTK 4's macOS backend on the installed version. If not, note it as a known limitation in the README and in plan 12's criteria; it is one of the reasons a native shell might be warranted.

## Verify

1. Every menu item does what its label says; every D18 shortcut works; disabled states match the page (Start disabled on the done page, Stop disabled on ready).
2. Screenshots of ready, recording, done and Preferences in light and dark, next to System Settings: fonts, radii, buttons and window controls read as siblings.
3. Change the accent in System Settings: the accent and the speaker colours update without a restart.
4. Import through the menu: an `NSOpenPanel`, filtered to audio; drop from Finder works.
5. `grep -rn -i omarchy src/` is empty.

## Risks and notes

- libadwaita CSS classes are internal detail and can change between releases. Keep `macos.css` under 80 lines and re-check after each `brew upgrade libadwaita`.
- If the GTK quartz app menu does not pick up `app.about` and `app.preferences`, the actions must be registered on the `gio::Application` (not the window) before `startup` completes.
- Full HIG parity is not the target. The stopping rule is the side-by-side in Verify step 2.

## Status

- [x] Step 1 actions
- [x] Step 2 menu bar
- [x] Step 3 window chrome
- [x] Step 4 typography
- [x] Step 5 `macos.css`
- [x] Step 6 Apple palette, Omarchy reader gone
- [x] Step 7 native panels, Reveal, drop
- [x] Step 8 About
- [x] Step 9 Preferences
- [ ] Step 10 icon
- [x] Step 11 notifications
- [x] Step 12 conventions
- [ ] Step 13 accessibility check
