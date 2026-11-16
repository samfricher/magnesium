//! The daily notification: on a day when something is due, one notification
//! that names the work, scheduled ahead with the Mac so it arrives with the
//! window closed. This module decides what to schedule; the shell hands it to
//! the Mac.
//!
//! What is due comes from the runtime's record (`magnesium progress`), today
//! and on each of the next seven days if nothing is answered first, and the
//! time from the plans' planned sessions (`magnesium calendar`). The whole
//! schedule is worked out again from the record each time: when the window
//! opens, when the learner changes the setting, and when the runtime commits
//! a change to the record, which starts the window's program to do it
//! (`magnesium _internal window-notifier`). So a notification never names
//! work that was done after it was scheduled, as long as the runtime that
//! marked the work is one that starts the window's program.
//!
//! It never says anything about days without study.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Weekday};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::calendar::{self, PlannedSession};
use crate::runtime;

/// How many days ahead the next day with work due is looked for, today
/// first. Only that next one is handed to the Mac: it is worked out again
/// every time the window opens or the record changes, and a build of the app
/// cannot reach what an earlier build handed over (see `Kept::build`), so
/// after an update at most one can arrive that cannot be changed.
pub const LOOK_AHEAD: i64 = 7;
/// Every request this module makes starts with this, so the shell can tell
/// its own from any other.
pub const ID_PREFIX: &str = "due-";
/// The time a notification comes on a day no planned session gives one.
pub const DEFAULT_TIME: &str = "09:00";
const FILE: &str = "notification.json";

/// The learner's setting, and what was last scheduled, kept apart from the
/// window's own state file: the program that schedules from the runtime
/// writes it with no window open.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kept {
    #[serde(default)]
    pub on: bool,
    /// "HH:MM": when a notification comes on a day with no planned session.
    #[serde(default)]
    pub time: Option<String>,
    /// What was last handed to the Mac.
    #[serde(default)]
    pub scheduled: Vec<Notice>,
    /// Each day a notification was due to arrive on, so a day never has a
    /// second one, even when its time moves later after the first arrived.
    #[serde(default)]
    pub fired: BTreeSet<NaiveDate>,
    /// Which build of the app scheduled `scheduled`. The Mac keeps an app's
    /// requests under its code signature: a build signed differently (every
    /// update of an app signed ad hoc) cannot see or take away what an
    /// earlier build scheduled, and the Mac still delivers it. Checked on
    /// 2026-11-16, macOS 27: a request from the replaced build arrived on
    /// time and the new build listed none.
    #[serde(default)]
    pub build: Option<String>,
    /// What an earlier build handed to the Mac and this one cannot reach:
    /// it still arrives, even with the notification turned off.
    #[serde(default)]
    pub held: Vec<Notice>,
    /// Why the notification was turned off by the window rather than the
    /// learner, said in Settings until they turn it on again.
    #[serde(default)]
    pub problem: Option<String>,
}

