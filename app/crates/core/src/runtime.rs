//! The Magnesium runtime, reached through its own commands.
//!
//! The window links none of the runtime's code. It runs the `magnesium`
//! program the learner installed and reads what it prints, so the runtime
//! stays small and the two can be updated apart.

use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
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

/// Run another of the runtime's read-only commands in a folder and return
/// what it printed, or the last line of its complaint.
pub fn run_in(folder: &Path, args: &[&str], limit: Duration) -> Result<String, String> {
    run(folder, args, limit)
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Which of its skill's passes this answer earned, 1 to 3, as
    /// `magnesium progress` counts them now. None when it earned none, when
    /// a later miss started the count again, or when the runtime cannot say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pass: Option<u8>,
    /// The first day its skill's next pass can count, as "2026-11-19", on
    /// the answer that earned the skill's latest pass, while the skill is
    /// not proven. On an earlier pass it would answer a question the
    /// learner is past, so only the latest carries it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_day: Option<String>,
    /// On an answer that earned a pass, the runtime's line of evidence from
    /// the record: the last miss on the skill before it and the days since
    /// the learner last answered it. A terminal shows the same line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<String>,
}

/// Each answer that earned a pass a skill holds now, by the record's id, and
/// which pass it was, from what `magnesium progress` printed. A runtime from
/// before it named these answers gives none.
pub fn passes_from(progress: &Value) -> HashMap<String, u8> {
    let plans = progress["plans"].as_array().into_iter().flatten();
    let skills = plans
        .flat_map(|plan| plan["skills"].as_array().into_iter().flatten())
        .chain(progress["other"]["skills"].as_array().into_iter().flatten());
    let mut passes = HashMap::new();
    for skill in skills {
        for (at, id) in skill["passed_by"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            if let (Some(id), Ok(pass)) = (id.as_str(), u8::try_from(at + 1)) {
                passes.insert(id.to_owned(), pass);
            }
        }
    }
    passes
}

/// The first day each skill's next pass can count, by the record's id for
/// the answer that earned the latest pass it holds, from what `magnesium
/// progress` printed. A proven skill, or one the runtime names no day for,
/// gives none.
pub fn next_days_from(progress: &Value) -> HashMap<String, String> {
    let plans = progress["plans"].as_array().into_iter().flatten();
    let skills = plans
        .flat_map(|plan| plan["skills"].as_array().into_iter().flatten())
        .chain(progress["other"]["skills"].as_array().into_iter().flatten());
    skills
        .filter(|skill| skill["proven"].as_bool() != Some(true))
        .filter_map(|skill| {
            let latest = skill["passed_by"].as_array()?.last()?.as_str()?;
            let day = skill["next_on"].as_str()?;
            Some((latest.to_owned(), day.to_owned()))
        })
        .collect()
}

/// The runtime's line of evidence for each answer that earned a pass the
/// skill holds now, by the record's id, from what `magnesium progress`
/// printed. A runtime from before it wrote them gives none.
pub fn receipts_from(progress: &Value) -> HashMap<String, String> {
    progress["pass_receipts"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(id, receipt)| {
            let text = receipt["text"].as_str()?.trim();
            (!text.is_empty()).then(|| (id.clone(), text.to_owned()))
        })
        .collect()
}

/// Say which pass each mark earned, as the runtime counts it now, and on the
/// latest pass of a skill, the day its next can count.
pub fn note_passes(marks: &mut [Mark], folder: &Path) {
    // Only a right answer given alone can earn one: the others need no reading.
    if !marks
        .iter()
        .any(|mark| mark.mark == "right" && mark.help == "none")
    {
        return;
    }
    let Some(progress) = progress(folder) else {
        return;
    };
    let passes = passes_from(&progress);
    let next_days = next_days_from(&progress);
    let receipts = receipts_from(&progress);
    for mark in marks {
        mark.pass = passes.get(&mark.id).copied();
        mark.next_day = next_days.get(&mark.id).cloned();
        mark.receipt = mark.pass.and_then(|_| receipts.get(&mark.id).cloned());
    }
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
                    pass: None,
                    next_day: None,
                    receipt: None,
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

/// What `magnesium closing --json` says: the closing lines for today, as
/// the runtime words them, and when the current sitting ended.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Closing {
    /// Empty when no answer was marked today.
    pub lines: Vec<String>,
    pub ended_ms: Option<i64>,
}

/// `magnesium closing --json`, read without changing anything. None when this
/// version of the runtime has no such command, or the folder is not set up.
pub fn closing(folder: &Path) -> Option<Closing> {
    let out = run(folder, &["closing", "--json"], Duration::from_secs(10)).ok()?;
    closing_from(&out)
}

