//! The recording timer: a plan with up to three limits, any of them unset.
//! `max_secs` stops a recording after that much recorded time (pauses do
//! not count, like the clock on screen); `start_at` starts one from the
//! ready page at that moment; `stop_at` stops it at that moment. Times of
//! day are resolved to the next occurrence when the plan is set, so the
//! plan itself only holds Unix seconds and the shell (GTK: `ui.rs`) checks
//! it from its half-second tick. The plan lives for one session; stopping clears it.

use chrono::{DateTime, Days, Local, LocalResult, NaiveDate, NaiveTime, TimeZone};

use crate::locales::{t, tf};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub max_secs: Option<i64>,
    pub start_at: Option<i64>,
    pub stop_at: Option<i64>,
}

/// Why the Timer dialog's choices make no plan; each shell says it in its
/// own words (`timer.needs_length`, `timer.no_such_time`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// A length limit of zero minutes.
    NoLength,
    /// A clock time the local clock skips (a DST change) or cannot read.
    NoSuchTime,
}

impl Plan {
    /// The plan for what a Timer dialog chose: a length in seconds, and
    /// start and stop times of day as (hour, minute), each optional. The
    /// rules live here so every shell applies the same ones: a length must
    /// be positive, times resolve to their next occurrence, and a stop at
    /// or before the start means the day after it (found from the start,
    /// so a DST change between them is counted right).
    pub fn from_choices(
        length_secs: Option<i64>,
        start: Option<(u32, u32)>,
        stop: Option<(u32, u32)>,
        now: i64,
    ) -> Result<Plan, Refused> {
        let max_secs = match length_secs {
            Some(secs) if secs <= 0 => return Err(Refused::NoLength),
            other => other,
        };
        let at = |(hour, minute): (u32, u32), after: i64| {
            next_occurrence(hour, minute, after).ok_or(Refused::NoSuchTime)
        };
        let start_at = start.map(|time| at(time, now)).transpose()?;
        let stop_at = stop
            .map(|time| at(time, start_at.unwrap_or(now)))
            .transpose()?;
        Ok(Plan {
            max_secs,
            start_at,
            stop_at,
        })
    }

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
    let now = Local.timestamp_opt(now, 0).single()?;
    let time = NaiveTime::from_hms_opt(hour, minute, 0)?;
    let on = |day: NaiveDate| first_reading(day.and_time(time).and_local_timezone(Local));
    let today = now.date_naive();
    // Tomorrow is only asked for when today will not do, so a DST change
    // tomorrow cannot void a time that is fine today.
    if let Some(candidate) = on(today).filter(|candidate| *candidate > now) {
        return Some(candidate.timestamp());
    }
    on(today.checked_add_days(Days::new(1))?).map(|candidate| candidate.timestamp())
}

/// The earlier of the two instants a repeated local time names, or the one
/// instant, or None in a gap. Compared by timestamp rather than trusting
/// `LocalResult::earliest`: on macOS chrono 0.4.45 lists the later instant
/// first for `Local` (measured: 01:30 on 2026-11-01 in New York came back
/// as EST before EDT), so `earliest` picks the second pass.
pub(crate) fn first_reading(result: LocalResult<DateTime<Local>>) -> Option<DateTime<Local>> {
    match result {
        LocalResult::Single(t) => Some(t),
        LocalResult::Ambiguous(a, b) => Some(a.min(b)),
        LocalResult::None => None,
    }
}

/// "mm:ss", or "h:mm:ss" from an hour on: the one clock format of the app,
/// for the recording clock, the countdown and the player. Negative reads
/// 00:00, so a countdown that overshoots never shows "-1".
pub fn clock(secs: i64) -> String {
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
        assert_eq!(clock(59), "00:59");
        assert_eq!(clock(3661), "1:01:01");
        assert_eq!(clock(-5), "00:00");
        assert_eq!(Plan::default().describe(), "Off");
        let plan = Plan {
            max_secs: Some(600),
            ..Default::default()
        };
        assert_eq!(plan.describe(), "Stops after 10 min");
    }

    #[test]
    fn choices_become_a_plan_by_the_same_rules_everywhere() {
        let now = Local
            .with_ymd_and_hms(2026, 9, 25, 16, 0, 0)
            .single()
            .unwrap()
            .timestamp();
        assert_eq!(
            Plan::from_choices(Some(0), None, None, now),
            Err(Refused::NoLength)
        );
        assert_eq!(
            Plan::from_choices(None, Some((25, 0)), None, now),
            Err(Refused::NoSuchTime)
        );
        assert_eq!(
            Plan::from_choices(None, None, None, now),
            Ok(Plan::default())
        );
        let plan = Plan::from_choices(Some(600), Some((17, 0)), Some((18, 0)), now).unwrap();
        assert_eq!(plan.max_secs, Some(600));
        assert_eq!(plan.start_at, Some(now + 3600));
        assert_eq!(plan.stop_at, Some(now + 2 * 3600));
        // A stop before the start is the next day's.
        let overnight = Plan::from_choices(None, Some((23, 0)), Some((1, 0)), now).unwrap();
        assert_eq!(
            overnight.stop_at.unwrap() - overnight.start_at.unwrap(),
            2 * 3600
        );
        // Without a start, the stop is simply the next one.
        let stop_only = Plan::from_choices(None, None, Some((15, 0)), now).unwrap();
        assert_eq!(stop_only.stop_at, Some(now + 23 * 3600));
    }

    /// Runs the `dst_*` checks in a child test process pinned to a time
    /// zone with DST: TZ is process-wide, and setting it here would race
    /// every other test reading the local clock.
    #[test]
    fn next_occurrence_across_dst_changes() {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "timer::tests::dst_in_new_york", "--ignored"])
            .env("TZ", "America/New_York")
            .status()
            .unwrap();
        assert!(status.success(), "the DST checks failed in the child");
    }

    #[test]
    #[ignore = "run by next_occurrence_across_dst_changes with TZ pinned"]
    fn dst_in_new_york() {
        let at = |d, h, m| {
            Local
                .with_ymd_and_hms(2026, 11, d, h, m, 0)
                .earliest()
                .unwrap()
                .timestamp()
        };
        // 2026-11-01 01:30 happens twice; the first pass is the answer.
        assert_eq!(
            next_occurrence(1, 30, at(1, 0, 30)),
            Some(at(1, 0, 30) + 3600)
        );
        // The day before the change, a time that is fine today stays today,
        // however tomorrow's clock behaves.
        assert_eq!(
            next_occurrence(1, 30, at(1, 0, 10) - 24 * 3600)
                .map(|t| t - (at(1, 0, 10) - 24 * 3600)),
            Some(80 * 60)
        );
        let spring = |h, m| {
            Local
                .with_ymd_and_hms(2026, 3, 8, h, m, 0)
                .earliest()
                .unwrap()
                .timestamp()
        };
        // 2026-03-08 02:30 does not exist: tomorrow's 02:30 it is.
        let next = next_occurrence(2, 30, spring(1, 0)).unwrap();
        assert_eq!(next, spring(1, 0) + 24 * 3600 + 90 * 60 - 3600);
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