impl Kept {
    pub fn read(root: &Path) -> Self {
        fs::read_to_string(root.join(FILE))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn write(&self, root: &Path) {
        let Ok(text) = serde_json::to_string_pretty(self) else {
            return;
        };
        if fs::create_dir_all(root).is_err() {
            return;
        }
        let temp = root.join(format!("{FILE}.{}.tmp", std::process::id()));
        if fs::write(&temp, text).is_ok() {
            let _ = fs::rename(&temp, root.join(FILE));
        }
    }

    pub fn time(&self) -> NaiveTime {
        self.time
            .as_deref()
            .and_then(clock)
            .or_else(|| clock(DEFAULT_TIME))
            .unwrap_or_default()
    }

    /// Note each scheduled notification whose time has come as having
    /// arrived, and forget days long past. `pending` is what this build
    /// finds still waiting with the Mac, when it can ask.
    ///
    /// Scheduled by another build, a notification still to come is this
    /// build's to replace only if the Mac lists it as this build's: the
    /// same signature, as with a stable one. Otherwise (every update of an
    /// app signed ad hoc) it arrives anyway and nothing can take it away, so
    /// it is kept as held and its day gets no second one.
    pub fn note_arrived(&mut self, now: NaiveDateTime, build: &str, pending: Option<&[String]>) {
        let other_build = self
            .build
            .as_deref()
            .is_some_and(|earlier| earlier != build);
        for notice in std::mem::take(&mut self.scheduled) {
            if notice.at <= now {
                self.fired.insert(notice.at.date());
            } else if other_build && !pending.is_some_and(|ids| ids.contains(&notice.id)) {
                self.fired.insert(notice.at.date());
                self.held.push(notice);
            } else {
                self.scheduled.push(notice);
            }
        }
        self.held.retain(|notice| notice.at > now);
        self.build = Some(build.to_owned());
        let long_ago = now.date() - Duration::days(LOOK_AHEAD);
        self.fired.retain(|day| *day >= long_ago);
    }
}

/// The first time on or after `at` that the local clock shows: a time the
/// clocks skip when they go forward (02:30 on that night, in many places)
/// becomes the first that exists after it.
pub fn existing(at: NaiveDateTime, exists: impl Fn(NaiveDateTime) -> bool) -> NaiveDateTime {
    (0..=12)
        .map(|quarter| at + Duration::minutes(15 * quarter))
        .find(|time| exists(*time))
        .unwrap_or(at)
}

fn exists_here(at: NaiveDateTime) -> bool {
    Local.from_local_datetime(&at).earliest().is_some()
}

/// One notification to schedule.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    /// "due-2026-11-17": one a day, so scheduling a day again replaces it.
    pub id: String,
    /// Local wall-clock time it comes at.
    pub at: NaiveDateTime,
    pub title: String,
    pub body: String,
    /// The workspace it opens the window on.
    pub folder: String,
}

/// What one workspace has due on one day.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Due {
    pub folder: String,
    pub name: String,
    pub checks: u64,
    pub redos: u64,
}

/// What the record and the plans say of one workspace, for the schedule.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    pub folder: String,
    pub name: String,
    /// The record's day: what `due` counts is due on it.
    pub today: Option<NaiveDate>,
    /// Due today, as the record counts it: (checks, redos).
    pub due: (u64, u64),
    /// Due on each later day if nothing is answered first.
    pub ahead: Vec<(NaiveDate, u64, u64)>,
    pub sessions: Vec<PlannedSession>,
}

impl Facts {
    fn due_on(&self, day: NaiveDate) -> (u64, u64) {
        if Some(day) == self.today {
            return self.due;
        }
        self.ahead
            .iter()
            .find(|(on, _, _)| *on == day)
            .map_or((0, 0), |(_, checks, redos)| (*checks, *redos))
    }

    /// The earliest planned session that falls on `day`.
    fn session_on(&self, day: NaiveDate) -> Option<NaiveTime> {
        let code = match day.weekday() {
            Weekday::Mon => "MO",
            Weekday::Tue => "TU",
            Weekday::Wed => "WE",
            Weekday::Thu => "TH",
            Weekday::Fri => "FR",
            Weekday::Sat => "SA",
            Weekday::Sun => "SU",
        };
        self.sessions
            .iter()
            .filter(|session| session.days.iter().any(|d| d == code))
            .filter_map(|session| clock(&session.start))
            .min()
    }
}

fn clock(text: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(text.trim(), "%H:%M").ok()
}

/// "Intro-Statistics" as the window names it, "Intro Statistics".
fn display_name(folder: &str) -> String {
    let name = Path::new(folder)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| folder.to_owned());
    let shown = name.replace(['-', '_'], " ");
    let shown = shown.split_whitespace().collect::<Vec<_>>().join(" ");
    if shown.is_empty() {
        name
    } else {
        shown
    }
}

/// What one workspace has due, in plain words: "2 quick checks and 1 paper redo".
fn work(checks: u64, redos: u64) -> String {
    let plural = |n: u64, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    match (checks, redos) {
        (0, r) => plural(r, "paper redo", "paper redos"),
        (c, 0) => plural(c, "quick check", "quick checks"),
        (c, r) => format!(
            "{} and {}",
            plural(c, "quick check", "quick checks"),
            plural(r, "paper redo", "paper redos")
        ),
    }
}

