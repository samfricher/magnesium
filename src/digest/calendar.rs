//! `magnesium calendar`: the plans' session lines as an iCalendar file, for
//! the learner's own calendar. It reads the plan files and writes nothing.
//!
//! Each session line is one weekly event on its days at its start time. The
//! times are floating (RFC 5545, 3.3.5): a plan says "08:00" on the learner's
//! own clock and names no zone, so a floating time stays 08:00 through summer
//! time and in another zone, and the file needs no VTIMEZONE tables.
//!
//! A series starts on the first of its days on or after the day the file is
//! made, so the learner's calendar never shows a planned session in the past.
//! Imported again later, the same session (same UID) starts on that later
//! day, and a calendar that takes the update drops the occurrences before it.
use super::sessions::{self, PlannedLine, SessionLines};
use super::{Table, Unreadable, is_placeholder, read_small, section};
use crate::workspace::Workspace;
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveTime, Utc, Weekday};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;

/// How long an event lasts when its line gives a start and no end.
const MINUTES_WITHOUT_AN_END: i64 = 30;
/// RFC 5545, 3.1: a content line is folded after 75 octets.
const FOLD_AT: usize = 75;

/// A plan's sessions and what in its Sessions section could not be read.
pub(super) struct PlanSessions {
    pub(super) target: String,
    pub(super) title: Option<String>,
    /// The day the plan is for, from its Facts table's When row.
    pub(super) date: Option<NaiveDate>,
    /// When the plan file last changed, for each event's LAST-MODIFIED.
    pub(super) modified: Option<DateTime<Utc>>,
    pub(super) lines: SessionLines,
}

/// One event of the file: a session line and the plan it is in.
struct Event<'a> {
    plan: &'a PlanSessions,
    line: &'a PlannedLine,
    /// The first of its days on or after the day the file is made.
    first: NaiveDate,
    /// Among the plan's sessions on the same days at the same times, which
    /// one this is, counted from 0: two such lines are still two events.
    twin: usize,
}

/// Each plan under `Study/` with its session lines, in the order of the
/// plans' folder names, and each plan file that could not be read.
pub(super) fn plans(root: &Path) -> (Vec<PlanSessions>, Vec<String>) {
    let mut notes = Vec::new();
    let Ok(entries) = fs::read_dir(root.join("Study")) else {
        return (Vec::new(), notes);
    };
    let mut targets = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with('.'))
        .collect::<Vec<_>>();
    targets.sort();
    let mut found = Vec::new();
    for target in targets {
        let file = super::plan_file(&target);
        let path = root.join(&file);
        if fs::symlink_metadata(&path).is_err() {
            continue;
        }
        let Some(plan) = read_small(&path) else {
            notes.push(file);
            continue;
        };
        found.push(PlanSessions {
            title: fact(&plan, "Target"),
            date: fact(&plan, "When").as_deref().and_then(super::first_date),
            modified: fs::metadata(&path)
                .and_then(|meta| meta.modified())
                .ok()
                .map(DateTime::<Utc>::from),
            lines: sessions::lines(&file, &plan),
            target,
        });
    }
    (found, notes)
}

fn fact(plan: &str, name: &str) -> Option<String> {
    let facts = Table::parse(section(plan, "Facts"));
    facts
        .rows
        .iter()
        .find(|row| {
            facts
                .cell(row, "Item")
                .is_some_and(|c| c.eq_ignore_ascii_case(name))
        })
        .and_then(|row| facts.cell(row, "Value"))
        .filter(|value| !is_placeholder(value))
        .map(str::to_owned)
}

/// What the learner does in a session, in the line's own words: what follows
/// "then I", as a title. "If it is Monday at 08:00, then I do the due cold
/// redos at my desk." is "Do the due cold redos at my desk".
pub(super) fn action(text: &str) -> String {
    // " then " is ASCII, so the lowered copy keeps the line's byte offsets.
    let lowered = text.to_ascii_lowercase();
    let rest = lowered
        .find(" then ")
        .map_or("", |at| &text[at + " then ".len()..]);
    let rest = rest.trim();
    let rest = ["I ", "i "]
        .iter()
        .find_map(|start| rest.strip_prefix(start))
        .unwrap_or(rest)
        .trim()
        .trim_end_matches(['.', ' ']);
    let mut chars = rest.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => "Study session".to_owned(),
    }
}

/// The first day on or after `from` that falls on one of the line's days.
fn first_day(line: &PlannedLine, from: NaiveDate) -> NaiveDate {
    from.iter_days()
        .take(7)
        .find(|day| line.days.contains(day.weekday()))
        .unwrap_or(from)
}

