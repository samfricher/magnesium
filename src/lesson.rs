use std::process::ExitCode;

use chrono::{Local, TimeZone};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use crate::adapter::host::HostIdentity;
use crate::learner::LearnerStore;
use crate::workspace::{Workspace, reject_symlink_components};

fn lesson(workspace: &Workspace, conversation: Option<&str>) -> Result<Option<Value>, String> {
    let id = workspace.id().map_err(|e| e.to_string())?;
    reject_symlink_components(&workspace.store_path(), workspace.control())
        .map_err(|e| e.to_string())?;
    if crate::learner::store::needs_upgrade(&workspace.store_path()).map_err(|e| e.to_string())? {
        return Err("this folder's record is from an earlier version: run magnesium init --upgrade-instructions first".into());
    }
    let store = LearnerStore::open(&workspace.store_path(), &id).map_err(|e| e.to_string())?;
    let snapshot = store
        .conn
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    let thread = conversation
        .map(|native| {
            HostIdentity::claude(native)
                .map(|host| host.stored_session_id())
                .map_err(|e| format!("not a conversation id: {e}"))
        })
        .transpose()?;
    let open_check = crate::sittings::Sittings::open_existing(workspace)?
        .map(|sittings| sittings.open_check())
        .transpose()?
        .flatten();
    let value = read(&snapshot, thread.as_deref(), open_check).map_err(|e| e.to_string())?;
    drop(snapshot);
    Ok(value)
}

