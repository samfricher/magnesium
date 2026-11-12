//! The Magnesium runtime, reached through its own commands.
//!
//! The window links none of the runtime's code. It runs the `magnesium`
//! program the learner installed and reads what it prints, so the runtime
//! stays small and the two can be updated apart.

use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::paths;

/// Set this to use another program in place of `magnesium`. For tests.
pub const PROGRAM_ENV: &str = "MAGNESIUM_APP_RUNTIME";

pub fn program() -> Option<PathBuf> {
    if let Some(named) = env::var_os(PROGRAM_ENV).filter(|v| !v.is_empty()) {
        let named = PathBuf::from(named);
        return named.is_file().then_some(named);
    }
    paths::find_program("magnesium")
}

fn run(folder: &Path, args: &[&str], limit: Duration) -> Result<String, String> {
    let program = program().ok_or("the magnesium program is not installed")?;
    let mut command = paths::light_command(&program);
    command.current_dir(folder).args(args);
    paths::run(command, limit)
}

/// `magnesium digest`: the study folder and the learner record, summarized
/// without changing either.
pub fn digest(folder: &Path) -> Result<Value, String> {
    let out = run(folder, &["digest"], Duration::from_secs(10))?;
    serde_json::from_str(&out).map_err(|e| format!("its digest was not readable ({e})"))
}

/// `magnesium attempts --json`: one row per marked answer. None when this
/// version of the runtime has no such command, or the folder has no record.
pub fn attempts(folder: &Path) -> Option<Vec<Value>> {
    let out = run(folder, &["attempts", "--json"], Duration::from_secs(10)).ok()?;
    let value: Value = serde_json::from_str(&out).ok()?;
    value["attempts"].as_array().cloned()
}

/// One answer the tutor marked, as the runtime's record has it. The window
/// shows a mark only from a row like this, never from the tutor's words.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Mark {
    /// The record's own id for the answer, which no other answer has.
    pub id: String,
    /// When it was recorded, in local time, as the runtime prints it:
    /// "2026-11-12 09:41".
    pub at: String,
    /// The skill the answer was for.
    pub label: String,
    /// "right", "partly", "wrong" or "declined": the first try.
    pub mark: String,
    /// "none", "hint", "step" or "solution".
    pub help: String,
    /// Whether it was answered from a blank page.
    pub cold: bool,
    /// The paper the answer was given on, or none in a lesson.
    pub paper: Option<String>,
}

/// The marked answers in what `magnesium attempts --json` printed, oldest
/// first. None when the text is not that command's answer, as when a
/// runtime from before the command prints a complaint about it. A row
/// without an id or a mark is left out: the window shows nothing it cannot
/// stand behind.
pub fn marks_from(printed: &str) -> Option<Vec<Mark>> {
    let value: Value = serde_json::from_str(printed).ok()?;
    let rows = value["attempts"].as_array()?;
    Some(
        rows.iter()
            .filter_map(|row| {
                let text = |key: &str| {
                    row[key]
                        .as_str()
                        .map(str::trim)
                        .filter(|t| !t.is_empty())
                        .map(str::to_owned)
                };
                Some(Mark {
                    id: text("attempt_id")?,
                    at: text("answered_at").unwrap_or_default(),
                    label: text("label").unwrap_or_default(),
                    mark: text("mark")?,
                    help: text("help").unwrap_or_else(|| "none".to_owned()),
                    cold: row["cold"].as_bool().unwrap_or(false),
                    paper: text("paper"),
                })
            })
            .collect(),
    )
}

/// The folder's marked answers, oldest first. None when this version of the
/// runtime has no `attempts` command, or the folder has no record.
pub fn marks(folder: &Path) -> Option<Vec<Mark>> {
    let out = run(folder, &["attempts", "--json"], Duration::from_secs(10)).ok()?;
    marks_from(&out)
}

