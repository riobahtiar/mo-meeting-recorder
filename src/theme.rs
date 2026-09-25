//! The colours the app draws with: Apple's system palette, light or dark to
//! match the appearance libadwaita follows. Window, text and accent
//! colours stay libadwaita's own; this module only supplies the waves, the
//! speakers, the recording dot and the transcription animation, plus the
//! `macos.css` layer for window chrome. The palette's own `accent` is fixed
//! systemBlue: it follows light and dark, not the user's accent colour.
//!
//! Which appearance is in force is the user's choice in Settings
//! (`Appearance`, stored by `settings.rs`): System, Light or Dark, applied through
//! libadwaita's style manager. libadwaita's macOS backend reads
//! `AppleInterfaceStyle` itself; a GTK built without that backend reports no
//! colour-scheme support and would stay light, so for System the appearance
//! is then read from `defaults` here, at startup and again whenever the
//! window becomes active, which is when someone who just changed System
//! Settings comes back to the app.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub type Rgb = (f64, f64, f64);

#[derive(Clone, Debug, Default)]
pub struct Theme {
    colors: HashMap<String, Rgb>,
}

/// Apple's palette, verified against the HIG system colours. Checked once
/// with Digital Color Meter; corrections go here, not in the callers.
fn load() -> Theme {
    load_for(adw::StyleManager::default().is_dark())
}

fn load_for(dark: bool) -> Theme {
    let c = |light: &str, dark_: &str| parse_hex(if dark { dark_ } else { light }).unwrap();
    let mut colors = HashMap::new();
    colors.insert("blue".into(), c("#007aff", "#0a84ff"));
    colors.insert("orange".into(), c("#ff9500", "#ff9f0a"));
    colors.insert("green".into(), c("#28cd41", "#32d74b"));
    colors.insert("red".into(), c("#ff3b30", "#ff453a"));
    colors.insert("yellow".into(), c("#ffcc00", "#ffd60a"));
    // systemPurple and the pre-2020 systemTeal, under the names the speaker
    // classes and the animation use.
    colors.insert("magenta".into(), c("#af52de", "#bf5af2"));
    colors.insert("cyan".into(), c("#55bef0", "#5ac8f5"));
    // Aliases for the transcription animation, which needs a scene background
    // and text in the current appearance: system grays, not libadwaita vars,
    // because it draws on Cairo, not on widgets.
    colors.insert("accent".into(), c("#007aff", "#0a84ff"));
    colors.insert("darker_background".into(), c("#f2f2f7", "#1c1c1e"));
    colors.insert("foreground".into(), c("#000000", "#ffffff"));
    colors.insert("bright_foreground".into(), c("#000000", "#ffffff"));
    Theme { colors }
}

impl Theme {
    pub fn get(&self, name: &str) -> Option<Rgb> {
        self.colors.get(name).copied()
    }
}

fn parse_hex(value: &str) -> Option<Rgb> {
    let hex = value.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some((
        f64::from(channel(0)?) / 255.0,
        f64::from(channel(2)?) / 255.0,
        f64::from(channel(4)?) / 255.0,
    ))
}

fn hex((r, g, b): Rgb) -> String {
    let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", byte(r), byte(g), byte(b))
}

/// `a` blended towards `b` by `amount`.
pub fn mix(a: Rgb, b: Rgb, amount: f64) -> Rgb {
    (
        a.0 + (b.0 - a.0) * amount,
        a.1 + (b.1 - a.1) * amount,
        a.2 + (b.2 - a.2) * amount,
    )
}

thread_local! {
    static CURRENT: RefCell<Option<Theme>> = const { RefCell::new(None) };
}

/// A system colour by name (`blue`, `orange`, `green`, `red`, `yellow`,
/// `magenta`, `cyan`, `accent`), or `fallback` when the theme is not loaded
/// yet, which only happens before the first `follow()` call.
pub fn color(name: &str, fallback: Rgb) -> Rgb {
    CURRENT.with(|c| {
        c.borrow()
            .as_ref()
            .and_then(|t| t.get(name))
            .unwrap_or(fallback)
    })
}

/// The rules that need the palette: the `.speaker-N` colours for transcript
/// rows, and the transcribing page, where the header bar sits over the
/// animation and must use the scene's own background and ink. Everything
/// else is in `macos.css`.
fn css(theme: &Theme) -> String {
    let get = |name: &str| hex(theme.get(name).unwrap_or((0.5, 0.5, 0.5)));
    format!(
        ".speaker-0 {{ color: {blue}; }} .speaker-1 {{ color: {orange}; }} \
         .speaker-2 {{ color: {green}; }} .speaker-3 {{ color: {magenta}; }} \
         .speaker-4 {{ color: {cyan}; }} .speaker-5 {{ color: {yellow}; }} \
         window.immersive {{ background: {scene}; }} \
         window.immersive headerbar {{ background: transparent; box-shadow: none; color: {ink}; }} \
         window.immersive headerbar button {{ color: {ink}; }}",
        blue = get("blue"),
        orange = get("orange"),
        green = get("green"),
        magenta = get("magenta"),
        cyan = get("cyan"),
        yellow = get("yellow"),
        scene = get("darker_background"),
        ink = get("foreground"),
    )
}

