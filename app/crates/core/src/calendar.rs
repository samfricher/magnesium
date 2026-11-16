//! The plans' planned sessions, as `magnesium calendar` reads them, and the
//! calendar file it prints, handed to the Mac to open.
//!
//! The runtime reads the plan and writes the file, so the window shows the
//! same sessions a terminal's `magnesium calendar` puts in the file. The file
//! is written outside the study folder, in the window's own temporary folder,
//! and nothing in the study folder changes.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runtime;

/// Set this to a program that is handed the calendar file in place of the
/// Mac's `open`. For tests, so a check never adds events to a real calendar.
pub const OPEN_ENV: &str = "MAGNESIUM_APP_OPEN_CALENDAR";

/// One planned session, as the runtime read it from a plan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedSession {
    /// The plan's key, the name of its folder under `Study/`.
    pub plan: String,
    /// The plan's title, when its Facts table gives one.
    pub title: Option<String>,
    /// What the learner does, in the line's own words: "Do the due cold redos at my desk".
    pub action: String,
    /// Its days, as two letters each, Monday first: ["MO", "TU"].
    pub days: Vec<String>,
    /// "08:00".
    pub start: String,
    /// "09:00", when the line gives an end.
    pub end: Option<String>,
}

/// The sessions a folder's plans hold.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Planned {
    pub sessions: Vec<PlannedSession>,
    /// How many session lines the runtime could not read. They are not in
    /// the calendar file; the tutor can fix them.
    pub unreadable: usize,
    /// How many plan files the runtime could not read at all.
    #[serde(default)]
    pub unreadable_plans: usize,
}

/// What `magnesium calendar --json` printed, or None when it is not that
/// command's answer, as from a runtime from before the command.
pub fn planned_from(printed: &str) -> Option<Planned> {
    let value: Value = serde_json::from_str(printed).ok()?;
    let rows = value["sessions"].as_array()?;
    let text = |row: &Value, key: &str| {
        row[key]
            .as_str()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
    };
    Some(Planned {
        sessions: rows
            .iter()
            .filter_map(|row| {
                Some(PlannedSession {
                    plan: text(row, "plan")?,
                    title: text(row, "title"),
                    action: text(row, "action")?,
                    days: row["days"]
                        .as_array()?
                        .iter()
                        .filter_map(|day| day.as_str().map(str::to_owned))
                        .collect(),
                    start: text(row, "start")?,
                    end: text(row, "end"),
                })
            })
            .collect(),
        unreadable: value["unreadable"].as_array().map_or(0, Vec::len),
        unreadable_plans: value["unreadable_plans"].as_array().map_or(0, Vec::len),
    })
}

/// A folder's planned sessions. None when the runtime has no `calendar`
/// command or cannot read the folder: the window then offers nothing.
pub fn planned(folder: &Path) -> Option<Planned> {
    let out = runtime::run_in(folder, &["calendar", "--json"], Duration::from_secs(10)).ok()?;
    planned_from(&out)
}

/// "Intro Statistics sessions.ics", from the folder's name.
fn file_name(folder: &Path) -> String {
    let name = folder
        .file_name()
        .map(|n| n.to_string_lossy().replace(['-', '_'], " "))
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "Study".to_owned());
    format!("{name} sessions.ics")
}

/// A file written earlier is removed after this long: the calendar app has
/// read it by then.
const KEEP_FILE: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// Take away the calendar files an earlier write left, once the calendar app
/// has had time to read them.
fn remove_old(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let old = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|time| time.elapsed().ok())
            .is_some_and(|age| age > KEEP_FILE);
        if old && entry.path().extension().is_some_and(|ext| ext == "ics") {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Ask the runtime for the folder's calendar file, write it to the window's
/// own temporary folder, and return where.
pub fn write(folder: &Path) -> Result<PathBuf, String> {
    let file = runtime::run_in(folder, &["calendar"], Duration::from_secs(10))
        .map_err(|error| error.trim_start_matches("magnesium: ").to_owned())?;
    if !file.starts_with("BEGIN:VCALENDAR") {
        return Err("Magnesium did not give a calendar file.".into());
    }
    let dir = env::temp_dir()
        .join("dev.magnesium.window")
        .join("calendar");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    remove_old(&dir);
    let path = dir.join(file_name(folder));
    fs::write(&path, file).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Write the folder's calendar file and hand it to the Mac, which opens it
/// in the learner's calendar app (Calendar unless they chose another), where
/// they choose the calendar and add the events.
pub fn add_to_calendar(folder: &Path) -> Result<PathBuf, String> {
    let path = write(folder)?;
    let opener = env::var_os(OPEN_ENV)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(if cfg!(target_os = "macos") {
                "/usr/bin/open"
            } else {
                "xdg-open"
            })
        });
    let status = std::process::Command::new(&opener)
        .arg(&path)
        .status()
        .map_err(|e| format!("The calendar file could not be opened: {e}"))?;
    if !status.success() {
        return Err("The calendar file could not be opened.".into());
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_runtime_s_sessions_are_read_and_a_row_without_its_words_is_left_out() {
        let printed = r#"{
          "sessions": [
            {"plan": "midterm-2", "title": "Midterm 2", "action": "Do the due cold redos", "text": "If it is ...",
             "days": ["MO","TU","WE","TH","FR"], "start": "08:00", "end": null},
            {"plan": "midterm-2", "title": null, "action": "", "days": ["SA"], "start": "10:00", "end": "11:30"}
          ],
          "unreadable": [{"file": "Study/midterm-2/plan.md", "section": "Sessions", "line": 36, "reason": "..."}],
          "ended": []
        }"#;
        let planned = planned_from(printed).expect("the command's answer");
        assert_eq!(planned.sessions.len(), 1);
        assert_eq!(planned.sessions[0].days, ["MO", "TU", "WE", "TH", "FR"]);
        assert_eq!(planned.sessions[0].end, None);
        assert_eq!(planned.unreadable, 1);
        assert_eq!(planned.unreadable_plans, 0);
        assert_eq!(
            planned_from("error: unrecognized subcommand 'calendar'"),
            None
        );
    }

    #[test]
    fn an_old_calendar_file_is_removed_and_a_new_one_kept() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("Old sessions.ics");
        let new = dir.path().join("New sessions.ics");
        let other = dir.path().join("notes.txt");
        for file in [&old, &new, &other] {
            fs::write(file, "x").unwrap();
        }
        let hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        for file in [&old, &other] {
            fs::File::options()
                .write(true)
                .open(file)
                .unwrap()
                .set_modified(hour_ago)
                .unwrap();
        }
        remove_old(dir.path());
        assert!(!old.exists());
        assert!(
            new.exists(),
            "a file the calendar app may still be reading stays"
        );
        assert!(other.exists(), "only calendar files are taken away");
    }

    #[test]
    fn the_file_is_named_after_the_folder() {
        assert_eq!(
            file_name(Path::new("/x/Intro-Statistics")),
            "Intro Statistics sessions.ics"
        );
        assert_eq!(file_name(Path::new("/")), "Study sessions.ics");
    }
}
