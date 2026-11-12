//! Two facts the runtime stores and prints through no command yet.
//!
//! The window reads them straight from the runtime's files, read-only. This
//! ties the window to two table layouts, so every query here fails quietly to
//! "nothing known". Replace each reader with a command once the runtime has
//! one.
//!
//! Quick checks moved in the runtime of issue 30. Before it, each check and
//! its passes and due day were a row of `items` in `sittings.sqlite`. From it
//! on, the learner store keeps what happened to each check, and the runtime
//! works out the passes and the due day each time it is asked, so no stored
//! row says either. The window does not work them out itself, because that
//! rule belongs to the runtime. Against that runtime it says it cannot tell.

use std::path::Path;

use chrono::NaiveDate;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn open(path: &Path) -> Option<Connection> {
    if !path.is_file() {
        return None;
    }
    // Read-only, and never created: a look must not change the record.
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()
}

/// A quick check from chat: something the learner got wrong, to be reproduced
/// alone on later days.
#[derive(Clone, Debug)]
pub struct Check {
    pub label: String,
    pub passes: u8,
    pub closed: bool,
    pub due: Option<NaiveDate>,
    pub last_pass: Option<NaiveDate>,
    pub last_miss: Option<String>,
}

/// Whether the learner store keeps the quick checks, as the runtime of issue
/// 30 does. Its `practice_items` table is the sign.
fn checks_in_the_learner_store(root: &Path) -> bool {
    open(&root.join(".magnesium/state.sqlite")).is_some_and(|conn| {
        conn.query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='practice_items'",
            [],
            |_| Ok(()),
        )
        .is_ok()
    })
}

/// The quick checks, or None when the window cannot tell: the runtime keeps
/// them where only it can work out what is due, or their table is not as
/// expected. A folder with no checks yet gives an empty list.
pub fn checks(root: &Path) -> Option<Vec<Check>> {
    if checks_in_the_learner_store(root) {
        return None;
    }
    let Some(conn) = open(&root.join(".magnesium/sittings.sqlite")) else {
        return Some(Vec::new());
    };
    let mut statement = conn
        .prepare(
            "SELECT label, status, passes, due_day, last_pass_day, misses_json FROM items ORDER BY created_ms",
        )
        .ok()?;
    let day =
        |text: Option<String>| text.and_then(|t| NaiveDate::parse_from_str(&t, "%Y-%m-%d").ok());
    let rows = statement.query_map([], |row| {
        let misses: String = row.get(5).unwrap_or_default();
        let last_miss = serde_json::from_str::<Value>(&misses)
            .ok()
            .and_then(|list| list.as_array().and_then(|l| l.last().cloned()))
            .and_then(|miss| miss.as_str().map(str::to_owned));
        Ok(Check {
            label: row.get(0)?,
            closed: row.get::<_, String>(1)? == "closed",
            passes: row.get::<_, i64>(2)?.clamp(0, 3) as u8,
            due: day(row.get(3)?),
            last_pass: day(row.get(4)?),
            last_miss,
        })
    });
    let rows: Vec<Check> = rows.ok()?.filter_map(Result::ok).collect();
    Some(rows)
}

/// How many quick checks are due today or earlier, or None when the window
/// cannot tell.
pub fn checks_due(root: &Path, today: NaiveDate) -> Option<u32> {
    let checks = checks(root)?;
    Some(
        checks
            .iter()
            .filter(|check| !check.closed && check.due.is_some_and(|due| due <= today))
            .count() as u32,
    )
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub step: String,
    /// "todo", "doing" or "done".
    pub status: String,
}

/// The steps of the current goal, from the most recently saved checkpoint. An
/// older runtime stores none, and then the rail shows the goal alone.
pub fn goal_steps(root: &Path) -> Vec<Step> {
    let Some(conn) = open(&root.join(".magnesium/state.sqlite")) else {
        return Vec::new();
    };
    let checkpoint: Option<String> = conn
        .query_row(
            "SELECT checkpoint_json FROM learning_sessions ORDER BY updated_at_ms DESC, session_id LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok();
    let Some(value) = checkpoint.and_then(|text| serde_json::from_str::<Value>(&text).ok()) else {
        return Vec::new();
    };
    value["goal_steps"]
        .as_array()
        .map(|steps| {
            steps
                .iter()
                .filter_map(|step| {
                    Some(Step {
                        step: step["step"].as_str()?.trim().to_owned(),
                        status: step["status"].as_str().unwrap_or("todo").to_owned(),
                    })
                })
                .filter(|step| !step.step.is_empty())
                .take(12)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_store_gives_nothing_and_creates_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(checks(dir.path()).map(|c| c.len()), Some(0));
        assert!(goal_steps(dir.path()).is_empty());
        assert!(!dir.path().join(".magnesium").exists());
    }

    #[test]
    fn due_checks_are_counted_and_steps_are_read() {
        let dir = tempfile::tempdir().unwrap();
        let control = dir.path().join(".magnesium");
        std::fs::create_dir_all(&control).unwrap();
        let conn = Connection::open(control.join("sittings.sqlite")).unwrap();
        conn.execute_batch(
            "CREATE TABLE items(item_id TEXT PRIMARY KEY, label TEXT, label_key TEXT, status TEXT, passes INTEGER, due_day TEXT, last_pass_day TEXT, misses_json TEXT, created_ms INTEGER, updated_ms INTEGER);
             INSERT INTO items VALUES('a','sign of b','sign of b','open',1,'2026-11-10','2026-11-09','[\"dropped the minus\"]',1,1);
             INSERT INTO items VALUES('b','later','later','open',0,'2026-11-20',NULL,'[]',2,2);
             INSERT INTO items VALUES('c','done','done','closed',3,NULL,'2026-11-01','[]',3,3);",
        )
        .unwrap();
        drop(conn);
        let today = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
        assert_eq!(checks_due(dir.path(), today), Some(1));
        let all = checks(dir.path()).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].last_miss.as_deref(), Some("dropped the minus"));

        let conn = Connection::open(control.join("state.sqlite")).unwrap();
        conn.execute_batch(
            "CREATE TABLE learning_sessions(session_id TEXT, checkpoint_json TEXT, updated_at_ms INTEGER);
             INSERT INTO learning_sessions VALUES('s1','{\"goal_steps\":[{\"step\":\"Take reciprocals\",\"status\":\"done\"},{\"step\":\"Read the intercept\",\"status\":\"doing\"}]}',5);
             INSERT INTO learning_sessions VALUES('s0','{}',1);",
        )
        .unwrap();
        drop(conn);
        assert_eq!(
            goal_steps(dir.path()),
            vec![
                Step {
                    step: "Take reciprocals".into(),
                    status: "done".into()
                },
                Step {
                    step: "Read the intercept".into(),
                    status: "doing".into()
                },
            ]
        );
    }

    #[test]
    fn checks_kept_in_the_learner_store_are_not_known() {
        let dir = tempfile::tempdir().unwrap();
        let control = dir.path().join(".magnesium");
        std::fs::create_dir_all(&control).unwrap();
        Connection::open(control.join("state.sqlite"))
            .unwrap()
            .execute_batch("CREATE TABLE practice_items(item_id TEXT PRIMARY KEY, label TEXT);")
            .unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
        assert!(checks(dir.path()).is_none());
        assert_eq!(checks_due(dir.path(), today), None);
    }
}