/// The notification's words. It names the workspace and the work, and, when
/// it comes at a planned session, that session's time. "Due" with no day:
/// a notification the Mac delivers late, after a sleep, is still true.
pub fn words(dues: &[Due], session: Option<NaiveTime>) -> (String, String) {
    let at = session.map(|time| format!("Your {} session. ", time.format("%H:%M")));
    let at = at.unwrap_or_default();
    match dues {
        [one] => {
            let count = one.checks + one.redos;
            (
                one.name.clone(),
                format!(
                    "{at}{} {} due.",
                    work(one.checks, one.redos),
                    if count == 1 { "is" } else { "are" }
                ),
            )
        }
        _ => {
            let each = dues
                .iter()
                .map(|due| format!("{}: {}", due.name, work(due.checks, due.redos)))
                .collect::<Vec<_>>()
                .join(". ");
            ("Due in Magnesium".to_owned(), format!("{at}{each}."))
        }
    }
}

/// The next notification to schedule: the first day of the next week that
/// has something due and has not had one, at its time if that is still to
/// come. At most one, so at most one a day, and at most one that an update
/// can leave out of reach.
pub fn schedule(now: NaiveDateTime, kept: &Kept, facts: &[Facts]) -> Vec<Notice> {
    schedule_in(now, kept, facts, exists_here)
}

/// `schedule`, with the clock's own idea of which local times exist.
pub fn schedule_in(
    now: NaiveDateTime,
    kept: &Kept,
    facts: &[Facts],
    exists: impl Fn(NaiveDateTime) -> bool + Copy,
) -> Vec<Notice> {
    if !kept.on {
        return Vec::new();
    }
    let today = now.date();
    (0..LOOK_AHEAD)
        .map(|n| today + Duration::days(n))
        .filter(|day| !kept.fired.contains(day))
        .find_map(|day| {
            let due = facts
                .iter()
                .filter_map(|each| {
                    let (checks, redos) = each.due_on(day);
                    (checks + redos > 0).then(|| {
                        (
                            each,
                            Due {
                                folder: each.folder.clone(),
                                name: each.name.clone(),
                                checks,
                                redos,
                            },
                        )
                    })
                })
                .collect::<Vec<_>>();
            if due.is_empty() {
                return None;
            }
            // At the earliest session planned that day in a workspace with work due, else at the learner's time.
            let session = due
                .iter()
                .filter_map(|(each, _)| each.session_on(day))
                .min();
            let at = existing(day.and_time(session.unwrap_or_else(|| kept.time())), exists);
            if at <= now {
                return None;
            }
            // The workspace it opens on: the one whose session it comes at, else the first.
            let lead = due
                .iter()
                .find(|(each, _)| session.is_some() && each.session_on(day) == session)
                .unwrap_or(&due[0]);
            let dues = due.iter().map(|(_, due)| due.clone()).collect::<Vec<_>>();
            let (title, body) = words(&dues, session);
            Some(Notice {
                id: format!("{ID_PREFIX}{day}"),
                at,
                title,
                body,
                folder: lead.1.folder.clone(),
            })
        })
        .into_iter()
        .collect()
}

/// Tell the runtime to start `program` (the window's own, with the window's
/// folder `home`) after each change it records, or with None that it should
/// start none. An error when the runtime cannot be told, as a runtime from
/// before the command, or none found: the notification must not then stay on.
pub fn name_program(program: Option<&Path>, home: &Path) -> Result<(), String> {
    let mut args: Vec<String> = vec!["_internal".into(), "window-notifier".into()];
    match program {
        Some(program) => {
            args.push(program.to_string_lossy().into_owned());
            args.push("--home".into());
            args.push(home.to_string_lossy().into_owned());
        }
        None => args.push("--off".into()),
    }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    // The command reads no study folder; it runs anywhere.
    runtime::run_in(
        &std::env::temp_dir(),
        &args,
        std::time::Duration::from_secs(10),
    )
    .map(|_| ())
}

/// Run `work` alone, or leave it to the run already at it. The runtime may
/// start the window's program once for every answer it records, faster
/// than one run ends: a run that finds another holding the schedule's lock
/// leaves a mark and ends at once, and the holder runs once more before it
/// lets go. Runs never queue.
pub fn coalesce(root: &Path, mut work: impl FnMut()) {
    let _ = fs::create_dir_all(root);
    let again = root.join("notification.again");
    loop {
        let Ok(lock) = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(root.join("notification.lock"))
        else {
            work();
            return;
        };
        match lock.try_lock() {
            Ok(()) => {
                loop {
                    let _ = fs::remove_file(&again);
                    work();
                    if !again.exists() {
                        break;
                    }
                }
                let _ = lock.unlock();
                // A mark left between the last look and letting go: take it up.
                if !again.exists() {
                    return;
                }
            }
            Err(fs::TryLockError::WouldBlock) => {
                let _ = fs::write(&again, b"");
                return;
            }
            Err(fs::TryLockError::Error(_)) => {
                work();
                return;
            }
        }
    }
}

