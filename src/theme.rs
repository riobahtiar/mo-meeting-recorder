//! The colours the app draws with: Apple's system palette, light or dark to
//! match the appearance libadwaita already follows. Window, text and accent
//! colours stay libadwaita's own; this module only supplies the waves, the
//! speakers, the recording dot and the transcription animation, plus the
//! `macos.css` layer for window chrome.

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

/// The `.speaker-N` rules for transcript rows.
fn css(theme: &Theme) -> String {
    let get = |name: &str| hex(theme.get(name).unwrap_or((0.5, 0.5, 0.5)));
    format!(
        ".speaker-0 {{ color: {blue}; }} .speaker-1 {{ color: {orange}; }} \
         .speaker-2 {{ color: {green}; }} .speaker-3 {{ color: {magenta}; }} \
         .speaker-4 {{ color: {cyan}; }} .speaker-5 {{ color: {yellow}; }}",
        blue = get("blue"),
        orange = get("orange"),
        green = get("green"),
        magenta = get("magenta"),
        cyan = get("cyan"),
        yellow = get("yellow"),
    )
}

/// The `macos.css` chrome layer, bundled with the binary so a bare `cargo
/// run` and the `.app` load the same rules.
const MACOS_CSS: &str = include_str!("../data/macos.css");

/// Applies the current palette and keeps following the system appearance and
/// accent. `changed` runs after every switch, so custom-drawn widgets can
/// repaint.
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
        let css = css(&load_for(false));
        for speaker in 0..6 {
            assert!(css.contains(&format!(".speaker-{speaker}")), "{css}");
        }
        assert!(css.contains("#007aff"));
    }
}