fn byday(day: Weekday) -> &'static str {
    match day {
        Weekday::Mon => "MO",
        Weekday::Tue => "TU",
        Weekday::Wed => "WE",
        Weekday::Thu => "TH",
        Weekday::Fri => "FR",
        Weekday::Sat => "SA",
        Weekday::Sun => "SU",
    }
}

const WEEK: [Weekday; 7] = [
    Weekday::Mon,
    Weekday::Tue,
    Weekday::Wed,
    Weekday::Thu,
    Weekday::Fri,
    Weekday::Sat,
    Weekday::Sun,
];

/// RFC 5545, 3.3.11: a TEXT value with its backslashes, semicolons, commas
/// and line breaks escaped, and other control characters left out.
fn text_value(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// One content line, folded after 75 octets without splitting a character,
/// each piece ended by CRLF (RFC 5545, 3.1).
fn content_line(out: &mut String, line: &str) {
    let mut room = FOLD_AT;
    let mut used = 0;
    for c in line.chars() {
        if used + c.len_utf8() > room {
            out.push_str("\r\n ");
            // The space that begins a continuation counts toward its 75.
            room = FOLD_AT - 1;
            used = 0;
        }
        out.push(c);
        used += c.len_utf8();
    }
    out.push_str("\r\n");
}

/// A stable UID for one session of one plan of one study folder, from the
/// session itself: its days and its times. Imported a second time, a session
/// the plan still holds has the same UID wherever it now sits in the list, so
/// a calendar updates its event instead of adding another, and a line added
/// or taken out above it moves nothing. A session whose days or times change
/// is a new event; the old one stays in the calendar until the learner
/// deletes it there, since an imported file cannot delete an event. The
/// words are left out so that a reworded line updates its event's name. The
/// folder's id is hashed so the file does not carry it.
fn uid(folder_id: &str, target: &str, line: &PlannedLine, twin: usize) -> String {
    let days = WEEK
        .iter()
        .filter(|day| line.days.contains(**day))
        .map(|day| byday(*day))
        .collect::<Vec<_>>()
        .join(",");
    let end = line
        .clock
        .end
        .map(|end| end.format("%H:%M").to_string())
        .unwrap_or_default();
    let session = format!(
        "{folder_id}\n{target}\n{days}\n{}\n{end}\n{twin}",
        line.clock.start.format("%H:%M")
    );
    let digest = Sha256::digest(session.as_bytes());
    let hex = digest
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{hex}@magnesium")
}

fn floating(day: NaiveDate, time: NaiveTime) -> String {
    day.and_time(time).format("%Y%m%dT%H%M%S").to_string()
}

/// What the file leaves out: each plan file and session line that could not
/// be read, each plan whose date has passed, and each session whose first
/// day comes after its plan's date.
struct LeftOut<'a> {
    unreadable_plans: Vec<String>,
    unreadable: Vec<&'a Unreadable>,
    ended: Vec<&'a PlanSessions>,
    too_late: Vec<(&'a PlanSessions, &'a PlannedLine, NaiveDate)>,
}

impl LeftOut<'_> {
    /// One line for each, as the command says them on its standard error.
    fn notes(&self) -> Vec<String> {
        let plans = self.unreadable_plans.iter().map(|file| {
            format!("{file}: not read, because it is too large, not text, or not a regular file")
        });
        let unreadable = self.unreadable.iter().map(|item| {
            format!(
                "{}, line {}: not read, because {}",
                item.file,
                item.line.unwrap_or_default(),
                item.reason
            )
        });
        let ended = self.ended.iter().map(|plan| {
            format!(
                "{}: left out, because its date, {}, has passed",
                super::plan_file(&plan.target),
                plan.date.map(|d| d.to_string()).unwrap_or_default()
            )
        });
        let too_late = self.too_late.iter().map(|(plan, line, first)| {
            format!(
                "{}: \"{}\" left out, because its first day, {first}, comes after the plan's date, {}",
                super::plan_file(&plan.target),
                action(&line.text),
                plan.date.map(|d| d.to_string()).unwrap_or_default()
            )
        });
        plans
            .chain(unreadable)
            .chain(ended)
            .chain(too_late)
            .collect()
    }
}