/// What the record and plans say of a workspace, or None when the runtime
/// cannot say (not set up, an earlier runtime, an earlier version's folder).
pub fn facts(folder: &Path) -> Option<Facts> {
    let progress = runtime::progress(folder)?;
    let today = progress["today"]
        .as_str()
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok());
    let count = |value: &Value| value.as_u64().unwrap_or(0);
    let ahead = progress["due_ahead"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|day| {
            let on = NaiveDate::parse_from_str(day["day"].as_str()?, "%Y-%m-%d").ok()?;
            Some((on, count(&day["checks"]), count(&day["redos"])))
        })
        .collect();
    let folder_text = folder.to_string_lossy().into_owned();
    Some(Facts {
        name: display_name(&folder_text),
        folder: folder_text,
        today,
        due: (
            count(&progress["due"]["checks"]),
            count(&progress["due"]["redos"]),
        ),
        ahead,
        sessions: calendar::planned(folder)
            .map(|planned| planned.sessions)
            .unwrap_or_default(),
    })
}

/// Local wall-clock now.
pub fn now() -> NaiveDateTime {
    Local::now().naive_local()
}

/// A local time as milliseconds since the epoch, for the page.
pub fn ms_of(at: NaiveDateTime) -> Option<i64> {
    Local
        .from_local_datetime(&at)
        .earliest()
        .map(|t| t.timestamp_millis())
}

