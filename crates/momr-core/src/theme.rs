//! The appearance choice: System, Light or Dark. The enum is pure data so
//! the core (settings) and every shell can share it; the actual switching
//! stays in the shell's theme module, which knows its own widget toolkit.

/// Light, dark or whatever the machine is set to. Stored by `settings`.
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