/// `magnesium progress --json`: each skill's passes, the level, what is due
/// and the days with a marked answer, worked out by the runtime from the
/// learner record. None when this version of the runtime has no such
/// command, when the folder is not set up, or when an earlier version set it
/// up: like the digest, the command never upgrades a folder.
pub fn progress(folder: &Path) -> Option<Value> {
    let out = run(folder, &["progress", "--json"], Duration::from_secs(10)).ok()?;
    let value: Value = serde_json::from_str(&out).ok()?;
    (value["active"].as_bool() == Some(true)
        && value["upgrade_needed"].as_bool() != Some(true)
        && value["days"].is_array())
    .then_some(value)
}

/// `magnesium init`: set a folder up for study.
pub fn init(folder: &Path) -> Result<(), String> {
    run(folder, &["init", "--json"], Duration::from_secs(30)).map(|_| ())
}

/// `magnesium init --upgrade-instructions`, in the study root: upgrade a
/// folder an earlier version set up. The runtime copies the record to
/// `.magnesium/backups` before it changes it, and switches the folder to this
/// version's teaching instructions. A large record takes a while to copy.
pub fn upgrade(root: &Path) -> Result<(), String> {
    run(
        root,
        &["init", "--upgrade-instructions", "--json"],
        Duration::from_secs(180),
    )
    .map(|_| ())
}

/// The lowest `window_features` this window works with. A runtime that says
/// less, or nothing, is refused: its version number cannot tell, because a
/// build from an older commit can carry the same one. 1 is the first runtime
/// whose `progress --json` counts what the learner did today. 2 is the first
/// that lists a sitting's due paper redos as data, which a lesson on the
/// direct engine poses. 3 is the first that lists the rows waiting for a
/// repair, which a lesson on the direct engine records.
pub const WINDOW_FEATURES_NEEDED: u64 = 3;

/// The `window_features` number that `magnesium digest` prints: which of the
/// window's needs this program meets. It is asked at the root of the disk, in
/// no study folder, so no record is read and nothing changes. None when the
/// program says no number, as every runtime from before it does.
pub fn window_features(program: &Path) -> Option<u64> {
    let mut command = paths::light_command(program);
    command.current_dir("/").arg("digest");
    let out = paths::run(command, Duration::from_secs(10)).ok()?;
    let value: Value = serde_json::from_str(&out).ok()?;
    value["window_features"].as_u64()
}

/// Whether a program is new enough for this window.
pub fn fits_window(program: &Path) -> bool {
    window_features(program).is_some_and(|number| number >= WINDOW_FEATURES_NEEDED)
}

pub fn version() -> Option<String> {
    let program = program()?;
    let mut command = paths::light_command(&program);
    command.arg("--version");
    let out = paths::run(command, Duration::from_secs(5)).ok()?;
    out.split_whitespace().last().map(str::to_owned)
}