/// The events the file holds, and what it leaves out. A plan whose date has
/// passed is left out whole, its unreadable lines too: nothing in it could
/// be in the file.
fn events<'a>(
    plans: &'a [PlanSessions],
    unreadable_plans: Vec<String>,
    today: NaiveDate,
) -> (Vec<Event<'a>>, LeftOut<'a>) {
    let mut events: Vec<Event<'a>> = Vec::new();
    let mut left_out = LeftOut {
        unreadable_plans,
        unreadable: Vec::new(),
        ended: Vec::new(),
        too_late: Vec::new(),
    };
    for plan in plans {
        if plan.lines.planned.is_empty() && plan.lines.unreadable.is_empty() {
            continue;
        }
        if plan.date.is_some_and(|date| date < today) {
            left_out.ended.push(plan);
            continue;
        }
        left_out.unreadable.extend(&plan.lines.unreadable);
        for line in &plan.lines.planned {
            let first = first_day(line, today);
            // A series whose first day is past the plan's date has no
            // occurrence before it ends, and a calendar would still show its
            // DTSTART (RFC 5545, 3.8.5.3).
            if plan.date.is_some_and(|date| first > date) {
                left_out.too_late.push((plan, line, first));
                continue;
            }
            let twin = events
                .iter()
                .filter(|other| {
                    std::ptr::eq(other.plan, plan)
                        && other.line.days == line.days
                        && other.line.clock == line.clock
                })
                .count();
            events.push(Event {
                plan,
                line,
                first,
                twin,
            });
        }
    }
    (events, left_out)
}

/// The file itself, with CRLF line ends. `now` is the moment it is made.
fn ics(folder_id: &str, events: &[Event], now: DateTime<Utc>) -> String {
    let mut out = String::new();
    let mut line = |text: &str| content_line(&mut out, text);
    line("BEGIN:VCALENDAR");
    line("VERSION:2.0");
    line(&format!(
        "PRODID:-//Magnesium//Planned sessions {}//EN",
        env!("CARGO_PKG_VERSION")
    ));
    line("CALSCALE:GREGORIAN");
    line("METHOD:PUBLISH");
    let stamp = now.format("%Y%m%dT%H%M%SZ").to_string();
    // Each file is a later revision of every event in it than any file made
    // before it: minutes since the Unix epoch only grow (RFC 5545, 3.8.7.4).
    let sequence = now.timestamp().max(0) / 60;
    for event in events {
        let session = event.line;
        let first = event.first;
        let start = session.clock.start;
        // An end at or before the start is the next morning: 22:00 to 00:30.
        let (end_day, end) = match session.clock.end {
            Some(end) if end > start => (first, end),
            Some(end) => (first + Duration::days(1), end),
            None => {
                let end = first.and_time(start) + Duration::minutes(MINUTES_WITHOUT_AN_END);
                (end.date(), end.time())
            }
        };
        let days = WEEK
            .iter()
            .filter(|day| session.days.contains(**day))
            .map(|day| byday(*day))
            .collect::<Vec<_>>()
            .join(",");
        // A plan for a dated test stops on that day; a floating start takes
        // a floating UNTIL (RFC 5545, 3.3.10).
        let until = event
            .plan
            .date
            .map(|date| {
                format!(
                    ";UNTIL={}",
                    floating(
                        date,
                        NaiveTime::from_hms_opt(23, 59, 59).unwrap_or_default()
                    )
                )
            })
            .unwrap_or_default();
        let name = event.plan.title.as_deref().unwrap_or(&event.plan.target);
        line("BEGIN:VEVENT");
        line(&format!(
            "UID:{}",
            uid(folder_id, &event.plan.target, session, event.twin)
        ));
        line(&format!("DTSTAMP:{stamp}"));
        line(&format!("SEQUENCE:{sequence}"));
        if let Some(modified) = event.plan.modified {
            line(&format!(
                "LAST-MODIFIED:{}",
                modified.format("%Y%m%dT%H%M%SZ")
            ));
        }
        line(&format!("DTSTART:{}", floating(first, start)));
        line(&format!("DTEND:{}", floating(end_day, end)));
        line(&format!("RRULE:FREQ=WEEKLY;WKST=MO;BYDAY={days}{until}"));
        line(&format!("SUMMARY:{}", text_value(&action(&session.text))));
        line(&format!(
            "DESCRIPTION:{}",
            text_value(&format!(
                "{}\nFrom your Magnesium plan for {name}.",
                session.text
            ))
        ));
        line("END:VEVENT");
    }
    line("END:VCALENDAR");
    out
}