/// Light, dark or whatever the Mac is set to. Stored by `settings.rs`; the
/// enum lives here because the animation example builds this module alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appearance {
    System,
    Light,
    Dark,
}

impl Appearance {
    pub const ALL: [Appearance; 3] = [Appearance::System, Appearance::Light, Appearance::Dark];

    pub fn key(self) -> &'static str {
        match self {
            Appearance::System => "system",
            Appearance::Light => "light",
            Appearance::Dark => "dark",
        }
    }

    pub fn from_key(key: &str) -> Appearance {
        match key {
            "light" => Appearance::Light,
            "dark" => Appearance::Dark,
            _ => Appearance::System,
        }
    }
}

/// Puts the chosen appearance in force. For System on a GTK that cannot
/// follow the Mac itself, `macos_dark` says what the Mac is set to (see
/// `macos_prefers_dark`).
pub fn apply_appearance(appearance: Appearance, macos_dark: impl FnOnce() -> bool) {
    let manager = adw::StyleManager::default();
    let scheme = match appearance {
        Appearance::Light => adw::ColorScheme::ForceLight,
        Appearance::Dark => adw::ColorScheme::ForceDark,
        Appearance::System if manager.system_supports_color_schemes() => adw::ColorScheme::Default,
        Appearance::System if macos_dark() => adw::ColorScheme::ForceDark,
        Appearance::System => adw::ColorScheme::ForceLight,
    };
    if manager.color_scheme() != scheme {
        manager.set_color_scheme(scheme);
    }
}

/// Whether the Mac is in Dark mode, from `defaults read -g
/// AppleInterfaceStyle`, which prints `Dark` or fails when Light is set.
pub fn macos_prefers_dark() -> bool {
    let output = std::process::Command::new("/usr/bin/defaults")
        .args(["read", "-g", "AppleInterfaceStyle"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok();
    dark_from_defaults(
        output
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned()),
    )
}

fn dark_from_defaults(output: Option<String>) -> bool {
    output.is_some_and(|text| text.trim().eq_ignore_ascii_case("dark"))
}

/// The `macos.css` chrome layer, bundled with the binary so a bare `cargo
/// run` and the `.app` load the same rules.
const MACOS_CSS: &str = include_str!("../data/macos.css");

/// Applies the current palette and keeps following the system appearance.
/// An accent colour change also repaints (libadwaita's widgets pick it up),
/// though the palette itself stays systemBlue. `changed` runs after every
/// switch, so custom-drawn widgets can repaint.
pub fn follow(changed: impl Fn() + 'static) {
    let provider = gtk::CssProvider::new();
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
    let changed = Rc::new(changed);
    let apply = Rc::new(move |notify: bool| {
        let theme = load();
        provider.load_from_string(&format!("{}\n{}", css(&theme), MACOS_CSS));
        CURRENT.with(|c| *c.borrow_mut() = Some(theme));
        if notify {
            changed();
        }
    });
    apply(false);

    let manager = adw::StyleManager::default();
    let apply_dark = apply.clone();
    manager.connect_dark_notify(move |_| apply_dark(true));
    let apply_accent = apply.clone();
    manager.connect_accent_color_notify(move |_| apply_accent(true));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_hex_colours() {
        assert_eq!(parse_hex("#ff0000"), Some((1.0, 0.0, 0.0)));
        assert_eq!(parse_hex("ff0000"), None);
        assert_eq!(hex((1.0, 0.5, 0.0)), "#ff8000");
    }

    #[test]
    fn palettes_follow_the_appearance() {
        let light = load_for(false);
        let dark = load_for(true);
        assert!(dark.get("blue").unwrap().0 > light.get("blue").unwrap().0);
        for key in [
            "blue", "orange", "green", "red", "yellow", "magenta", "cyan",
        ] {
            assert!(light.get(key).is_some(), "{key}");
            assert!(dark.get(key).is_some(), "{key}");
        }
    }

    #[test]
    fn css_names_every_speaker() {
        let rules = css(&load_for(false));
        for speaker in 0..6 {
            assert!(rules.contains(&format!(".speaker-{speaker}")), "{rules}");
        }
        assert!(rules.contains("#007aff"));
        // The transcribing page takes the scene colours of the appearance.
        assert!(rules.contains("window.immersive { background: #f2f2f7"));
        assert!(css(&load_for(true)).contains("window.immersive { background: #1c1c1e"));
    }

    #[test]
    fn defaults_says_dark_only_when_it_prints_dark() {
        assert!(dark_from_defaults(Some("Dark\n".into())));
        assert!(!dark_from_defaults(Some("".into())));
        // Light mode has no such default: the command fails, so no output.
        assert!(!dark_from_defaults(None));
    }
}
