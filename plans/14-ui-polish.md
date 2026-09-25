# 14 UI polish: one window, one look

## Goal

The window looks and behaves like one Mac app on every page: no stray outlines, one window size that the user controls, light and dark that can be switched, controls that never crop their text, a transcribing scene that fits the window, a Settings dialog that reads as a Settings dialog, a settings button where a Mac user looks for one, and a compact strip that is worth shrinking to. This plan answers the first test session on a display (2026-09-25, screenshots in the pull request), which found eight problems with plan 07's first pass.

## Done when

- [ ] No page and no dialog shows a blue rectangle around a container: focus rings appear on buttons, entries and rows only, drawn by libadwaita.
- [ ] The window opens at the size it had when it was last closed (960×680 the first time), keeps that size across ready, recording and done, and only the compact strip changes it. The done page fits a narrow window by folding its sidebar away.
- [ ] Settings › General › Appearance offers System, Light and Dark; the choice applies at once and is kept. With System, a change in System Settings is followed, also on a GTK that reports no colour-scheme support.
- [ ] No dropdown value, row title or key hint is ellipsised at the default dialog width; the API key rows show their status and their "where to get it" text in full.
- [ ] "TRANSCRIBING" fits the window at every size from the 360 px minimum up, in a font that exists on macOS.
- [ ] Settings has pages (General, Transcription, Recording, Audio, Storage) with a switcher, and the dialog opens wide enough for its rows.
- [ ] A gear button at the right of the header bar opens Settings; ⌘, still works.
- [ ] The compact strip keeps the header bar (clock as the title, Pause, Stop and Expand at the right) over one two-lane wave; it never shows the drag hint or the traffic lights over content.

## Prerequisites

Plan 07 (the chrome this plan corrects). No new decisions: D12 and D20 stand.

## Background

What the first display session showed, and why:

1. **Blue rectangles around containers.** `macos.css` had `.macos *:focus-visible { outline: … }`. GTK 4 sets the `FOCUSED` and `FOCUS_VISIBLE` state flags on the focus widget *and every ancestor* up to the window (`gtkwindow.c` `synthesize_focus_change_events`), so `*:focus-visible` outlines the row, its group, the page box and the toolbar view at once. libadwaita already draws a focus ring on every control that can take focus; the wildcard rule must go, not be narrowed.
2. **Window size.** `ui.rs` `fit_window` resized to 480×700 for ready and 1100×760 for done, and back, on every page change, and threw away the size the user had chosen. A Mac app keeps the size the user gave it. The done page is the only one that wants width; it adapts instead (`adw::OverlaySplitView` with a breakpoint) rather than forcing the window.
3. **Light and dark.** libadwaita's macOS settings backend reads `AppleInterfaceStyle`, but nothing in the app let the user choose, and a GTK built without that backend reports `system_supports_color_schemes() == false` and stays light. Settings gets an explicit Appearance row (`adw::StyleManager::set_color_scheme`), and the System choice falls back to `defaults read -g AppleInterfaceStyle`, re-read when the window becomes active again, when GTK reports no support.
4. **Cropped rows.** `adw::ComboRow` ellipsises its value when the title side is wide: long subtitles ("Bahasa Indonesia or English. Takes effect on the next launch.") and a 640 px dialog left "Bahasa Ind…" and "In…". The API key rows put the whole hint into the row title ("ElevenLabs API key — Dashboard › profile…"), which `EntryRow` can only ellipsise. The fix is structural: short titles, hints as subtitles or as their own rows, a wider dialog, and the done sidebar 360 px wide.
5. **Cropped animation.** `animation.rs` asked for "JetBrains Mono", which most Macs do not have; Cairo substituted a proportional font whose wide glyphs pushed the twelve letters past the window edge, because the size came from a formula and not from a measurement. Now the title is measured in the font that is actually used (Menlo, shipped with macOS) and scaled to the width.
6. **Settings.** One long page with fourteen rows and no grouping worth the name. Pages with icons and a switcher are what `adw::PreferencesDialog` is for.
7. **Settings button.** Only the app menu and ⌘, opened Settings. A gear at the right of the header bar is where Mac users (and every libadwaita app) put it.
8. **Compact strip.** A 300×84 box of thin waves with a custom drag handle, a tooltip explaining how to drag, and the native traffic lights painted over the top of it. Keeping the header bar in compact mode gives the strip a real title bar (drag, traffic lights, the clock as the title) and a place for Pause, Stop and Expand; the waves become one two-lane lane like the player's, so the strip reads as the same app.

## Steps

### 1. Focus rings and the CSS layer

Delete the wildcard focus rule from `data/macos.css`. Move the app's own rules out of `ui.rs` `load_css()` into `data/macos.css` (they are the app's chrome too), and keep only the palette-dependent rules (`.speaker-N`, the immersive background) in `theme.rs` `css()`, so light and dark get the right ones. One CSS provider, one file to read.