/// The sessions as data, for the window: each event's words, days and times,
/// and what the file leaves out.
fn listed(events: &[Event], left_out: &LeftOut) -> Value {
    json!({
        "sessions": events.iter().map(|event| {
            let session = event.line;
            json!({
                "plan": event.plan.target,
                "title": event.plan.title,
                "action": action(&session.text),
                "text": session.text,
                "days": WEEK.iter().filter(|day| session.days.contains(**day)).map(|day| byday(*day)).collect::<Vec<_>>(),
                "start": session.clock.start.format("%H:%M").to_string(),
                "end": session.clock.end.map(|end| end.format("%H:%M").to_string()),
            })
        }).collect::<Vec<_>>(),
        "unreadable": left_out.unreadable.iter().map(|item| item.to_json()).collect::<Vec<_>>(),
        "unreadable_plans": left_out.unreadable_plans,
        "ended": left_out.ended.iter().map(|plan| json!({
            "plan": plan.target,
            "title": plan.title,
            "date": plan.date.map(|d| d.to_string()),
        })).collect::<Vec<_>>(),
        "after_date": left_out.too_late.iter().map(|(plan, line, first)| json!({
            "plan": plan.target,
            "action": action(&line.text),
            "first": first.to_string(),
            "date": plan.date.map(|d| d.to_string()),
        })).collect::<Vec<_>>(),
    })
}

/// Set to a program that is handed the file in place of the Mac's `open`,
/// for `--open`. For tests, so a check never adds events to a real calendar.
const OPENER_ENV: &str = "MAGNESIUM_CALENDAR_OPENER";
/// A file `--open` made earlier is removed after this long: the calendar app
/// has read it by then.
const KEEP_OPENED_FILE: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// Write the file outside the study folder, in this user's temporary folder,
/// and hand it to the Mac, which opens it in the learner's calendar app.
/// Files an earlier `--open` left there are removed first.
fn open_in_calendar(root: &Path, file: &str) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("magnesium-calendar");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.filter_map(Result::ok) {
            let old = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|time| time.elapsed().ok())
                .is_some_and(|age| age > KEEP_OPENED_FILE);
            if old && entry.path().extension().is_some_and(|ext| ext == "ics") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().replace(['-', '_'], " "))
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "Study".to_owned());
    let path = dir.join(format!("{} sessions.ics", name.trim()));
    fs::write(&path, file).map_err(|e| e.to_string())?;
    let opener = std::env::var_os(OPENER_ENV)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(if cfg!(target_os = "macos") {
                "/usr/bin/open"
            } else {
                "xdg-open"
            })
        });
    let status = std::process::Command::new(opener)
        .arg(&path)
        .status()
        .map_err(|e| format!("the calendar file could not be opened: {e}"))?;
    if !status.success() {
        return Err("the calendar file could not be opened".into());
    }
    Ok(path)
}

pub(crate) fn print_calendar(as_json: bool, open: bool) -> ExitCode {
    let found = (|| {
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        let workspace = Workspace::discover(&cwd)
            .map_err(|e| e.to_string())?
            .ok_or("this folder is not set up for Magnesium; run `magnesium init` in it first")?;
        let folder_id = workspace.id().map_err(|e| e.to_string())?;
        Ok::<_, String>((workspace.root().to_path_buf(), folder_id))
    })();
    let (root, folder_id) = match found {
        Ok(found) => found,
        Err(error) => {
            eprintln!("magnesium: {error}");
            return ExitCode::FAILURE;
        }
    };
    let today = Local::now().date_naive();
    let (plans, unreadable_plans) = plans(&root);
    let (events, left_out) = events(&plans, unreadable_plans, today);
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&listed(&events, &left_out)).unwrap_or_default()
        );
        return ExitCode::SUCCESS;
    }
    for note in left_out.notes() {
        eprintln!("magnesium: {note}");
    }
    if events.is_empty() {
        eprintln!(
            "magnesium: no planned sessions to put in a calendar. Ask the tutor to plan when you study, and it writes them in your plan."
        );
        return ExitCode::FAILURE;
    }
    let file = ics(&folder_id, &events, Utc::now());
    if open {
        return match open_in_calendar(&root, &file) {
            Ok(_) => {
                println!(
                    "Your calendar app is opening {} planned {}. Choose the calendar to add them to.",
                    events.len(),
                    if events.len() == 1 {
                        "session"
                    } else {
                        "sessions"
                    }
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("magnesium: {error}");
                ExitCode::FAILURE
            }
        };
    }
    let mut stdout = std::io::stdout().lock();
    if stdout
        .write_all(file.as_bytes())
        .and_then(|()| stdout.flush())
        .is_err()
    {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests;
