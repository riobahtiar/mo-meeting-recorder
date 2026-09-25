//! The recording timer: a plan with up to three limits, any of them unset.
//! `max_secs` stops a recording after that much recorded time (pauses do
//! not count, like the clock on screen); `start_at` starts one from the
//! ready page at that moment; `stop_at` stops it at that moment. Times of
//! day are resolved to the next occurrence when the plan is set, so the
//! plan itself only holds Unix seconds and the shell (GTK: `ui.rs`) checks
//! it from its half-second tick. The plan lives for one session; stopping clears it.

use chrono::{Days, Local, NaiveDate, NaiveTime, TimeZone};

use crate::locales::{t, tf};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub max_secs: Option<i64>,
    pub start_at: Option<i64>,
    pub stop_at: Option<i64>,
}

impl Plan {
    /// A scheduled start whose time has come.
    pub fn due_start(&self, now: i64) -> bool {
        self.start_at.is_some_and(|at| now >= at)
    }

    /// Whether a recording that has run `elapsed` seconds should stop now.
    pub fn due_stop(&self, now: i64, elapsed: i64) -> bool {
        self.remaining(now, elapsed).is_some_and(|left| left <= 0)
    }

    /// Seconds until the sooner of the two stop limits, None without one.
    pub fn remaining(&self, now: i64, elapsed: i64) -> Option<i64> {
        let by_length = self.max_secs.map(|max| max - elapsed);
        let by_clock = self.stop_at.map(|at| at - now);
        match (by_length, by_clock) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// The plan in words for the Timer row: "Off", "Stops after 45 min",
    /// "Starts 14:00 · Stops 15:00".
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(at) = self.start_at {
            parts.push(tf("timer.starts_at", &[&clock_time(at)]));
        }
        if let Some(max) = self.max_secs {
            parts.push(tf("timer.stops_after", &[&length(max)]));
        }
        if let Some(at) = self.stop_at {
            parts.push(tf("timer.stops_at", &[&clock_time(at)]));
        }
        if parts.is_empty() {
            t("timer.off").to_owned()
        } else {
            parts.join(" · ")
        }
    }
}

/// "1 h 30 min", "45 min", "2 h".
pub fn length(secs: i64) -> String {
    let (h, m) = (secs / 3600, secs / 60 % 60);
    match (h, m) {
        (0, m) => tf("timer.minutes", &[&m.to_string()]),
        (h, 0) => tf("timer.hours", &[&h.to_string()]),
        (h, m) => format!(
            "{} {}",
            tf("timer.hours", &[&h.to_string()]),
            tf("timer.minutes", &[&m.to_string()])
        ),
    }
}

/// A Unix time as the local clock, with the day when it is not today.
/// Out-of-range times read empty.
pub fn clock_time(at: i64) -> String {
    let Some(when) = Local.timestamp_opt(at, 0).single() else {
        return String::new();
    };
    let today = Local::now().date_naive() == when.date_naive();
    let format = if today { "%H:%M" } else { "%a %H:%M" };
    when.format(format).to_string()
}

/// The next time the local clock reads `hour:minute` after `now`, as Unix
/// seconds: later today when that is still ahead, else tomorrow. A time the
/// clock reads twice (the hour repeated when DST ends) is its first
/// reading; a time the clock skips today falls through to tomorrow. None
/// only when tomorrow skips it too or the time cannot exist (25:00), which
/// the caller tells the user rather than setting a timer that never fires.
pub fn next_occurrence(hour: u32, minute: u32, now: i64) -> Option<i64> {
    let now = Local.timestamp_opt(now, 0).earliest()?;
    let time = NaiveTime::from_hms_opt(hour, minute, 0)?;
    let on = |day: NaiveDate| day.and_time(time).and_local_timezone(Local).earliest();
    let today = now.date_naive();
    // Tomorrow is only asked for when today will not do, so a DST change
    // tomorrow cannot void a time that is fine today.
    if let Some(candidate) = on(today).filter(|candidate| *candidate > now) {
        return Some(candidate.timestamp());
    }
    on(today.checked_add_days(Days::new(1))?).map(|candidate| candidate.timestamp())
}

/// The "mm:ss" or "h:mm:ss" countdown for the status line.
pub fn countdown(secs: i64) -> String {
    let secs = secs.max(0);
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sooner_limit_wins() {
        let plan = Plan {
            max_secs: Some(600),
            start_at: None,
            stop_at: Some(1_000_100),
        };
        // 100 s to the clock limit, 500 s to the length limit.
        assert_eq!(plan.remaining(1_000_000, 100), Some(100));
        assert!(!plan.due_stop(1_000_000, 100));
        assert!(plan.due_stop(1_000_100, 100));
        assert!(plan.due_stop(1_000_000, 600));
        let only_length = Plan {
            max_secs: Some(60),
            ..Default::default()
        };
        assert_eq!(only_length.remaining(5, 30), Some(30));
        assert_eq!(Plan::default().remaining(5, 30), None);
        assert!(!Plan::default().due_stop(5, 1_000_000));
    }

    #[test]
    fn a_scheduled_start_is_due_at_its_time() {
        let plan = Plan {
            start_at: Some(50),
            ..Default::default()
        };
        assert!(!plan.due_start(49));
        assert!(plan.due_start(50));
        assert!(!Plan::default().due_start(1_000_000));
    }

    #[test]
    fn lengths_and_countdowns_read_well() {
        assert_eq!(length(45 * 60), "45 min");
        assert_eq!(length(2 * 3600), "2 h");
        assert_eq!(length(90 * 60), "1 h 30 min");
        assert_eq!(countdown(59), "00:59");
        assert_eq!(countdown(3661), "1:01:01");
        assert_eq!(countdown(-5), "00:00");
        assert_eq!(Plan::default().describe(), "Off");
        let plan = Plan {
            max_secs: Some(600),
            ..Default::default()
        };
        assert_eq!(plan.describe(), "Stops after 10 min");
    }

    #[test]
    fn next_occurrence_is_today_or_tomorrow() {
        let now = Local
            .with_ymd_and_hms(2026, 9, 25, 16, 0, 0)
            .single()
            .unwrap()
            .timestamp();
        let later = next_occurrence(17, 30, now).unwrap();
        assert_eq!(later - now, 90 * 60);
        let earlier = next_occurrence(15, 0, now).unwrap();
        assert_eq!(earlier - now, 23 * 3600);
        // The same minute counts as passed: it would start at once.
        assert_eq!(next_occurrence(16, 0, now).unwrap() - now, 24 * 3600);
    }
}
