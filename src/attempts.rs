//! `magnesium attempts`: every answer the tutor marked, one row each.
//!
//! Read-only. The rows come from the learner store's `attempts` table, which
//! the runtime fills from the attempts on recorded observations, in chat and
//! on practice papers.
use crate::learner::store::LearnerStore;
use crate::workspace::{Workspace, reject_symlink_components};
use chrono::{Local, TimeZone};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::process::ExitCode;

/// Every attempt, oldest first.
pub(crate) fn list(conn: &Connection) -> rusqlite::Result<Vec<Value>> {
    let mut statement = conn.prepare(
        "SELECT attempt_id,answered_at_ms,question,paper,paper_question,mark,marks,out_of,
                help,confidence,cold,seconds,label,observation_id,response_event_id,session_id
         FROM attempts ORDER BY answered_at_ms,attempt_no",
    )?;
    statement
        .query_map([], |r| {
            Ok(json!({
                "attempt_id": r.get::<_, String>(0)?,
                "answered_at": local_time(r.get(1)?),
                "question": r.get::<_, String>(2)?,
                "paper": r.get::<_, Option<String>>(3)?,
                "paper_question": r.get::<_, Option<String>>(4)?,
                "mark": r.get::<_, String>(5)?,
                "marks": r.get::<_, Option<f64>>(6)?,
                "out_of": r.get::<_, Option<f64>>(7)?,
                "help": r.get::<_, String>(8)?,
                "confidence": r.get::<_, Option<String>>(9)?,
                "cold": r.get::<_, bool>(10)?,
                "seconds": r.get::<_, Option<i64>>(11)?,
                "label": r.get::<_, String>(12)?,
                "observation_id": r.get::<_, String>(13)?,
                "response_event_id": r.get::<_, String>(14)?,
                "session_id": r.get::<_, Option<String>>(15)?,
            }))
        })?
        .collect()
}

fn local_time(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|time| time.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

fn number(value: &Value) -> String {
    value
        .as_f64()
        .map(|n| {
            if n.fract() == 0.0 {
                format!("{n:.0}")
            } else {
                n.to_string()
            }
        })
        .unwrap_or_default()
}

/// One line per attempt, for a person reading the terminal.
fn render(attempts: &[Value]) -> String {
    if attempts.is_empty() {
        return "No answers recorded yet.\n".into();
    }
    let mut text = String::new();
    for a in attempts {
        let mark = match a["marks"].is_null() {
            true => a["mark"].as_str().unwrap_or("").to_owned(),
            false => format!(
                "{} {}/{}",
                a["mark"].as_str().unwrap_or(""),
                number(&a["marks"]),
                number(&a["out_of"])
            ),
        };
        let question = match a["paper"].as_str() {
            Some(paper) => format!(
                "{paper} Q{}: {}",
                a["paper_question"].as_str().unwrap_or(""),
                a["question"].as_str().unwrap_or("")
            ),
            None => a["question"].as_str().unwrap_or("").to_owned(),
        };
        let mut facts = vec![
            mark,
            format!("help {}", a["help"].as_str().unwrap_or("")),
            format!("confidence {}", a["confidence"].as_str().unwrap_or("none")),
            if a["cold"] == true {
                "cold"
            } else {
                "not cold"
            }
            .to_owned(),
        ];
        if let Some(seconds) = a["seconds"].as_i64() {
            facts.push(format!("{seconds} s"));
        }
        text.push_str(&format!(
            "{}  {}\n    {}. Tests: {}\n",
            a["answered_at"].as_str().unwrap_or(""),
            question.split_whitespace().collect::<Vec<_>>().join(" "),
            facts.join(", "),
            a["label"].as_str().unwrap_or("")
        ));
    }
    text
}

fn attempts_here() -> Result<Option<Vec<Value>>, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let Some(workspace) = Workspace::discover(&cwd).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let id = workspace.id().map_err(|e| e.to_string())?;
    reject_symlink_components(&workspace.store_path(), workspace.control())
        .map_err(|e| e.to_string())?;
    let store = LearnerStore::open(&workspace.store_path(), &id).map_err(|e| e.to_string())?;
    list(store.connection())
        .map(Some)
        .map_err(|e| e.to_string())
}

pub(crate) fn print_attempts(as_json: bool) -> ExitCode {
    match attempts_here() {
        Ok(Some(attempts)) => {
            if as_json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "attempts": attempts }))
                        .unwrap_or_default()
                );
            } else {
                print!("{}", render(&attempts));
            }
            ExitCode::SUCCESS
        }
        Ok(None) => {
            eprintln!("magnesium: this folder is not a Magnesium study folder");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("magnesium: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn each_attempt_reads_as_one_line_with_its_mark_help_and_confidence() {
        let text = super::render(&[
            json!({"answered_at":"2026-11-09 18:02","question":"Solve 2x² − 5x − 3 = 0","paper":null,"paper_question":null,"mark":"wrong","marks":null,"out_of":null,"help":"none","confidence":null,"cold":false,"seconds":null,"label":"sign of b in the quadratic formula"}),
            json!({"answered_at":"2026-11-10 09:15","question":"Find the mean of X","paper":"MockA","paper_question":"3b","mark":"partly","marks":2.0,"out_of":4.0,"help":"none","confidence":"sure","cold":true,"seconds":540,"label":"expected value"}),
        ]);
        assert_eq!(
            text,
            "2026-11-09 18:02  Solve 2x² − 5x − 3 = 0\n    wrong, help none, confidence none, not cold. Tests: sign of b in the quadratic formula\n\
             2026-11-10 09:15  MockA Q3b: Find the mean of X\n    partly 2/4, help none, confidence sure, cold, 540 s. Tests: expected value\n"
        );
        assert_eq!(super::render(&[]), "No answers recorded yet.\n");
    }
}