### 2. One window size

- `settings.rs`: `load_window_size()` / `save_window_size()` (`window_width`, `window_height` in `settings.json`, integers). Saved from `close-request` and on `unmap`; read in `Recorder::new`.
- Remove `fit_window`, `FULL_SIZE`, `DONE_SIZE` and `fitted`. One `DEFAULT_SIZE` (960×680), one minimum (`set_size_request(560, 520)`) except in compact mode.
- Ready page: `adw::Clamp` (maximum 640) so the meters and the button sit centred in a wide window.
- Done page: `adw::OverlaySplitView` with the sidebar (name, speakers, chapters, actions) at 360 px and the transcript as content; an `adw::Breakpoint` at `max-width: 860sp` sets `collapsed` so a narrow window shows the transcript with a sidebar toggle in the header bar.

### 3. Appearance

- `settings.rs`: `load_appearance()` / `save_appearance()` with `system`, `light`, `dark`.
- `theme.rs`: `apply_appearance(Appearance)` maps to `adw::ColorScheme::{Default, ForceLight, ForceDark}`. For `system` on a GTK without colour-scheme support, `macos_is_dark()` runs `defaults read -g AppleInterfaceStyle` (prints `Dark` or fails), applied at startup and whenever the window's `is-active` turns true, since that is when a user who just changed System Settings comes back.
- Settings › General › Appearance row.

### 4. Rows that fit

- Settings dialog `content_width(760)`, `content_height(640)`.
- Every ComboRow subtitle at most one short clause; the "next launch" note becomes the group description.
- API keys: one `adw::ExpanderRow` per provider titled with the provider, subtitle "Saved in the Keychain" or "No key yet"; inside, an `adw::PasswordEntryRow` with an apply button and an `adw::ActionRow` whose subtitle is the where-to-get-it hint, wrapping.
- Done sidebar 360 px; the "Transcribe again" row loses its subtitle (the button's tooltip says it).

### 5. The animation

`animation.rs`: `FONT = "Menlo"`; `title()` measures the twelve glyph advances at a trial size and scales the size so the whole title fits `w - 2 * pad`, then clamps to the height budget. The HUD panel keeps at least two transcript rows and never draws above the horizon.

### 6. Settings pages

`show_preferences` builds five `adw::PreferencesPage`s with icons: General (interface language, appearance, menu bar item), Transcription (model, default language, provider, keys), Recording (format, your name, meetings folder, timer defaults from plan 15), Audio (microphone, computer audio, plan 15's source pickers), Storage (plan 15's cache, models, reset). The dialog is one function per page so each stays readable.

### 7. Settings button

`gtk::Button` with `emblem-system-symbolic`, `action_name("app.preferences")`, tooltip "Settings (⌘,)", packed at the end of the header bar; hidden in compact mode.

### 8. The compact strip

- `set_compact(true)` keeps `reveal_top_bars`, sets the header's title widget to the clock box (dot + timer), shows the strip-only buttons (Pause, Stop, Expand) packed at the end, hides the gear and the full-page compact button, and shows the `compact` stack page: one `gtk::DrawingArea` drawing the mic history above the midline and the computer history below it, 48 px high, in the two wave colours.
- Window: `set_size_request(360, 92)`, `set_default_size(380, 92)`, `set_resizable(false)` while compact, restored after.
- The old `WindowHandle`, the two thin meters, the drag-hint tooltip and `strip.drag_hint` go.

## Verify

1. Tab through the ready page and Settings: rings on controls only.
2. Resize the window on ready, record, stop, wait for done: the size never changes; quit and relaunch: same size. Narrow the window below 860 px on done: the sidebar folds, the toggle appears.
3. Settings › Appearance: Light, Dark, System, then switch the system in System Settings with System chosen: the window follows (at once, or when it is next activated on a GTK without support).
4. Settings in Indonesian at the default width: no "…" in any row; expand each key row.
5. Transcribe with the window at its minimum width and at full screen: the title fits both.
6. ⇧⌘M while recording: the strip shows the clock in the title bar, the two-lane wave and three buttons; Pause and Stop work from it; ⇧⌘M again restores the previous size.

Coded 2026-09-25 in a Linux container against GTK 4.14 and libadwaita 1.5
headers (the 1.6 symbols left unresolved at link time): `cargo check`,
`cargo test` and `clippy -D warnings` pass. Nothing here has been seen on a
display yet; the Verify list is the next session's job, and every Done when
line stays open until then.

## Status

- [x] Step 1 focus rings and CSS layer
- [x] Step 2 one window size
- [x] Step 3 appearance
- [x] Step 4 rows that fit
- [x] Step 5 animation
- [x] Step 6 settings pages
- [x] Step 7 settings button
- [x] Step 8 compact strip
