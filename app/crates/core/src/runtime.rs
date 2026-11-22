use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::paths;

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

pub fn digest(folder: &Path) -> Result<Value, String> {
    let out = run(folder, &["digest"], Duration::from_secs(10))?;
    serde_json::from_str(&out).map_err(|e| format!("its digest was not readable ({e})"))
}

pub fn map_titles(folder: &Path, target: &str) -> std::collections::BTreeMap<String, String> {
    let Ok(out) = run(
        folder,
        &["map", "--target", target, "--json"],
        Duration::from_secs(10),
    ) else {
        return Default::default();
    };
    let Ok(value) = serde_json::from_str::<Value>(&out) else {
        return Default::default();
    };
    value["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            Some((
                item["id"].as_str()?.to_ascii_lowercase(),
                item["title"].as_str()?.to_owned(),
            ))
        })
        .collect()
}

pub fn attempts(folder: &Path) -> Option<Vec<Value>> {
    let out = run(folder, &["attempts", "--json"], Duration::from_secs(10)).ok()?;
    let value: Value = serde_json::from_str(&out).ok()?;
    value["attempts"].as_array().cloned()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    pub id: String,
    pub at: String,
    pub label: String,
    pub mark: String,
    pub help: String,
    pub cold: bool,
    pub paper: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pass: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_day: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<String>,
}

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

pub fn note_passes(marks: &mut [Mark], folder: &Path) {
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

pub fn marks(folder: &Path) -> Option<Vec<Mark>> {
    let out = run(folder, &["attempts", "--json"], Duration::from_secs(10)).ok()?;
    marks_from(&out)
}

pub fn progress(folder: &Path) -> Option<Value> {
    let out = run(folder, &["progress", "--json"], Duration::from_secs(10)).ok()?;
    let value: Value = serde_json::from_str(&out).ok()?;
    (value["active"].as_bool() == Some(true)
        && value["upgrade_needed"].as_bool() != Some(true)
        && value["days"].is_array())
    .then_some(value)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Closing {
    pub lines: Vec<String>,
    pub ended_ms: Option<i64>,
}

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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub step: String,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct OpenTask {
    pub kind: String,
    pub text: String,
    pub posed_at_ms: i64,
    #[serde(default)]
    pub whole_reply: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Lesson {
    pub goal: String,
    #[serde(default)]
    pub steps: Vec<Step>,
    pub open_task: Option<OpenTask>,
}

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

pub fn init(folder: &Path) -> Result<(), String> {
    run(folder, &["init", "--json"], Duration::from_secs(30)).map(|_| ())
}

pub fn upgrade(root: &Path) -> Result<(), String> {
    run(
        root,
        &["init", "--upgrade-instructions", "--json"],
        Duration::from_secs(180),
    )
    .map(|_| ())
}

pub const WINDOW_FEATURES_NEEDED: u64 = 4;

pub fn window_features(program: &Path) -> Option<u64> {
    let mut command = paths::light_command(program);
    command.current_dir("/").arg("digest");
    let out = paths::run(command, Duration::from_secs(10)).ok()?;
    let value: Value = serde_json::from_str(&out).ok()?;
    value["window_features"].as_u64()
}

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

pub fn searchable(folder: &Path, paths: &[String]) -> Option<Vec<Value>> {
    let mut args = vec!["searchable", "--json", "--"];
    args.extend(paths.iter().map(String::as_str));
    let out = run(folder, &args, Duration::from_secs(90)).ok()?;
    let value: Value = serde_json::from_str(&out).ok()?;
    let files = value["files"].as_array().cloned()?;
    (files.len() == paths.len()).then_some(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        assert!(receipts_from(&json!({"plans": []})).is_empty());
    }

    #[test]
    fn a_runtime_without_the_command_gives_none_and_not_an_error() {
        assert_eq!(marks_from("error: unknown command: attempts\n"), None);
        assert_eq!(marks_from(""), None);
        assert_eq!(marks_from("{\"error\": \"no such command\"}"), None);
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
        assert_eq!(
            (marks[0].id.as_str(), marks[0].help.as_str(), marks[0].cold),
            ("a-10", "none", false)
        );
        assert_eq!(marks[0].label, "");
    }
}