/// `magnesium searchable --json`: for each named file, whether the tutor's
/// source search will read it, and why not when it will not. The paths are
/// from the folder's root. None when this version of the runtime has no such
/// command. The command works in a folder that is not set up yet, and
/// changes nothing.
pub fn searchable(folder: &Path, paths: &[String]) -> Option<Vec<Value>> {
    let mut args = vec!["searchable", "--json", "--"];
    args.extend(paths.iter().map(String::as_str));
    // Each PDF is read to see whether it has any text.
    let out = run(folder, &args, Duration::from_secs(90)).ok()?;
    let value: Value = serde_json::from_str(&out).ok()?;
    let files = value["files"].as_array().cloned()?;
    (files.len() == paths.len()).then_some(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Rows as `magnesium attempts --json` prints them: a wrong first try, a
    /// right one given a hint, a right cold one on a paper, and a decline.
    fn printed() -> String {
        json!({"attempts": [
            {"attempt_id": "a-1", "answered_at": "2026-11-12 09:41", "question": "What is b in x² − 5x + 6 = 0?",
             "paper": null, "paper_question": null, "mark": "wrong", "marks": null, "out_of": null, "help": "none",
             "confidence": null, "cold": true, "seconds": null, "label": "sign of b",
             "observation_id": "o-1", "response_event_id": "e-1", "session_id": "s-1", "kind": "reading coefficients", "no_start": false},
            {"attempt_id": "a-2", "answered_at": "2026-11-12 09:44", "question": "What is b in x² + 3x − 4 = 0?",
             "paper": null, "paper_question": null, "mark": "right", "marks": null, "out_of": null, "help": "hint",
             "confidence": "sure", "cold": false, "seconds": 41, "label": "sign of b",
             "observation_id": "o-2", "response_event_id": "e-2", "session_id": "s-1", "kind": null, "no_start": false},
            {"attempt_id": "a-3", "answered_at": "2026-11-12 10:02", "question": "Find Km",
             "paper": "Diagnostic", "paper_question": "3", "mark": "right", "marks": 4.0, "out_of": 4.0, "help": "none",
             "confidence": "unsure", "cold": true, "seconds": 300, "label": "reading Km from a plot",
             "observation_id": "o-3", "response_event_id": "e-3", "session_id": null, "kind": "recall", "no_start": true},
            {"attempt_id": "a-4", "answered_at": "2026-11-12 10:05", "question": "Units of Vmax",
             "paper": null, "paper_question": null, "mark": "declined", "marks": null, "out_of": null, "help": "none",
             "confidence": null, "cold": false, "seconds": null, "label": "units of a rate",
             "observation_id": "o-4", "response_event_id": "e-4", "session_id": "s-1", "kind": null, "no_start": false}
        ]})
        .to_string()
    }

    #[test]
    fn marks_are_read_from_what_the_runtime_prints() {
        let marks = marks_from(&printed()).expect("the rows");
        assert_eq!(marks.len(), 4);
        assert_eq!(
            marks[0],
            Mark {
                id: "a-1".into(),
                at: "2026-11-12 09:41".into(),
                label: "sign of b".into(),
                mark: "wrong".into(),
                help: "none".into(),
                cold: true,
                paper: None,
            }
        );
        assert_eq!(
            (
                marks[1].mark.as_str(),
                marks[1].help.as_str(),
                marks[1].cold
            ),
            ("right", "hint", false)
        );
        assert_eq!(marks[2].paper.as_deref(), Some("Diagnostic"));
        assert_eq!(marks[3].mark, "declined");
    }

    #[test]
    fn a_runtime_without_the_command_gives_none_and_not_an_error() {
        // What a runtime from before `attempts` prints, and what a program
        // that is not the runtime prints.
        assert_eq!(marks_from("error: unknown command: attempts\n"), None);
        assert_eq!(marks_from(""), None);
        assert_eq!(marks_from("{\"error\": \"no such command\"}"), None);
        // A folder with no answers yet is a list with nothing in it.
        assert_eq!(marks_from("{\"attempts\": []}"), Some(Vec::new()));
    }

    #[test]
    fn a_row_the_window_cannot_stand_behind_is_left_out() {
        let printed = json!({"attempts": [
            {"answered_at": "2026-11-12 09:41", "mark": "right", "help": "none", "cold": true, "label": "no id"},
            {"attempt_id": "a-9", "answered_at": "2026-11-12 09:41", "help": "none", "cold": true, "label": "no mark"},
            {"attempt_id": "a-10", "mark": "right", "label": "  "},
        ]})
        .to_string();
        let marks = marks_from(&printed).unwrap();
        assert_eq!(marks.len(), 1);
        // No help and no cold flag are read as the careful way: help given
        // is not assumed, but a cold answer is not assumed either.
        assert_eq!(
            (marks[0].id.as_str(), marks[0].help.as_str(), marks[0].cold),
            ("a-10", "none", false)
        );
        assert_eq!(marks[0].label, "");
    }
}