fn read(
    conn: &Connection,
    thread: Option<&str>,
    open_check: Option<(i64, String)>,
) -> rusqlite::Result<Option<Value>> {
    let session = match thread {
        Some(thread) => conn
            .query_row(
                "SELECT session_id FROM host_bindings WHERE host_kind='claude' AND host_thread_id=?1 AND session_id IS NOT NULL",
                [thread],
                |r| r.get::<_, String>(0),
            )
            .optional()?,
        None => conn
            .query_row(
                "SELECT session_id FROM learning_sessions ORDER BY updated_at_ms DESC, session_id LIMIT 1",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()?,
    };
    let Some(session) = session else {
        return Ok(None);
    };
    let (goal, status, checkpoint, updated): (String, String, String, i64) = conn.query_row(
        "SELECT goal,status,checkpoint_json,updated_at_ms FROM learning_sessions WHERE session_id=?1",
        [&session],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let checkpoint: Value = serde_json::from_str(&checkpoint).unwrap_or(Value::Null);
    let threads: Vec<(String, bool, bool)> = {
        let mut statement = conn.prepare(
            "SELECT host_thread_id,learning_active,recording_enabled FROM host_bindings WHERE session_id=?1 AND (?2 IS NULL OR host_thread_id=?2) ORDER BY host_thread_id",
        )?;
        statement
            .query_map(params![session, thread], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect::<rusqlite::Result<_>>()?
    };
    let learning = threads.iter().any(|(_, on, _)| *on);
    let recording = !threads.is_empty() && threads.iter().all(|(_, _, on)| *on);
    let steps: Vec<Value> = checkpoint["goal_steps"]
        .as_array()
        .map(|steps| {
            steps
                .iter()
                .filter_map(|step| {
                    let words = step["step"].as_str()?.trim();
                    (!words.is_empty()).then(|| {
                        json!({"step": words, "status": step["status"].as_str().unwrap_or("todo")})
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let task = pending_task(conn, &session, &checkpoint)?;
    let check = match open_check {
        Some((number, label)) => {
            posed_check(conn, &threads, number)?.map(|posed| (number, label, posed))
        }
        None => None,
    };
    let open_task = match (task, check) {
        (Some(task), Some((_, _, check))) if task.at_ms >= check.at_ms => {
            Some(task.value("task", None))
        }
        (_, Some((number, label, check))) => Some(check.value(
            "quick_check",
            Some(json!({"number": number, "label": label})),
        )),
        (Some(task), None) => Some(task.value("task", None)),
        (None, None) => None,
    };
    Ok(Some(json!({
        "session_id": session,
        "goal": goal,
        "status": status,
        "learning_on": learning,
        "recording_on": recording,
        "updated_at": local_time(updated),
        "steps": steps,
        "open_task": open_task,
    })))
}

struct Posed {
    text: String,
    at_ms: i64,
    whole_reply: bool,
}

impl Posed {
    fn value(self, kind: &str, check: Option<Value>) -> Value {
        let mut value = json!({
            "kind": kind,
            "text": self.text,
            "posed_at": local_time(self.at_ms),
            "posed_at_ms": self.at_ms,
            "whole_reply": self.whole_reply,
        });
        if let (Some(object), Some(Value::Object(check))) = (value.as_object_mut(), check) {
            object.extend(check);
        }
        value
    }
}

fn pending_task(
    conn: &Connection,
    session: &str,
    checkpoint: &Value,
) -> rusqlite::Result<Option<Posed>> {
    let Some(proposal) = checkpoint["pending_task"]["proposal_event_ref"].as_str() else {
        return Ok(None);
    };
    let Some((text, at_ms)) = conn
        .query_row(
            "SELECT text,COALESCE(occurred_at_ms,captured_at_ms) FROM learning_events WHERE event_id=?1",
            [proposal],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
        )
        .optional()?
    else {
        return Ok(None);
    };
    let answered: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM attempts WHERE session_id=?1 AND answered_at_ms>=?2)",
        params![session, at_ms],
        |r| r.get(0),
    )?;
    let words = checkpoint["pending_task"]["conditions"]
        .as_str()
        .and_then(|words| words_in(&text, words));
    Ok((!answered).then_some(match words {
        Some(words) => Posed {
            text: words,
            at_ms,
            whole_reply: false,
        },
        None => Posed {
            text,
            at_ms,
            whole_reply: true,
        },
    }))
}

fn words_in(reply: &str, words: &str) -> Option<String> {
    let words = words.replace("\r\n", "\n");
    let words = words.trim();
    (!words.is_empty() && reply.replace("\r\n", "\n").contains(words)).then(|| words.to_owned())
}

fn check_words(reply: &str, number: i64) -> Option<&str> {
    let needle = format!("quick check {number}");
    let bytes = reply.as_bytes();
    let start = reply.char_indices().map(|(at, _)| at).find(|&at| {
        bytes
            .get(at..at + needle.len())
            .is_some_and(|here| here.eq_ignore_ascii_case(needle.as_bytes()))
            && !bytes.get(at + needle.len()).is_some_and(u8::is_ascii_digit)
    })?;
    let start = reply[..start].trim_end_matches(['*', '_']).len();
    Some(reply[start..].trim_end())
}

fn posed_check(
    conn: &Connection,
    threads: &[(String, bool, bool)],
    number: i64,
) -> rusqlite::Result<Option<Posed>> {
    let mut statement = conn.prepare(
        "SELECT text,COALESCE(occurred_at_ms,captured_at_ms) FROM learning_events WHERE host_kind='claude' AND host_thread_id=?1 AND speaker='tutor' ORDER BY event_no DESC LIMIT 50",
    )?;
    let mut found: Option<Posed> = None;
    for (thread, ..) in threads {
        let rows = statement.query_map([thread], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (text, at_ms) = row?;
            if crate::sittings::says_check(&text.to_lowercase(), number) {
                if found.as_ref().is_none_or(|f| at_ms > f.at_ms) {
                    found = Some(match check_words(&text, number) {
                        Some(words) => Posed {
                            text: words.to_owned(),
                            at_ms,
                            whole_reply: false,
                        },
                        None => Posed {
                            text,
                            at_ms,
                            whole_reply: true,
                        },
                    });
                }
                break;
            }
        }
    }
    Ok(found)
}

fn local_time(ms: i64) -> Option<String> {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|time| time.format("%Y-%m-%d %H:%M").to_string())
}

fn render(lesson: Option<&Value>) -> String {
    let Some(lesson) = lesson else {
        return "No lesson yet.\n".into();
    };
    let mut text = format!(
        "{}\n    {}, learning {}, recording {}. Last changed {}.\n",
        lesson["goal"].as_str().unwrap_or(""),
        lesson["status"].as_str().unwrap_or(""),
        if lesson["learning_on"] == true {
            "on"
        } else {
            "off"
        },
        if lesson["recording_on"] == true {
            "on"
        } else {
            "off"
        },
        lesson["updated_at"].as_str().unwrap_or("unknown"),
    );
    for step in lesson["steps"].as_array().into_iter().flatten() {
        let mark = match step["status"].as_str() {
            Some("done") => "x",
            Some("doing") => ">",
            _ => " ",
        };
        text.push_str(&format!(
            "  [{mark}] {}\n",
            step["step"].as_str().unwrap_or("")
        ));
    }
    match lesson["open_task"].as_object() {
        Some(task) => text.push_str(&format!(
            "Open {}, posed {}:\n{}\n",
            match task["kind"].as_str() {
                Some("quick_check") => "quick check",
                _ => "task",
            },
            task["posed_at"].as_str().unwrap_or("at an unknown time"),
            task["text"].as_str().unwrap_or("").trim()
        )),
        None => text.push_str("No task open.\n"),
    }
    text
}

pub(crate) fn print_lesson(conversation: Option<&str>, as_json: bool) -> ExitCode {
    let found = std::env::current_dir()
        .map_err(|e| e.to_string())
        .and_then(|cwd| Workspace::discover(&cwd).map_err(|e| e.to_string()));
    let workspace = match found {
        Ok(Some(workspace)) => workspace,
        Ok(None) => {
            eprintln!("magnesium: this folder is not a Magnesium study folder");
            return ExitCode::FAILURE;
        }
        Err(error) => {
            eprintln!("magnesium: {error}");
            return ExitCode::FAILURE;
        }
    };
    match lesson(&workspace, conversation) {
        Ok(value) => {
            if as_json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "lesson": value })).unwrap_or_default()
                );
            } else {
                print!("{}", render(value.as_ref()));
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("magnesium: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{check_words, words_in};

    #[test]
    fn only_words_the_reply_holds_exactly_are_the_task() {
        let reply = "Correct: 3 × 4 = 12.\r\n\r\nNow a step up. Factorise x² − 8x + 15.";
        assert_eq!(
            words_in(reply, " Factorise x² − 8x + 15.\n").as_deref(),
            Some("Factorise x² − 8x + 15.")
        );
        assert_eq!(
            words_in(reply, "Correct: 3 × 4 = 12.\n\nNow a step up.").as_deref(),
            Some("Correct: 3 × 4 = 12.\n\nNow a step up.")
        );
        assert_eq!(words_in(reply, "Factorise x^2 - 8x + 15."), None);
        assert_eq!(words_in(reply, "No help"), None);
        assert_eq!(words_in(reply, "  "), None);
    }

    #[test]
    fn a_quick_check_is_its_reply_from_its_own_words() {
        assert_eq!(
            check_words(
                "Welcome back.\n\n**Quick check 2:** What is b in x² − 5x?",
                2
            ),
            Some("**Quick check 2:** What is b in x² − 5x?")
        );
        assert_eq!(check_words("Quick check 12: hi", 1), None);
        assert_eq!(
            check_words("İ quick CHECK 1: hi ", 1),
            Some("quick CHECK 1: hi")
        );
        assert_eq!(check_words("no check here", 1), None);
    }
}
