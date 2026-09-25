//! The player on the done page: a two-lane waveform of the meeting (you above
//! the line, the other side below it) with a playhead, click or drag to seek.
//!
//! Playback is one ffmpeg decoding to the default output, because the app
//! already depends on ffmpeg for recording and converting. The mechanics
//! live in core `playback` (this shell passes the macOS `audiotoolbox`
//! sink); pausing stops the process and playing starts it again at the
//! position; a meeting saved as separate files is mixed on the fly.
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gio, glib};

use momr_core::export;
use momr_core::playback::{self, BINS, Playback, clock, peaks, probe_duration_us};

const MIC_COLOR: (f64, f64, f64) = (0.0, 0.478, 1.0);
const SYSTEM_COLOR: (f64, f64, f64) = (1.0, 0.584, 0.0);

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
}

#[derive(Clone)]
pub struct Player {
    root: gtk::Box,
    button: gtk::Button,
    wave: gtk::DrawingArea,
    time: gtk::Label,
    state: Rc<RefCell<State>>,
    /// Called with the position in ms while playing, for the transcript highlight.
    on_position: PositionCallback,
    /// Called with ffmpeg's reason when playback fails, so the app can say so.
    on_error: ErrorCallback,
    ticking: Rc<Cell<bool>>,
}

impl Player {
    pub fn new() -> Self {
        let button = gtk::Button::builder()
            .icon_name("media-playback-start-symbolic")
            .tooltip_text(momr_core::locales::t("player.play"))
            .valign(gtk::Align::Center)
            .css_classes(["circular", "flat"])
            .build();
        let wave = gtk::DrawingArea::builder()
            .content_height(56)
            .hexpand(true)
            .build();
        wave.set_cursor_from_name(Some("pointer"));
        let time = gtk::Label::builder()
            .label("00:00")
            .css_classes(["numeric", "caption", "dim-label"])
            .valign(gtk::Align::Center)
            .build();
        let root = gtk::Box::builder()
            .spacing(10)
            .css_classes(["card", "player"])
            .build();

        root.append(&button);
        root.append(&wave);
        root.append(&time);

        let player = Player {
            root,
            button,
            wave,
            time,
            state: Rc::default(),
            on_position: Rc::default(),
            on_error: Rc::default(),
            ticking: Rc::default(),
        };

        let this = player.clone();
        player.wave.set_draw_func(move |_, cr, width, height| {
            this.draw(cr, f64::from(width), f64::from(height));
        });

        let this = player.clone();
        player.button.connect_clicked(move |_| this.toggle());

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

        // Hovering a chapter marker shows its title.
        player.wave.set_has_tooltip(true);
        let this = player.clone();
        player
            .wave
            .connect_query_tooltip(move |_, x, _, _, tooltip| {
                match this.chapter_near(f64::from(x)) {
                    Some(title) => {
                        tooltip.set_text(Some(&title));
                        true
                    }
                    None => false,
                }
            });

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
                .map(|p| probe_duration_us(p))
                .max()
                .unwrap_or(0);
            state.files = playable.clone();
            state.paused_at_us = 0;
            state.generation += 1;
            state.generation
        };
        self.root.set_visible(!playable.is_empty());

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
            if let Ok((Some(mic), computer)) = peaks {
                let mut state = this.state.borrow_mut();
                if state.generation == generation {
                    let computer = computer.flatten().unwrap_or_else(|| vec![0.0; BINS]);
                    state.peaks = Some((mic, computer));
                }
            }
            this.wave.queue_draw();
        });
        self.refresh();
    }

    /// Shows chapter markers on the waveform; empty removes them.
    pub fn set_chapters(&self, chapters: Vec<(i64, String)>) {
        self.state.borrow_mut().chapters = chapters;
        self.wave.queue_draw();
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
        drop(state);
        self.set_playing_icon(false);
        self.wave.queue_draw();
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
        let started = {
            let mut state = self.state.borrow_mut();
            if state.files.is_empty() {
                return;
            }
            if state.paused_at_us >= state.duration_us {
                state.paused_at_us = 0;
            }
            state.playback = None;
            playback::Playback::start(&state.files, state.paused_at_us, "audiotoolbox").map(
                |playback| {
                    state.playback = Some(playback);
                },
            )
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
            momr_core::locales::t("player.pause")
        } else {
            momr_core::locales::t("player.play")
        }));
    }

    fn start_ticking(&self) {
        if self.ticking.replace(true) {
            return;
        }
        let this = self.clone();
        glib::timeout_add_local(Duration::from_millis(100), move || {
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
            this.refresh();
            if this.is_playing() {
                glib::ControlFlow::Continue
            } else {
                this.ticking.set(false);
                glib::ControlFlow::Break
            }
        });
    }

    fn refresh(&self) {
        let (position, duration) = (self.position_us(), self.duration_us());
        self.time.set_label(&format!(
            "{} / {}",
            clock(position / 1_000_000),
            clock(duration / 1_000_000)
        ));
        self.wave.queue_draw();
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
        let mid = height / 2.0;
        let step = width / BINS as f64;
        let bar = (step * 0.8).max(1.0);
        let head = played * width;

        match state.peaks.as_ref() {
            Some((mic, computer)) => {
                for i in 0..BINS {
                    let x = i as f64 * step;
                    let alpha = if x <= head { 1.0 } else { 0.38 };
                    let up = (f64::from(mic[i]) * (mid - 2.0)).max(0.5);
                    let down = (f64::from(computer[i]) * (mid - 2.0)).max(0.5);
                    let (r, g, b) = crate::theme::color("blue", MIC_COLOR);
                    cr.set_source_rgba(r, g, b, alpha);
                    cr.rectangle(x, mid - up, bar, up);
                    let _ = cr.fill();
                    let (r, g, b) = crate::theme::color("orange", SYSTEM_COLOR);
                    cr.set_source_rgba(r, g, b, alpha);
                    cr.rectangle(x, mid, bar, down);
                    let _ = cr.fill();
                }
            }
            None => {
                cr.set_source_rgba(0.5, 0.5, 0.5, 0.3);
                cr.rectangle(0.0, mid - 0.5, width, 1.0);
                let _ = cr.fill();
            }
        }
        // Markers and playhead in the text colour, so they show on light themes too.
        let ink = self.wave.color();
        let (ir, ig, ib) = (
            f64::from(ink.red()),
            f64::from(ink.green()),
            f64::from(ink.blue()),
        );
        if duration > 0 {
            // Chapter markers: a thin line with a small notch at the top.
            for (ms, _) in &state.chapters {
                let x = ((*ms * 1000) as f64 / duration as f64 * width).round() + 0.5;
                cr.set_source_rgba(ir, ig, ib, 0.35);
                cr.rectangle(x - 0.5, 0.0, 1.0, height);
                let _ = cr.fill();
                cr.set_source_rgba(ir, ig, ib, 0.85);
                cr.move_to(x - 4.0, 0.0);
                cr.line_to(x + 4.0, 0.0);
                cr.line_to(x, 5.0);
                cr.close_path();
                let _ = cr.fill();
            }
            cr.set_source_rgba(ir, ig, ib, 0.9);
            cr.rectangle(head.round() - 0.5, 0.0, 1.5, height);
            let _ = cr.fill();
        }
    }
}
