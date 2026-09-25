//! The player on the done page (plan 18): a two-lane waveform of the meeting
//! (you above the line, the other side below it) with a playhead, click or
//! drag to seek, and under it the transport: previous and next transcript
//! line, back and forward 15 s, play, speed, volume, and a level animation
//! while it plays.
//!
//! Playback is one ffmpeg decoding to the default output, because the app
//! already depends on ffmpeg for recording and converting. The mechanics
//! live in core `playback`, the output in `momr-platform`; pausing stops the
//! process and playing starts it again at the position; a meeting saved as
//! separate files is mixed on the fly. A new speed or volume restarts it at
//! the position too, so the volume slider is applied once it rests rather
//! than on every step of a drag.
//!
//! While playing, the waveform and the level bars redraw on the frame clock,
//! so the playhead glides instead of stepping; paused, nothing ticks.
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gio, glib};

use momr_core::export;
use momr_core::locales::t;
use momr_core::playback::{
    self, BINS, MAX_VOLUME, Playback, SPEEDS, Sound, peaks, probe_duration_us,
};
use momr_core::timer::clock;

const MIC_COLOR: (f64, f64, f64) = (0.0, 0.478, 1.0);
const SYSTEM_COLOR: (f64, f64, f64) = (1.0, 0.584, 0.0);
/// The skip buttons and the arrow keys.
const SKIP_US: i64 = 15_000_000;
const KEY_SKIP_US: i64 = 5_000_000;
/// A slider that has rested this long is applied (one ffmpeg restart).
const VOLUME_SETTLE: Duration = Duration::from_millis(180);
/// "Previous line" within this much of a line's start goes to the one
/// before, as media players do with tracks.
const RESTART_GRACE_MS: i64 = 2000;
/// Level bars next to the time.
const LEVEL_BARS: usize = 5;

type PositionCallback = Rc<RefCell<Option<Box<dyn Fn(i64)>>>>;
type ErrorCallback = Rc<RefCell<Option<Box<dyn Fn(&str)>>>>;

#[derive(Default)]
struct State {
    files: Vec<PathBuf>,
    duration_us: i64,
    /// Where playback starts from next, while paused.
    paused_at_us: i64,
    playback: Option<Playback>,
    /// Peaks per bin for the mic and the computer track, 0..1.
    peaks: Option<(Vec<f32>, Vec<f32>)>,
    /// Bumped on every load, so a slow waveform for an old meeting is dropped.
    generation: u64,
    /// Chapter starts in ms with their titles, drawn as markers.
    chapters: Vec<(i64, String)>,
    /// Transcript line starts in ms, for previous and next line.
    lines: Vec<i64>,
    /// Where the pointer is over the waveform, for the hover line.
    hover_x: Option<f64>,
    /// The level bars as drawn, eased toward the audio so they move
    /// smoothly: (heights 0..1, frame time of the last update in µs).
    levels: ([f64; LEVEL_BARS], i64),
}

#[derive(Clone)]
pub struct Player {
    root: gtk::Box,
    button: gtk::Button,
    wave: gtk::DrawingArea,
    levels: gtk::DrawingArea,
    elapsed: gtk::Label,
    remaining: gtk::Label,
    speed: gtk::DropDown,
    mute: gtk::Button,
    volume: gtk::Scale,
    /// Everything but the play button, off while nothing is loaded.
    controls: Vec<gtk::Widget>,
    state: Rc<RefCell<State>>,
    sound: Rc<Cell<Sound>>,
    /// The volume before Mute, so Unmute gives it back.
    unmuted: Rc<Cell<f64>>,
    /// Bumped per slider step; only the last one restarts playback.
    volume_generation: Rc<Cell<u64>>,
    /// Called with the position in ms while playing, for the transcript highlight.
    on_position: PositionCallback,
    /// Called with ffmpeg's reason when playback fails, so the app can say so.
    on_error: ErrorCallback,
    ticking: Rc<Cell<bool>>,
    /// The second shown by the clock labels, so they change once a second.
    shown_second: Rc<Cell<i64>>,
}

fn icon_button(icon: &str, tooltip: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .valign(gtk::Align::Center)
        .css_classes(["flat", "circular", "player-control"])
        .build()
}