pub fn closing_from(printed: &str) -> Option<Closing> {
    let value: Value = serde_json::from_str(printed).ok()?;
    if value["active"].as_bool() != Some(true) {
        return None;
    }
    let lines = value["closing"]["lines"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|line| line.as_str().map(str::to_owned))
        .collect();
    Some(Closing {
        lines,
        ended_ms: value["sitting"]["ended_ms"].as_i64(),
    })
}

/// A step of the lesson's goal, as the record's checkpoint holds it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub step: String,
    /// "todo", "doing" or "done".
    pub status: String,
}

/// The task open in a lesson, in the words of the tutor's message that
/// posed it, as the record holds it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct OpenTask {
    /// "task", or "quick_check" for a sitting's quick check.
    pub kind: String,
    pub text: String,
    pub posed_at_ms: i64,
    /// True when the record could not find the task's own words in the
    /// reply, so `text` is the whole reply.
    #[serde(default)]
    pub whole_reply: bool,
}

/// A lesson as `magnesium lesson --json` prints it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Lesson {
    pub goal: String,
    #[serde(default)]
    pub steps: Vec<Step>,
    pub open_task: Option<OpenTask>,
}

/// `magnesium lesson --json`: the lesson a conversation is in (or, with no
/// conversation, the folder's most recently changed one), read from the
/// record without changing it. None when the runtime could not say: it has no
/// such command, or the folder is not set up. Some(None) when it has no
/// lesson for that conversation.
pub fn lesson(folder: &Path, conversation: Option<&str>) -> Option<Option<Lesson>> {
    let mut args = vec!["lesson", "--json"];
    if let Some(conversation) = conversation {
        args.extend(["--conversation", conversation]);
    }
    let out = run(folder, &args, Duration::from_secs(10)).ok()?;
    lesson_from(&out)
}

pub fn lesson_from(printed: &str) -> Option<Option<Lesson>> {
    let value: Value = serde_json::from_str(printed).ok()?;
    let lesson = value.get("lesson")?;
    if lesson.is_null() {
        return Some(None);
    }
    serde_json::from_value(lesson.clone()).ok().map(Some)
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
/// repair, which a lesson on the direct engine records. 4 is the first with
/// `lesson --json`, where the window reads the goal's steps and, on the
/// plugin engine, the open task, and the first that refuses an attempt on a
/// message the window sent as talk.
pub const WINDOW_FEATURES_NEEDED: u64 = 4;

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
                pass: None,
                next_day: None,
                receipt: None,
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
    fn an_answer_is_told_which_pass_it_earned_from_what_the_runtime_prints() {
        let printed = json!({
            "plans": [{"plan": "midterm-1", "from": "record", "skills": [
                {"skill": "reading Km", "passes": 2, "passed_by": [null, "a-3"]}
            ]}, {"plan": "old", "from": "file"}],
            "other": {"skills": [
                {"skill": "units", "passes": 1, "passed_by": ["a-7"]},
                // A runtime from before it named them.
                {"skill": "sign of b", "passes": 1}
            ]}
        });
        let passes = passes_from(&printed);
        assert_eq!(passes.get("a-3"), Some(&2));
        assert_eq!(passes.get("a-7"), Some(&1));
        assert_eq!(passes.len(), 2);
    }

    #[test]
    fn the_latest_pass_of_a_skill_not_proven_carries_its_next_day() {
        let printed = json!({
            "plans": [{"plan": "midterm-1", "from": "record", "skills": [
                {"skill": "reading Km", "passes": 2, "proven": false, "passed_by": ["a-1", "a-3"], "next_on": "2026-11-19"},
                {"skill": "rate", "passes": 3, "proven": true, "passed_by": ["a-4", "a-5", "a-6"], "next_on": null}
            ]}],
            "other": {"skills": [
                // A pass brought in from a gap log has no answer to carry it.
                {"skill": "units", "passes": 1, "proven": false, "passed_by": [null], "next_on": "2026-11-17"},
                {"skill": "sign of b", "passes": 0, "proven": false, "passed_by": [], "next_on": "2026-11-17"}
            ]}
        });
        let days = next_days_from(&printed);
        assert_eq!(days.get("a-3").map(String::as_str), Some("2026-11-19"));
        assert_eq!(days.len(), 1, "{days:?}");
    }

    #[test]
    fn an_answer_that_earned_a_pass_carries_the_runtimes_receipt() {
        let printed = json!({
            "pass_receipts": {
                "a-3": {"text": "Not right on Tue 10 Nov: sign of b. It held through the 4 days since you last answered it."},
                "a-4": {"text": "  "}
            }
        });
        let receipts = receipts_from(&printed);
        assert!(receipts["a-3"].starts_with("Not right on Tue 10 Nov"));
        assert_eq!(receipts.len(), 1);
        // A runtime from before it wrote them.
        assert!(receipts_from(&json!({"plans": []})).is_empty());
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