/// Work out the schedule from each workspace in the library, note it as
/// scheduled, and return it. `folders` are the library's.
pub fn plan(
    root: &Path,
    folders: &[String],
    build: &str,
    pending: Option<&[String]>,
) -> (Kept, Vec<Notice>) {
    let mut kept = Kept::read(root);
    let now = now();
    kept.note_arrived(now, build, pending);
    let facts = if kept.on {
        folders
            .iter()
            .filter_map(|folder| facts(&PathBuf::from(folder)))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let notices = schedule(now, &kept, &facts);
    kept.scheduled = notices.clone();
    kept.write(root);
    (kept, notices)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    fn at(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M").unwrap()
    }

    fn session(days: &[&str], start: &str) -> PlannedSession {
        PlannedSession {
            plan: "midterm-2".into(),
            title: Some("Midterm 2".into()),
            action: "Do the due checks".into(),
            days: days.iter().map(|d| d.to_string()).collect(),
            start: start.into(),
            end: None,
        }
    }

    /// Monday 16 November 2026, with one check due today and still due on
    /// each later day, a redo falling due on Wednesday, and a weekday session at 08:00.
    fn stats() -> Facts {
        Facts {
            folder: "/s/Intro-Statistics".into(),
            name: "Intro Statistics".into(),
            today: Some(day("2026-11-16")),
            due: (1, 0),
            ahead: (1..7)
                .map(|n| (day("2026-11-16") + Duration::days(n), 1, u64::from(n >= 2)))
                .collect(),
            sessions: vec![session(&["MO", "TU", "WE", "TH", "FR"], "08:00")],
        }
    }

    fn on() -> Kept {
        Kept {
            on: true,
            ..Kept::default()
        }
    }

    #[test]
    fn off_schedules_nothing() {
        assert!(schedule(at("2026-11-16 07:00"), &Kept::default(), &[stats()]).is_empty());
    }

    #[test]
    fn the_next_day_with_work_due_at_its_session_or_the_learner_s_time_naming_the_work() {
        let notices = schedule(at("2026-11-16 07:00"), &on(), &[stats()]);
        assert_eq!(notices.len(), 1, "only the next one is handed over");
        assert_eq!(notices[0].id, "due-2026-11-16");
        assert_eq!(notices[0].at, at("2026-11-16 08:00"));
        assert_eq!(notices[0].title, "Intro Statistics");
        assert_eq!(notices[0].body, "Your 08:00 session. 1 quick check is due.");
        assert_eq!(notices[0].folder, "/s/Intro-Statistics");
        // Wednesday: a redo falls due as well.
        let wednesday = schedule(
            at("2026-11-18 07:00"),
            &on(),
            &[Facts {
                today: Some(day("2026-11-18")),
                due: (1, 1),
                ..stats()
            }],
        );
        assert_eq!(
            wednesday[0].body,
            "Your 08:00 session. 1 quick check and 1 paper redo are due."
        );
        // A day with no planned session: the learner's time, 09:00 unless they set one.
        let saturday = Facts {
            today: Some(day("2026-11-21")),
            ..stats()
        };
        let weekend = schedule(at("2026-11-21 07:00"), &on(), &[saturday]);
        assert_eq!(weekend[0].at, at("2026-11-21 09:00"));
        assert_eq!(weekend[0].body, "1 quick check is due.");
    }

    #[test]
    fn nothing_due_on_a_day_means_no_notification_that_day() {
        let mut quiet = stats();
        quiet.due = (0, 0);
        quiet.ahead = vec![(day("2026-11-18"), 0, 2)];
        let notices = schedule(at("2026-11-16 07:00"), &on(), &[quiet]);
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].id, "due-2026-11-18");
        assert_eq!(
            notices[0].body,
            "Your 08:00 session. 2 paper redos are due."
        );
        assert!(
            schedule(at("2026-11-16 07:00"), &on(), &[Facts::default()]).is_empty(),
            "nothing due all week"
        );
    }

    #[test]
    fn a_time_already_past_today_or_a_day_one_arrived_on_gets_none() {
        let late = schedule(at("2026-11-16 08:30"), &on(), &[stats()]);
        assert_eq!(late[0].id, "due-2026-11-17", "08:00 has passed today");
        let mut kept = on();
        kept.time = Some("21:00".into());
        let mut weekend = stats();
        weekend.sessions.clear();
        let first = schedule(at("2026-11-16 08:30"), &kept, &[weekend.clone()]);
        assert_eq!(first[0].at, at("2026-11-16 21:00"));
        // It arrives at 21:00; then the learner moves their time later. Today keeps the one it had.
        kept.scheduled = first;
        kept.note_arrived(at("2026-11-16 21:05"), "one", None);
        kept.time = Some("22:00".into());
        let again = schedule(at("2026-11-16 21:05"), &kept, &[weekend]);
        assert_eq!(
            again[0].id, "due-2026-11-17",
            "never a second one the same day"
        );
    }

    #[test]
    fn what_an_earlier_build_handed_over_is_held_only_when_this_build_cannot_reach_it() {
        let mut kept = on();
        kept.scheduled = schedule(at("2026-11-16 07:00"), &kept, &[stats()]);
        kept.build = Some("old".into());
        // The same build: nothing to come is taken or held.
        let mut same = kept.clone();
        same.note_arrived(at("2026-11-16 07:30"), "old", Some(&[]));
        assert!(same.fired.is_empty() && same.held.is_empty());
        // A new build with the same signature lists it as its own: it replaces it, and no day goes missing.
        let mut reachable = kept.clone();
        reachable.note_arrived(
            at("2026-11-16 07:30"),
            "new",
            Some(&["due-2026-11-16".to_owned()]),
        );
        assert!(reachable.fired.is_empty() && reachable.held.is_empty());
        assert_eq!(
            schedule(at("2026-11-16 07:30"), &reachable, &[stats()])[0].id,
            "due-2026-11-16"
        );
        // A new build that cannot see it (an update signed ad hoc): it still arrives, so it is held and its day has no second.
        let mut held = kept.clone();
        held.note_arrived(at("2026-11-16 07:30"), "new", Some(&[]));
        assert_eq!(held.held.len(), 1);
        assert_eq!(
            schedule(at("2026-11-16 07:30"), &held, &[stats()])[0].id,
            "due-2026-11-17"
        );
        // Turned off then: nothing is scheduled, and the held one is still said to be on its way.
        held.on = false;
        assert!(schedule(at("2026-11-16 07:30"), &held, &[stats()]).is_empty());
        assert_eq!(held.held[0].id, "due-2026-11-16");
        // Once its time has passed it is no longer held.
        held.note_arrived(at("2026-11-16 08:05"), "new", Some(&[]));
        assert!(held.held.is_empty());
        assert_eq!(held.build.as_deref(), Some("new"));
    }

    #[test]
    fn a_session_in_the_hour_the_clocks_skip_comes_at_the_first_time_that_exists() {
        // As where the clocks go from 02:00 to 03:00 on 2027-03-14: 02:00 to 02:59 do not exist.
        let gap = |t: NaiveDateTime| {
            !(t.date() == day("2027-03-14") && t.time().format("%H").to_string() == "02")
        };
        assert_eq!(
            existing(at("2027-03-14 02:30"), gap),
            at("2027-03-14 03:00")
        );
        assert_eq!(
            existing(at("2027-03-15 02:30"), gap),
            at("2027-03-15 02:30")
        );
        let night = Facts {
            today: Some(day("2027-03-14")),
            sessions: vec![session(&["SU"], "02:30")],
            ..stats()
        };
        let notices = schedule_in(at("2027-03-14 00:00"), &on(), &[night], gap);
        assert_eq!(notices[0].at, at("2027-03-14 03:00"));
        assert_eq!(notices[0].body, "Your 02:30 session. 1 quick check is due.");
    }

    #[test]
    fn a_run_that_finds_the_schedule_busy_leaves_a_mark_and_the_holder_runs_once_more() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        // Another run holds the lock.
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(root.join("notification.lock"))
            .unwrap();
        lock.lock().unwrap();
        let mut ran = 0;
        let started = std::time::Instant::now();
        coalesce(&root, || ran += 1);
        assert_eq!(ran, 0, "it does not wait or run");
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        assert!(root.join("notification.again").exists());
        lock.unlock().unwrap();
        // The next run to hold it runs once, takes the mark up, and runs no more.
        let mut runs = 0;
        coalesce(&root, || runs += 1);
        assert_eq!(runs, 1);
        assert!(!root.join("notification.again").exists());
        // A mark left while it runs makes it run once more.
        let mut times = 0;
        let mark = root.join("notification.again");
        coalesce(&root, || {
            times += 1;
            if times == 1 {
                fs::write(&mark, b"").unwrap();
            }
        });
        assert_eq!(times, 2);
    }

    #[test]
    fn work_answered_before_its_time_takes_that_day_s_notification_away() {
        let mut answered = stats();
        answered.due = (0, 0);
        let notices = schedule(at("2026-11-16 07:30"), &on(), &[answered]);
        assert_eq!(notices[0].id, "due-2026-11-17");
    }

    #[test]
    fn two_workspaces_on_one_day_share_one_notification() {
        let mut algebra = Facts {
            folder: "/s/Linear-Algebra".into(),
            name: "Linear Algebra".into(),
            today: Some(day("2026-11-16")),
            due: (0, 1),
            ahead: vec![],
            sessions: vec![session(&["MO"], "07:15")],
        };
        let notices = schedule(at("2026-11-16 07:00"), &on(), &[stats(), algebra.clone()]);
        assert_eq!(
            notices[0].at,
            at("2026-11-16 07:15"),
            "the earliest session among those with work due"
        );
        assert_eq!(notices[0].title, "Due in Magnesium");
        assert_eq!(
            notices[0].body,
            "Your 07:15 session. Intro Statistics: 1 quick check. Linear Algebra: 1 paper redo."
        );
        assert_eq!(
            notices[0].folder, "/s/Linear-Algebra",
            "it opens on the workspace whose session it is"
        );
        algebra.due = (0, 0);
        let notices = schedule(at("2026-11-16 07:00"), &on(), &[stats(), algebra]);
        assert_eq!(
            notices[0].at,
            at("2026-11-16 08:00"),
            "a session in a workspace with nothing due does not set the time"
        );
    }

    #[test]
    fn no_word_about_days_missed() {
        let notices = schedule(at("2026-11-16 07:00"), &on(), &[stats()]);
        for notice in notices {
            let words = format!("{} {}", notice.title, notice.body).to_lowercase();
            for banned in [
                "miss", "streak", "behind", "overdue", "days ago", "since", "late", "again",
            ] {
                assert!(!words.contains(banned), "{banned:?} in {words:?}");
            }
        }
    }
}