fn speed_label(speed: f64) -> String {
    let text = format!("{speed:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    format!("{text}×")
}

impl Player {
    pub fn new() -> Self {
        let wave = gtk::DrawingArea::builder()
            .content_height(92)
            .hexpand(true)
            .build();
        wave.set_cursor_from_name(Some("pointer"));
        // Which lane is which, as a legend between the two times: labels on
        // the waveform itself would sit on the bars.
        let lane = |text: &str, class: &str| {
            let dot = gtk::Label::builder()
                .label("●")
                .css_classes(["caption", class])
                .build();
            let name = gtk::Label::builder()
                .label(text)
                .css_classes(["caption", "dim-label"])
                .build();
            let lane = gtk::Box::builder().spacing(4).build();
            lane.append(&dot);
            lane.append(&name);
            lane
        };
        let legend = gtk::Box::builder()
            .spacing(14)
            .halign(gtk::Align::Center)
            .build();
        legend.append(&lane(t("player.lane_mic"), "speaker-0"));
        legend.append(&lane(t("player.lane_computer"), "speaker-1"));

        let elapsed = gtk::Label::builder()
            .label("00:00")
            .xalign(0.0)
            .hexpand(true)
            .css_classes(["numeric", "caption", "dim-label"])
            .build();
        let remaining = gtk::Label::builder()
            .label("-00:00")
            .xalign(1.0)
            .css_classes(["numeric", "caption", "dim-label"])
            .build();
        remaining.set_hexpand(true);
        let times = gtk::Box::builder().build();
        times.append(&elapsed);
        times.append(&legend);
        times.append(&remaining);

        let previous = icon_button("media-skip-backward-symbolic", t("player.previous_line"));
        let back = icon_button("media-seek-backward-symbolic", t("player.back"));
        let button = gtk::Button::builder()
            .icon_name("media-playback-start-symbolic")
            .tooltip_text(t("player.play"))
            .valign(gtk::Align::Center)
            .css_classes(["circular", "suggested-action", "player-play"])
            .width_request(40)
            .height_request(40)
            .halign(gtk::Align::Center)
            .build();
        let forward = icon_button("media-seek-forward-symbolic", t("player.forward"));
        let next = icon_button("media-skip-forward-symbolic", t("player.next_line"));

        let sound = momr_core::settings::load_player_sound();
        let speed_labels: Vec<String> = SPEEDS.iter().map(|s| speed_label(*s)).collect();
        let speed_refs: Vec<&str> = speed_labels.iter().map(String::as_str).collect();
        let speed = gtk::DropDown::from_strings(&speed_refs);
        speed.set_tooltip_text(Some(t("player.speed")));
        speed.set_valign(gtk::Align::Center);
        speed.add_css_class("flat");
        speed.set_selected(
            SPEEDS
                .iter()
                .position(|s| (s - sound.speed).abs() < 1e-3)
                .unwrap_or(2) as u32,
        );
        let mute = icon_button("audio-volume-high-symbolic", t("player.mute"));
        let volume =
            gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, MAX_VOLUME * 100.0, 1.0);
        volume.set_draw_value(false);
        volume.set_width_request(96);
        volume.set_valign(gtk::Align::Center);
        volume.set_tooltip_text(Some(t("player.volume")));
        // The notch marks 100 %: past it the meeting is louder than recorded.
        volume.add_mark(100.0, gtk::PositionType::Bottom, None);
        volume.set_value(sound.volume * 100.0);
        let levels = gtk::DrawingArea::builder()
            .content_width(26)
            .content_height(22)
            .valign(gtk::Align::Center)
            .build();

        let transport = gtk::Box::builder()
            .spacing(2)
            .halign(gtk::Align::Start)
            .build();
        for widget in [&previous, &back] {
            transport.append(widget);
        }
        transport.append(&button);
        for widget in [&forward, &next] {
            transport.append(widget);
        }
        let tools = gtk::Box::builder()
            .spacing(4)
            .hexpand(true)
            .halign(gtk::Align::End)
            .build();
        tools.append(&levels);
        tools.append(&speed);
        tools.append(&mute);
        tools.append(&volume);
        let bar = gtk::Box::builder().spacing(12).build();
        bar.append(&transport);
        bar.append(&tools);

        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .css_classes(["card", "player"])
            .tooltip_text(t("player.keys"))
            .build();
        root.append(&wave);
        root.append(&times);
        root.append(&bar);

        let player = Player {
            root,
            button,
            wave,
            levels,
            elapsed,
            remaining,
            speed,
            mute,
            volume,
            controls: vec![
                previous.clone().upcast(),
                back.clone().upcast(),
                forward.clone().upcast(),
                next.clone().upcast(),
            ],
            state: Rc::default(),
            sound: Rc::new(Cell::new(sound)),
            unmuted: Rc::new(Cell::new(if sound.volume > 0.0 {
                sound.volume
            } else {
                1.0
            })),
            volume_generation: Rc::default(),
            on_position: Rc::default(),
            on_error: Rc::default(),
            ticking: Rc::default(),
            shown_second: Rc::new(Cell::new(-1)),
        };
        player.set_volume_icon();

        let this = player.clone();
        player.wave.set_draw_func(move |_, cr, width, height| {
            this.draw(cr, f64::from(width), f64::from(height));
        });
        let this = player.clone();
        player.levels.set_draw_func(move |_, cr, width, height| {
            this.draw_levels(cr, f64::from(width), f64::from(height));
        });

        let this = player.clone();
        player.button.connect_clicked(move |_| this.toggle());
        let this = player.clone();
        back.connect_clicked(move |_| this.skip(-SKIP_US));
        let this = player.clone();
        forward.connect_clicked(move |_| this.skip(SKIP_US));
        let this = player.clone();
        previous.connect_clicked(move |_| this.previous_line());
        let this = player.clone();
        next.connect_clicked(move |_| this.next_line());
        let this = player.clone();
        player.speed.connect_selected_notify(move |dropdown| {
            if let Some(speed) = SPEEDS.get(dropdown.selected() as usize) {
                this.set_sound(Sound {
                    speed: *speed,
                    ..this.sound.get()
                });
                this.restart();
            }
        });
        let this = player.clone();
        player.volume.connect_value_changed(move |scale| {
            let volume = scale.value() / 100.0;
            if volume > 0.0 {
                this.unmuted.set(volume);
            }
            this.set_sound(Sound {
                volume,
                ..this.sound.get()
            });
            this.set_volume_icon();
            // Applied once the slider rests: each application restarts ffmpeg.
            let generation = this.volume_generation.get() + 1;
            this.volume_generation.set(generation);
            let later = this.clone();
            glib::timeout_add_local_once(VOLUME_SETTLE, move || {
                if later.volume_generation.get() == generation {
                    later.restart();
                }
            });
        });
        let this = player.clone();
        player.mute.connect_clicked(move |_| {
            let muted = this.sound.get().volume <= 0.0;
            this.volume.set_value(if muted {
                this.unmuted.get() * 100.0
            } else {
                0.0
            });
        });

        // Click or drag anywhere on the waveform to seek.
        let drag = gtk::GestureDrag::new();
        let this = player.clone();
        drag.connect_drag_begin(move |_, x, _| this.seek_to_x(x));
        let this = player.clone();
        drag.connect_drag_update(move |gesture, dx, _| {
            if let Some((x, _)) = gesture.start_point() {
                this.seek_to_x(x + dx);
            }
        });
        player.wave.add_controller(drag);

        // The hover line shows where a click would land.
        let motion = gtk::EventControllerMotion::new();
        let this = player.clone();
        motion.connect_motion(move |_, x, _| {
            this.state.borrow_mut().hover_x = Some(x);
            this.wave.queue_draw();
        });
        let this = player.clone();
        motion.connect_leave(move |_| {
            this.state.borrow_mut().hover_x = None;
            this.wave.queue_draw();
        });
        player.wave.add_controller(motion);

        // Hovering says the time there, and the chapter when on a marker.
        player.wave.set_has_tooltip(true);
        let this = player.clone();
        player
            .wave
            .connect_query_tooltip(move |_, x, _, _, tooltip| {
                let x = f64::from(x);
                let duration = this.duration_us();
                if duration <= 0 {
                    return false;
                }
                let width = f64::from(this.wave.width()).max(1.0);
                let at = clock((duration as f64 * (x / width).clamp(0.0, 1.0)) as i64 / 1_000_000);
                tooltip.set_text(Some(&match this.chapter_near(x) {
                    Some(title) => format!("{at} · {title}"),
                    None => at,
                }));
                true
            });

        player.set_loaded(false);
        player
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    pub fn connect_error(&self, callback: impl Fn(&str) + 'static) {
        *self.on_error.borrow_mut() = Some(Box::new(callback));
    }

    fn report(&self, reason: &str) {
        if let Some(callback) = self.on_error.borrow().as_ref() {
            callback(reason);
        }
    }

    pub fn connect_position(&self, callback: impl Fn(i64) + 'static) {
        *self.on_position.borrow_mut() = Some(Box::new(callback));
    }

    /// The keys the done page hands over when no text has the focus: Space
    /// plays or pauses, ← and → skip 5 s. True when the key was used.
    pub fn handle_key(&self, key: gtk::gdk::Key) -> bool {
        if self.state.borrow().files.is_empty() {
            return false;
        }
        match key {
            gtk::gdk::Key::space => self.toggle(),
            gtk::gdk::Key::Left => self.skip(-KEY_SKIP_US),
            gtk::gdk::Key::Right => self.skip(KEY_SKIP_US),
            _ => return false,
        }
        true
    }

    /// Loads the meeting in `dir`: its audio for playback, its tracks for the waveform.
    pub fn load(&self, dir: &Path) {
        self.unload();
        let playable: Vec<PathBuf> = if dir.join("audio.ogg").is_file() {
            vec![dir.join("audio.ogg")]
        } else {
            ["mic.ogg", "computer.ogg"]
                .iter()
                .map(|f| dir.join(f))
                .filter(|p| p.is_file())
                .collect()
        };
        let generation = {
            let mut state = self.state.borrow_mut();
            state.duration_us = playable
                .iter()
                .filter_map(|p| probe_duration_us(p))
                .max()
                .unwrap_or(0);
            state.files = playable.clone();
            state.paused_at_us = 0;
            state.generation += 1;
            state.generation
        };
        self.root.set_visible(!playable.is_empty());
        self.set_loaded(!playable.is_empty());

        // The kept tracks give each side its own lane; without them, one lane.
        let (mic, computer) = export::tracks(dir);
        let sources = if mic.is_file() && computer.is_file() {
            (mic, Some(computer))
        } else if let Some(first) = playable.first() {
            (first.clone(), None)
        } else {
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let peaks = gio::spawn_blocking(move || {
                let mic = peaks(&sources.0);
                let computer = sources.1.as_deref().map(peaks);
                (mic, computer)
            })
            .await;
            if let Ok((Some((mic, length)), computer)) = peaks {
                let mut state = this.state.borrow_mut();
                if state.generation == generation {
                    let computer = computer
                        .flatten()
                        .map(|(peaks, _)| peaks)
                        .unwrap_or_else(|| vec![0.0; BINS]);
                    state.peaks = Some((mic, computer));
                    // Without a duration from ffprobe, the decoded length is
                    // the next best thing: seeking and the clock work.
                    if state.duration_us <= 0 {
                        state.duration_us = length;
                    }
                }
            }
            this.refresh();
        });
        self.refresh();
    }

    /// Shows chapter markers on the waveform; empty removes them.
    pub fn set_chapters(&self, chapters: Vec<(i64, String)>) {
        self.state.borrow_mut().chapters = chapters;
        self.wave.queue_draw();
    }

    /// The transcript's line starts in ms, for previous and next line.
    pub fn set_lines(&self, mut lines: Vec<i64>) {
        lines.sort_unstable();
        lines.dedup();
        self.state.borrow_mut().lines = lines;
    }

    fn chapter_near(&self, x: f64) -> Option<String> {
        let duration = self.duration_us();
        if duration <= 0 {
            return None;
        }
        let width = f64::from(self.wave.width()).max(1.0);
        self.state
            .borrow()
            .chapters
            .iter()
            .map(|(ms, title)| ((*ms * 1000) as f64 / duration as f64 * width, title))
            .filter(|(at, _)| (at - x).abs() <= 6.0)
            .min_by(|a, b| (a.0 - x).abs().total_cmp(&(b.0 - x).abs()))
            .map(|(_, title)| title.clone())
    }

    /// Stops playback and forgets the meeting.
    pub fn unload(&self) {
        let mut state = self.state.borrow_mut();
        state.playback = None;
        state.files.clear();
        state.duration_us = 0;
        state.paused_at_us = 0;
        state.peaks = None;
        state.chapters.clear();
        state.lines.clear();
        state.levels = Default::default();
        drop(state);
        self.set_playing_icon(false);
        self.set_loaded(false);
        self.refresh();
    }

    fn set_loaded(&self, loaded: bool) {
        self.button.set_sensitive(loaded);
        for widget in &self.controls {
            widget.set_sensitive(loaded);
        }
    }

    fn duration_us(&self) -> i64 {
        self.state.borrow().duration_us
    }

    fn position_us(&self) -> i64 {
        let state = self.state.borrow();
        match &state.playback {
            Some(playback) => playback.position_us().min(state.duration_us),
            None => state.paused_at_us,
        }
    }

    pub fn is_playing(&self) -> bool {
        self.state.borrow().playback.is_some()
    }

    fn toggle(&self) {
        if self.is_playing() {
            self.pause();
        } else {
            self.play();
        }
    }

    pub fn play(&self) {
        let sound = self.sound.get();
        let started = {
            let mut state = self.state.borrow_mut();
            if state.files.is_empty() {
                return;
            }
            if state.duration_us > 0 && state.paused_at_us >= state.duration_us {
                state.paused_at_us = 0;
            }
            state.playback = None;
            playback::Playback::start(&state.files, state.paused_at_us, sound).map(|playback| {
                state.playback = Some(playback);
            })
        };
        if let Err(reason) = &started {
            self.report(reason);
        }
        let started = started.is_ok();
        self.set_playing_icon(started);
        if started {
            self.start_ticking();
        }
    }

    pub fn pause(&self) {
        let position = self.position_us();
        {
            let mut state = self.state.borrow_mut();
            state.playback = None;
            state.paused_at_us = position;
        }
        self.set_playing_icon(false);
        self.refresh();
    }

    /// A new speed or volume takes effect: playing, ffmpeg restarts at the
    /// position; paused, it applies on the next play.
    fn restart(&self) {
        if self.is_playing() {
            let position = self.position_us();
            self.state.borrow_mut().paused_at_us = position;
            self.play();
        }
    }

    fn set_sound(&self, sound: Sound) {
        let sound = sound.clamped();
        self.sound.set(sound);
        let _ = momr_core::settings::save_player_sound(sound);
    }

    fn set_volume_icon(&self) {
        let volume = self.sound.get().volume;
        let (icon, tip) = match volume {
            v if v <= 0.0 => ("audio-volume-muted-symbolic", t("player.unmute")),
            v if v < 0.34 => ("audio-volume-low-symbolic", t("player.mute")),
            v if v < 0.67 => ("audio-volume-medium-symbolic", t("player.mute")),
            _ => ("audio-volume-high-symbolic", t("player.mute")),
        };
        self.mute.set_icon_name(icon);
        self.mute.set_tooltip_text(Some(tip));
    }

    /// Jumps to `ms` and plays from there, for a click on the transcript.
    pub fn play_from(&self, ms: i64) {
        self.state.borrow_mut().paused_at_us = (ms * 1000).max(0);
        self.play();
    }

    fn seek(&self, us: i64) {
        let us = us.clamp(0, self.duration_us().max(0));
        let playing = self.is_playing();
        self.state.borrow_mut().paused_at_us = us;
        if playing {
            self.play();
        }
        self.refresh();
    }

    fn skip(&self, by_us: i64) {
        self.seek(self.position_us() + by_us);
    }

    fn previous_line(&self) {
        let now = self.position_us() / 1000;
        let target = previous_line(&self.state.borrow().lines, now);
        self.seek(target * 1000);
    }

    fn next_line(&self) {
        let now = self.position_us() / 1000;
        let target = next_line(&self.state.borrow().lines, now);
        if let Some(ms) = target {
            self.seek(ms * 1000);
        }
    }

    fn seek_to_x(&self, x: f64) {
        let width = f64::from(self.wave.width()).max(1.0);
        let fraction = (x / width).clamp(0.0, 1.0);
        self.seek((self.duration_us() as f64 * fraction) as i64);
    }

    fn set_playing_icon(&self, playing: bool) {
        self.button.set_icon_name(if playing {
            "media-playback-pause-symbolic"
        } else {
            "media-playback-start-symbolic"
        });
        self.button.set_tooltip_text(Some(if playing {
            t("player.pause")
        } else {
            t("player.play")
        }));
        if playing {
            self.root.add_css_class("playing");
        } else {
            self.root.remove_css_class("playing");
        }
    }

    /// Redraws on the frame clock while playing: the playhead glides and
    /// the level bars move; the end of the file pauses and rewinds.
    fn start_ticking(&self) {
        if self.ticking.replace(true) {
            return;
        }
        let this = self.clone();
        self.wave.add_tick_callback(move |_, frame| {
            let ended = this
                .state
                .borrow_mut()
                .playback
                .as_mut()
                .and_then(Playback::ended);
            if let Some(result) = ended {
                this.pause();
                this.state.borrow_mut().paused_at_us = 0;
                if let Err(reason) = result {
                    this.report(&reason);
                }
            }
            this.ease_levels(frame.frame_time());
            this.refresh();
            let still = this.is_playing()
                || this
                    .state
                    .borrow()
                    .levels
                    .0
                    .iter()
                    .any(|level| *level > 0.01);
            if still {
                glib::ControlFlow::Continue
            } else {
                this.ticking.set(false);
                glib::ControlFlow::Break
            }
        });
    }

    /// Moves the level bars toward the loudness at the playhead: up fast,
    /// down slowly, the way a meter reads, and to rest when paused. Each bar
    /// sways a little on its own phase, so a steady voice still looks alive,
    /// but silence is always flat.
    fn ease_levels(&self, now_us: i64) {
        let playing = self.is_playing();
        let (position, duration) = (self.position_us(), self.duration_us());
        let mut state = self.state.borrow_mut();
        let loudness = match (&state.peaks, playing && duration > 0) {
            (Some((mic, computer)), true) => {
                let at = ((position as f64 / duration as f64) * BINS as f64) as usize;
                let at = at.min(BINS - 1);
                f64::from(mic[at].max(computer[at]))
            }
            _ => 0.0,
        };
        let (levels, last) = &mut state.levels;
        let dt = if *last == 0 {
            0.016
        } else {
            ((now_us - *last) as f64 / 1e6).clamp(0.0, 0.1)
        };
        *last = now_us;
        let t = now_us as f64 / 1e6;
        for (i, level) in levels.iter_mut().enumerate() {
            let sway = 0.6 + 0.4 * (t * (5.0 + i as f64 * 1.7) + i as f64 * 1.3).sin().abs();
            let target = loudness * sway;
            let rate = if target > *level { 18.0 } else { 5.0 };
            *level += (target - *level) * (rate * dt).min(1.0);
        }
    }

    fn refresh(&self) {
        let (position, duration) = (self.position_us(), self.duration_us());
        let second = position / 1_000_000;
        if second != self.shown_second.replace(second) || !self.is_playing() {
            self.elapsed.set_label(&clock(second));
            self.remaining.set_label(&format!(
                "-{}",
                clock((duration - position).max(0) / 1_000_000)
            ));
        }
        self.wave.queue_draw();
        self.levels.queue_draw();
        if let Some(callback) = self.on_position.borrow().as_ref() {
            callback(position / 1000);
        }
    }

    fn draw(&self, cr: &gtk::cairo::Context, width: f64, height: f64) {
        let state = self.state.borrow();
        let duration = self.duration_us();
        let played = if duration > 0 {
            self.position_us() as f64 / duration as f64
        } else {
            0.0
        };
        // Room above and below for the lane captions, a gap at the middle.
        let mid = height / 2.0;
        let reach = mid - 3.0;
        let step = width / BINS as f64;
        let head = played * width;
        let ink = self.wave.color();
        let (ir, ig, ib) = (
            f64::from(ink.red()),
            f64::from(ink.green()),
            f64::from(ink.blue()),
        );

        // The centre line, faint, so a silent stretch still reads as a track.
        cr.set_source_rgba(ir, ig, ib, 0.12);
        cr.rectangle(0.0, mid - 0.5, width, 1.0);
        let _ = cr.fill();

        if let Some((mic, computer)) = state.peaks.as_ref() {
            // Bars grouped so each is at least 2 px with a 1 px gap, rounded.
            let group = (3.0 / step).ceil().max(1.0) as usize;
            let bar = (step * group as f64 - 1.0).max(1.0);
            cr.set_line_cap(gtk::cairo::LineCap::Round);
            cr.set_line_width(bar);
            let (mr, mg, mb) = crate::theme::color("blue", MIC_COLOR);
            let (sr, sg, sb) = crate::theme::color("orange", SYSTEM_COLOR);
            for start in (0..BINS).step_by(group) {
                let end = (start + group).min(BINS);
                let peak = |v: &[f32]| f64::from(v[start..end].iter().copied().fold(0.0, f32::max));
                let x = (start as f64 + group as f64 / 2.0) * step;
                let alpha = if x <= head { 1.0 } else { 0.32 };
                let up = (peak(mic) * reach).max(0.75);
                let down = (peak(computer) * reach).max(0.75);
                cr.set_source_rgba(mr, mg, mb, alpha);
                cr.move_to(x, mid - 1.5);
                cr.line_to(x, mid - 1.5 - up);
                let _ = cr.stroke();
                cr.set_source_rgba(sr, sg, sb, alpha);
                cr.move_to(x, mid + 1.5);
                cr.line_to(x, mid + 1.5 + down);
                let _ = cr.stroke();
            }
        }

        if duration > 0 {
            // Chapter markers: a thin line with a small notch at the top.
            for (ms, _) in &state.chapters {
                let x = ((*ms * 1000) as f64 / duration as f64 * width).round() + 0.5;
                cr.set_source_rgba(ir, ig, ib, 0.3);
                cr.rectangle(x - 0.5, 0.0, 1.0, height);
                let _ = cr.fill();
                cr.set_source_rgba(ir, ig, ib, 0.85);
                cr.move_to(x - 4.0, 0.0);
                cr.line_to(x + 4.0, 0.0);
                cr.line_to(x, 5.0);
                cr.close_path();
                let _ = cr.fill();
            }
            if let Some(x) = state.hover_x {
                cr.set_source_rgba(ir, ig, ib, 0.35);
                cr.rectangle(x.round() - 0.5, 0.0, 1.0, height);
                let _ = cr.fill();
            }
            // The playhead: a line with a knob on the centre line.
            let (ar, ag, ab) = accent();
            cr.set_source_rgba(ar, ag, ab, 1.0);
            cr.rectangle(head.round() - 1.0, 0.0, 2.0, height);
            let _ = cr.fill();
            cr.arc(head.round(), mid, 5.0, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
        }
    }

    fn draw_levels(&self, cr: &gtk::cairo::Context, width: f64, height: f64) {
        let state = self.state.borrow();
        let (ar, ag, ab) = accent();
        let gap = 2.0;
        let bar = (width - gap * (LEVEL_BARS as f64 - 1.0)) / LEVEL_BARS as f64;
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        cr.set_line_width(bar);
        for (i, level) in state.levels.0.iter().enumerate() {
            let x = i as f64 * (bar + gap) + bar / 2.0;
            // A floor, so the bars read as a meter at rest too.
            let h = (level.clamp(0.0, 1.0) * (height - bar)).max(1.0);
            cr.set_source_rgba(ar, ag, ab, if *level > 0.01 { 0.95 } else { 0.35 });
            cr.move_to(x, height - bar / 2.0);
            cr.line_to(x, height - bar / 2.0 - h);
            let _ = cr.stroke();
        }
    }
}

/// The accent colour for the playhead and the level bars: the theme's, or
/// the system blue when the theme has none.
fn accent() -> (f64, f64, f64) {
    crate::theme::color("accent", MIC_COLOR)
}

/// Where "previous line" goes from `now_ms`: the start of the line playing,
/// or the one before when that start was under two seconds ago.
fn previous_line(lines: &[i64], now_ms: i64) -> i64 {
    let current = lines.iter().rposition(|start| *start <= now_ms);
    match current {
        Some(i) if now_ms - lines[i] >= RESTART_GRACE_MS => lines[i],
        Some(i) if i > 0 => lines[i - 1],
        _ => 0,
    }
}

/// Where "next line" goes from `now_ms`: the next line's start, if any.
fn next_line(lines: &[i64], now_ms: i64) -> Option<i64> {
    lines.iter().copied().find(|start| *start > now_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_steps_follow_media_player_habits() {
        let lines = [0, 9_000, 13_000, 30_000];
        // Well into a line: back to its start.
        assert_eq!(previous_line(&lines, 20_000), 13_000);
        // Just after a start: the line before.
        assert_eq!(previous_line(&lines, 13_500), 9_000);
        assert_eq!(previous_line(&lines, 500), 0);
        assert_eq!(previous_line(&[], 5_000), 0);
        assert_eq!(next_line(&lines, 9_000), Some(13_000));
        assert_eq!(next_line(&lines, 30_000), None);
    }

    #[test]
    fn speeds_read_as_multipliers() {
        let labels: Vec<String> = SPEEDS.iter().map(|s| speed_label(*s)).collect();
        assert_eq!(
            labels,
            ["0.5×", "0.75×", "1×", "1.25×", "1.5×", "1.75×", "2×"]
        );
    }
}
