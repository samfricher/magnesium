use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{Local, NaiveDate, TimeZone};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde_json::{Value, json};

use crate::practice::{DueRedos, GapLogReport, Item};
use crate::workspace::{OpenerMode, Workspace};

pub(crate) const STORE_FILE: &str = "sittings.sqlite";
pub(crate) const LOG_FILE: &str = "sittings.log";
const STORE_VERSION: i64 = 2;
pub(crate) const SITTING_GAP_MS: i64 = 3 * 60 * 60 * 1000;
pub(crate) const MAX_CHECKS_PER_SITTING: usize = 3;
const MAX_MISS_CHARS: usize = 240;
const MAX_QUESTION_CHARS: usize = 400;
const MAX_LOGGED_SITTINGS: i64 = 1000;
const MAX_OPENER_BYTES: usize = 4096;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta(
  singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
  version INTEGER NOT NULL,
  last_learner_ms INTEGER
);
CREATE TABLE IF NOT EXISTS sittings(
  sitting_no INTEGER PRIMARY KEY,
  sitting_id TEXT NOT NULL UNIQUE,
  started_ms INTEGER NOT NULL,
  day TEXT NOT NULL,
  opener_delivered INTEGER NOT NULL DEFAULT 0,
  redos_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(redos_json)),
  not_taken INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS sitting_items(
  sitting_id TEXT NOT NULL REFERENCES sittings(sitting_id),
  item_id TEXT NOT NULL,
  number INTEGER NOT NULL,
  label TEXT NOT NULL,
  posed INTEGER NOT NULL DEFAULT 0,
  outcome TEXT CHECK (outcome IN ('passed','failed','helped','declined','answered')),
  PRIMARY KEY (sitting_id, item_id)
);
CREATE TABLE IF NOT EXISTS sitting_ends(
  sitting_id TEXT PRIMARY KEY REFERENCES sittings(sitting_id),
  ended_ms INTEGER NOT NULL,
  ended_by TEXT,
  shown_ms INTEGER
);
CREATE TABLE IF NOT EXISTS checks_alone(
  sitting_id TEXT PRIMARY KEY REFERENCES sittings(sitting_id),
  asked_ms INTEGER NOT NULL,
  asked_by TEXT,
  shown_ms INTEGER
);
";

const FROM_VERSION_1: &str = "
CREATE TABLE sitting_items_v2(
  sitting_id TEXT NOT NULL REFERENCES sittings(sitting_id),
  item_id TEXT NOT NULL,
  number INTEGER NOT NULL,
  label TEXT NOT NULL,
  posed INTEGER NOT NULL DEFAULT 0,
  outcome TEXT CHECK (outcome IN ('passed','failed','helped','declined','answered')),
  PRIMARY KEY (sitting_id, item_id)
);
INSERT INTO sitting_items_v2 SELECT sitting_id,item_id,number,label,posed,outcome FROM sitting_items;
DROP TABLE sitting_items;
ALTER TABLE sitting_items_v2 RENAME TO sitting_items;
DROP TABLE IF EXISTS attempts;
DROP TABLE IF EXISTS items;
UPDATE meta SET version = 2;
";

pub(crate) struct Sittings {
    conn: Connection,
    log: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Counts {
    pub(crate) due: i64,
    pub(crate) posed: i64,
}

pub(crate) const CHECKS_ENV: &str = "MAGNESIUM_SITTING_CHECKS";

pub(crate) fn checks_wanted() -> bool {
    std::env::var(CHECKS_ENV).is_ok_and(|value| value == "1")
}

#[derive(Clone, Debug)]
pub(crate) struct Opener {
    pub(crate) text: String,
    pub(crate) checks: Value,
    pub(crate) redos: Value,
}

impl Opener {
    fn new(checks: &[Check], items: &[Item], today: NaiveDate) -> Option<Self> {
        let redos = crate::practice::due_redos(items, today);
        (!checks.is_empty() || !redos.is_empty()).then(|| Self {
            text: opener_text(checks, &redos),
            checks: checks_json(checks),
            redos: crate::practice::due_rows_json(items, today),
        })
    }
}

fn checks_json(checks: &[Check]) -> Value {
    Value::Array(
        checks
            .iter()
            .map(|check| {
                json!({
                    "number": check.number,
                    "label": check.label,
                    "misses": check.misses,
                    "question": check.question,
                    "key": check.key,
                })
            })
            .collect(),
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SittingNow {
    pub(crate) id: String,
    pub(crate) started_ms: i64,
    pub(crate) ended_ms: Option<i64>,
    pub(crate) ended_by: Option<String>,
    pub(crate) shown_ms: Option<i64>,
}

pub(crate) fn open_check_item_read_only(workspace: &Workspace) -> Option<String> {
    let path = workspace.control().join(STORE_FILE);
    if !path.try_exists().ok()? {
        return None;
    }
    crate::workspace::reject_symlink_components(&path, workspace.control()).ok()?;
    let conn =
        Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    conn.busy_timeout(Duration::from_secs(2)).ok()?;
    conn.query_row(
        "SELECT i.item_id FROM sitting_items i
         WHERE i.sitting_id = (SELECT sitting_id FROM sittings ORDER BY sitting_no DESC LIMIT 1)
           AND i.posed=1 AND i.outcome IS NULL ORDER BY i.number LIMIT 1",
        [],
        |r| r.get(0),
    )
    .optional()
    .ok()
    .flatten()
}

pub(crate) fn now_state_read_only(workspace: &Workspace) -> Result<Option<SittingNow>, String> {
    let path = workspace.control().join(STORE_FILE);
    if !path.try_exists().map_err(error)? {
        return Ok(None);
    }
    crate::workspace::reject_symlink_components(&path, workspace.control()).map_err(error)?;
    let conn = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(error)?;
    conn.busy_timeout(Duration::from_secs(2)).map_err(error)?;
    let has = |table: &str| -> Result<bool, String> {
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [table],
            |r| r.get(0),
        )
        .map_err(error)
    };
    if !has("sittings")? {
        return Ok(None);
    }
    let query = if has("sitting_ends")? {
        "SELECT s.sitting_id, s.started_ms, e.ended_ms, e.ended_by, e.shown_ms
         FROM sittings s LEFT JOIN sitting_ends e ON e.sitting_id = s.sitting_id
         ORDER BY s.sitting_no DESC LIMIT 1"
    } else {
        "SELECT sitting_id, started_ms, NULL, NULL, NULL FROM sittings ORDER BY sitting_no DESC LIMIT 1"
    };
    conn.query_row(query, [], |r| {
        Ok(SittingNow {
            id: r.get(0)?,
            started_ms: r.get(1)?,
            ended_ms: r.get(2)?,
            ended_by: r.get(3)?,
            shown_ms: r.get(4)?,
        })
    })
    .optional()
    .map_err(error)
}

#[derive(Clone, Debug)]
struct Check {
    number: i64,
    label: String,
    misses: Vec<String>,
    question: Option<String>,
    key: Option<String>,
}

pub(crate) fn now_ms() -> i64 {
    crate::learner::store::now_ms()
}

pub(crate) fn local_day(ms: i64) -> NaiveDate {
    crate::learner::practice::local_day(ms)
}

fn clip(text: &str, chars: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match text.char_indices().nth(chars) {
        Some((end, _)) => format!("{}...", &text[..end]),
        None => text,
    }
}

fn error(error: impl std::fmt::Display) -> String {
    format!("sittings: {error}")
}

fn items_now(workspace: &Workspace) -> Vec<Item> {
    crate::practice::open_store(workspace)
        .and_then(|store| crate::practice::snapshot(&store.conn, workspace.root()))
        .unwrap_or_default()
}

pub(crate) fn outcome(effect: &str) -> Option<&'static str> {
    match effect {
        "passed" | "closed" => Some("passed"),
        "failed" | "restarted" => Some("failed"),
        "helped" => Some("helped"),
        "declined" => Some("declined"),
        "no_count" => Some("answered"),
        _ => None,
    }
}

impl Sittings {
    pub(crate) fn open(workspace: &Workspace) -> Result<Self, String> {
        Self::open_with(workspace, true)?.ok_or_else(|| error("store missing"))
    }

    pub(crate) fn open_existing(workspace: &Workspace) -> Result<Option<Self>, String> {
        Self::open_with(workspace, false)
    }

    fn open_with(workspace: &Workspace, create: bool) -> Result<Option<Self>, String> {
        let path = workspace.control().join(STORE_FILE);
        for candidate in ["", "-wal", "-shm", "-journal"] {
            let candidate = PathBuf::from(format!("{}{candidate}", path.display()));
            if std::fs::symlink_metadata(&candidate).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(error("a symlinked sittings store is not allowed"));
            }
        }
        if create {
            create_private(&path)?;
        } else if !path.try_exists().map_err(error)? {
            return Ok(None);
        }
        let mut conn =
            Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)
                .map_err(error)?;
        conn.busy_timeout(Duration::from_secs(2)).map_err(error)?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(error)?;
        crate::workspace::apply_sync_policy(&conn).map_err(error)?;
        conn.execute_batch(SCHEMA).map_err(error)?;
        conn.execute(
            "INSERT OR IGNORE INTO meta(singleton,version) VALUES(1,?1)",
            [STORE_VERSION],
        )
        .map_err(error)?;
        let version: i64 = conn
            .query_row("SELECT version FROM meta", [], |r| r.get(0))
            .map_err(error)?;
        if version == 1 {
            from_version_1(&mut conn, workspace)?;
        } else if version != STORE_VERSION {
            return Err(error(format!(
                "store version {version} is not {STORE_VERSION}"
            )));
        }
        Ok(Some(Self {
            conn,
            log: workspace.control().join(LOG_FILE),
        }))
    }

    fn write(&mut self) -> Result<Transaction<'_>, String> {
        self.conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(error)
    }

    pub(crate) fn note_learner_message(
        &mut self,
        workspace: &Workspace,
        now: i64,
    ) -> Result<bool, String> {
        let last: Option<i64> = self
            .conn
            .query_row("SELECT last_learner_ms FROM meta", [], |r| r.get(0))
            .map_err(error)?;
        let begins = last.is_none_or(|last| now - last >= SITTING_GAP_MS);
        let items = if begins {
            items_now(workspace)
        } else {
            Vec::new()
        };
        let due = crate::practice::due_checks(&items, local_day(now))
            .into_iter()
            .map(|item| (item.facts.item_id.clone(), item.facts.label.clone()))
            .collect::<Vec<_>>();
        let tx = self.write()?;
        let last: Option<i64> = tx
            .query_row("SELECT last_learner_ms FROM meta", [], |r| r.get(0))
            .map_err(error)?;
        tx.execute("UPDATE meta SET last_learner_ms=?1", [now])
            .map_err(error)?;
        let begins = last.is_none_or(|last| now - last >= SITTING_GAP_MS);
        if begins {
            begin(&tx, now, &due)?;
        }
        tx.commit().map_err(error)?;
        if begins {
            self.write_log();
        }
        Ok(begins)
    }

    fn current(&self) -> Result<Option<(String, bool)>, String> {
        self.conn
            .query_row(
                "SELECT sitting_id,opener_delivered FROM sittings ORDER BY sitting_no DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(error)
    }

    pub(crate) fn open_check(&self) -> Result<Option<(i64, String)>, String> {
        let Some((sitting, _)) = self.current()? else {
            return Ok(None);
        };
        self.conn
            .query_row(
                "SELECT number,label FROM sitting_items WHERE sitting_id=?1 AND posed=1 AND outcome IS NULL ORDER BY number LIMIT 1",
                [&sitting],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(error)
    }

    pub(crate) fn open_counts(&self) -> Result<Option<Counts>, String> {
        let Some((sitting, _)) = self.current()? else {
            return Ok(None);
        };
        let (due, posed): (i64, i64) = self
            .conn
            .query_row(
                "SELECT count(*), coalesce(sum(posed=1 OR outcome IS NOT NULL),0) FROM sitting_items WHERE sitting_id=?1",
                [&sitting],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(error)?;
        Ok((due > posed).then_some(Counts { due, posed }))
    }

    pub(crate) fn take_opener(
        &mut self,
        workspace: &Workspace,
        now: i64,
        report: &mut GapLogReport,
    ) -> Result<Option<Opener>, String> {
        let Some((sitting, delivered)) = self.current()? else {
            return Ok(None);
        };
        if delivered {
            return Ok(None);
        }
        let today = local_day(now);
        *report = crate::practice::sync_gap_logs(workspace, today);
        let items = items_now(workspace);
        let redos = crate::practice::due_redos(&items, today);
        let checks = self.checks(&sitting, &items)?;
        let tx = self.write()?;
        let claimed = tx
            .execute(
                "UPDATE sittings SET opener_delivered=1, redos_json=?2 WHERE sitting_id=?1 AND opener_delivered=0",
                params![
                    sitting,
                    json!(redos.iter().map(|r| json!({"gap_log": r.gap_log, "due": r.due, "first": r.first})).collect::<Vec<_>>()).to_string()
                ],
            )
            .map_err(error)?;
        tx.commit().map_err(error)?;
        let opener = Opener::new(&checks, &items, today).filter(|_| claimed > 0);
        if opener.is_some() {
            self.write_log();
        }
        Ok(opener)
    }

    pub(crate) fn opener_now(
        &self,
        workspace: &Workspace,
        now: i64,
    ) -> Result<Option<Opener>, String> {
        let Some((sitting, _)) = self.current()? else {
            return Ok(None);
        };
        let items = items_now(workspace);
        let checks = self.checks(&sitting, &items)?;
        Ok(Opener::new(&checks, &items, local_day(now)))
    }

    fn checks(&self, sitting: &str, items: &[Item]) -> Result<Vec<Check>, String> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT number, label, item_id FROM sitting_items WHERE sitting_id=?1 ORDER BY number",
            )
            .map_err(error)?;
        let taken = statement
            .query_map([sitting], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(error)?;
        Ok(taken
            .into_iter()
            .map(|(number, label, item_id)| {
                let item = items.iter().find(|item| item.facts.item_id == item_id);
                Check {
                    number,
                    label,
                    misses: item.map(|i| i.state.misses.clone()).unwrap_or_default(),
                    question: item.and_then(|i| i.facts.next_question.clone()),
                    key: item.and_then(|i| i.facts.answer_key.clone()),
                }
            })
            .collect())
    }

    pub(crate) fn end(&mut self, by: Option<&str>, now: i64) -> Result<Option<String>, String> {
        let Some((sitting, _)) = self.current()? else {
            return Ok(None);
        };
        let tx = self.write()?;
        tx.execute(
            "INSERT INTO sitting_ends(sitting_id,ended_ms,ended_by) VALUES(?1,?2,?3)
             ON CONFLICT(sitting_id) DO UPDATE SET ended_ms=excluded.ended_ms, ended_by=excluded.ended_by",
            params![sitting, now, by],
        )
        .map_err(error)?;
        tx.execute("DELETE FROM checks_alone WHERE sitting_id=?1", [&sitting])
            .map_err(error)?;
        tx.commit().map_err(error)?;
        Ok(Some(sitting))
    }

    pub(crate) fn now_state(&self) -> Result<Option<SittingNow>, String> {
        self.conn
            .query_row(
                "SELECT s.sitting_id, s.started_ms, e.ended_ms, e.ended_by, e.shown_ms
                 FROM sittings s LEFT JOIN sitting_ends e ON e.sitting_id = s.sitting_id
                 ORDER BY s.sitting_no DESC LIMIT 1",
                [],
                |r| {
                    Ok(SittingNow {
                        id: r.get(0)?,
                        started_ms: r.get(1)?,
                        ended_ms: r.get(2)?,
                        ended_by: r.get(3)?,
                        shown_ms: r.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(error)
    }

    pub(crate) fn reopen(&mut self) -> Result<(), String> {
        let Some((sitting, _)) = self.current()? else {
            return Ok(());
        };
        let tx = self.write()?;
        tx.execute("DELETE FROM sitting_ends WHERE sitting_id=?1", [&sitting])
            .map_err(error)?;
        tx.commit().map_err(error)
    }

    pub(crate) fn claim_end_shown(&mut self, sitting: &str, now: i64) -> Result<bool, String> {
        let tx = self.write()?;
        let changed = tx
            .execute(
                "UPDATE sitting_ends SET shown_ms=?2 WHERE sitting_id=?1 AND (shown_ms IS NULL OR shown_ms < ended_ms)",
                params![sitting, now],
            )
            .map_err(error)?;
        tx.commit().map_err(error)?;
        Ok(changed > 0)
    }

    pub(crate) fn note_reply(&mut self, reply: &str) -> Result<(), String> {
        let Some((sitting, _)) = self.current()? else {
            return Ok(());
        };
        let reply = reply.to_lowercase();
        let mut statement = self
            .conn
            .prepare("SELECT number FROM sitting_items WHERE sitting_id=?1")
            .map_err(error)?;
        let posed = statement
            .query_map([&sitting], |r| r.get::<_, i64>(0))
            .map_err(error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(error)?
            .into_iter()
            .filter(|number| says_check(&reply, *number))
            .collect::<Vec<_>>();
        drop(statement);
        if posed.is_empty() {
            return Ok(());
        }
        let tx = self.write()?;
        for number in posed {
            tx.execute(
                "UPDATE sitting_items SET posed=1 WHERE sitting_id=?1 AND number=?2",
                params![sitting, number],
            )
            .map_err(error)?;
        }
        tx.commit().map_err(error)?;
        self.write_log();
        Ok(())
    }

    pub(crate) fn settle(&mut self, outcomes: &[(String, &'static str)]) -> Result<(), String> {
        if outcomes.is_empty() {
            return Ok(());
        }
        let Some((sitting, _)) = self.current()? else {
            return Ok(());
        };
        let tx = self.write()?;
        let mut changed = 0;
        for (item, outcome) in outcomes {
            changed += tx
                .execute(
                    "UPDATE sitting_items SET outcome=?3, posed=1 WHERE sitting_id=?1 AND item_id=?2 AND outcome IS NULL",
                    params![sitting, item, outcome],
                )
                .map_err(error)?;
        }
        tx.commit().map_err(error)?;
        if changed > 0 {
            self.write_log();
        }
        Ok(())
    }

    fn write_log(&self) {
        if let Ok(lines) = self.log_lines() {
            let _ = crate::workspace::write_private_atomic(&self.log, lines.as_bytes());
        }
    }

    fn log_lines(&self) -> Result<String, String> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT sitting_id,started_ms,redos_json,not_taken FROM (SELECT * FROM sittings ORDER BY sitting_no DESC LIMIT ?1) ORDER BY sitting_no",
            )
            .map_err(error)?;
        let sittings = statement
            .query_map([MAX_LOGGED_SITTINGS], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })
            .map_err(error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(error)?;
        let mut items = self
            .conn
            .prepare(
                "SELECT label,posed,outcome FROM sitting_items WHERE sitting_id=?1 ORDER BY number",
            )
            .map_err(error)?;
        let mut out = String::new();
        for (sitting, started, redos, not_taken) in sittings {
            let rows = items
                .query_map([&sitting], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, bool>(1)?,
                        r.get::<_, Option<String>>(2)?,
                    ))
                })
                .map_err(error)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(error)?;
            let with = |keep: &dyn Fn(bool, Option<&str>) -> bool| {
                rows.iter()
                    .filter(|(_, posed, outcome)| keep(*posed, outcome.as_deref()))
                    .map(|(label, ..)| label.clone())
                    .collect::<Vec<_>>()
            };
            let started = Local
                .timestamp_millis_opt(started)
                .single()
                .map(|time| time.to_rfc3339());
            let line = json!({
                "sitting": sitting,
                "started": started,
                "due": with(&|_, _| true),
                "posed": with(&|posed, outcome| posed || outcome.is_some_and(|o| o != "declined")),
                "answered": with(&|_, outcome| outcome.is_some_and(|o| o != "declined")),
                "declined": with(&|_, outcome| outcome == Some("declined")),
                "passed": with(&|_, outcome| outcome == Some("passed")),
                "failed": with(&|_, outcome| outcome == Some("failed")),
                "helped": with(&|_, outcome| outcome == Some("helped")),
                "due_not_taken": not_taken,
                "paper_redos_due": serde_json::from_str::<Value>(&redos).unwrap_or(Value::Null),
            });
            out.push_str(&line.to_string());
            out.push('\n');
        }
        Ok(out)
    }
}

fn create_private(path: &Path) -> Result<(), String> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(error(e)),
    }
}

fn begin(tx: &Transaction<'_>, now: i64, due: &[(String, String)]) -> Result<(), String> {
    let today = local_day(now).to_string();
    let sitting = crate::learner::store::new_id("sitting").map_err(error)?;
    let taken = due.len().min(MAX_CHECKS_PER_SITTING);
    tx.execute(
        "INSERT INTO sittings(sitting_id,started_ms,day,not_taken) VALUES(?1,?2,?3,?4)",
        params![sitting, now, today, (due.len() - taken) as i64],
    )
    .map_err(error)?;
    for (number, (item, label)) in due.iter().take(taken).enumerate() {
        tx.execute(
            "INSERT INTO sitting_items(sitting_id,item_id,number,label) VALUES(?1,?2,?3,?4)",
            params![sitting, item, number as i64 + 1, label],
        )
        .map_err(error)?;
    }
    Ok(())
}

fn from_version_1(conn: &mut Connection, workspace: &Workspace) -> Result<(), String> {
    let checks = {
        let mut statement = conn
            .prepare(
                "SELECT item_id,label,status,passes,due_day,last_pass_day,misses_json,created_ms,updated_ms FROM items",
            )
            .map_err(error)?;
        statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, i64>(8)?,
                ))
            })
            .map_err(error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(error)?
    };
    let store = crate::practice::open_store(workspace).map_err(error)?;
    let tx = store.conn.unchecked_transaction().map_err(error)?;
    for (id, label, status, passes, due, last_pass, misses, created, updated) in checks {
        let passes = if status == "closed" {
            3
        } else {
            passes.clamp(0, 2)
        };
        tx.execute(
            "INSERT OR IGNORE INTO practice_items(item_id,origin,label,label_key,logged_on,base_passes,
                base_next_on,base_last_pass_on,base_misses_json,created_at_ms,updated_at_ms)
             VALUES(?1,'chat',?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                id,
                label,
                crate::learner::label_key(&label),
                local_day(created).to_string(),
                passes,
                (passes < 3).then_some(due).flatten(),
                last_pass,
                if serde_json::from_str::<Vec<String>>(&misses).is_ok() {
                    misses
                } else {
                    "[]".to_owned()
                },
                created,
                updated
            ],
        )
        .map_err(error)?;
    }
    tx.commit().map_err(error)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(error)?;
    let version: i64 = tx
        .query_row("SELECT version FROM meta", [], |r| r.get(0))
        .map_err(error)?;
    if version == 1 {
        tx.execute_batch(FROM_VERSION_1).map_err(error)?;
    }
    tx.commit().map_err(error)
}

pub(crate) fn says_check(reply_lowercase: &str, number: i64) -> bool {
    let needle = format!("quick check {number}");
    reply_lowercase.match_indices(&needle).any(|(start, _)| {
        !reply_lowercase[start + needle.len()..]
            .chars()
            .next()
            .is_some_and(|next| next.is_ascii_digit())
    })
}

fn opener_text(checks: &[Check], redos: &[DueRedos]) -> String {
    let mut text = String::new();
    let mut shortened: Vec<Check> = Vec::new();
    for (misses, questions) in [
        (MAX_MISS_CHARS, MAX_QUESTION_CHARS),
        (160, 300),
        (80, 200),
        (40, 160),
        (0, 120),
        (0, 60),
    ] {
        let clipped = |text: &Option<String>| text.as_deref().map(|t| clip(t, questions));
        shortened = checks
            .iter()
            .map(|check| Check {
                number: check.number,
                label: check.label.clone(),
                misses: match misses {
                    0 => Vec::new(),
                    _ => check.misses.iter().map(|miss| clip(miss, misses)).collect(),
                },
                question: clipped(&check.question),
                key: clipped(&check.key),
            })
            .collect();
        let redos: Vec<DueRedos> = redos
            .iter()
            .map(|redo| DueRedos {
                rows: redo
                    .rows
                    .iter()
                    .map(|row| crate::practice::DueRow {
                        question: clipped(&row.question),
                        key: clipped(&row.key),
                        ..row.clone()
                    })
                    .collect(),
                ..redo.clone()
            })
            .collect();
        text = opener_text_with(&shortened, &redos, &[]);
        if text.len() <= MAX_OPENER_BYTES {
            return text;
        }
    }
    let bare: Vec<DueRedos> = redos
        .iter()
        .map(|redo| DueRedos {
            rows: redo
                .rows
                .iter()
                .map(|row| crate::practice::DueRow {
                    question: None,
                    key: None,
                    ..row.clone()
                })
                .collect(),
            ..redo.clone()
        })
        .collect();
    for listed in (0..=bare.len()).rev() {
        text = opener_text_with(&shortened, &bare[..listed], &bare[listed..]);
        if text.len() <= MAX_OPENER_BYTES {
            return text;
        }
    }
    let mut end = MAX_OPENER_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text
}

fn opener_text_with(checks: &[Check], redos: &[DueRedos], unlisted: &[DueRedos]) -> String {
    let mut text = String::from(
        "Sitting opener. A new sitting has begun: open it with what is due, before new material. The learner may decline any of it at no cost, and a skip, no-quiz or stop request wins.",
    );
    if !checks.is_empty() {
        text.push_str("\n\nQuick checks, one at a time, about a minute each:");
        for check in checks {
            text.push_str(&format!("\n{}. Label \"{}\".", check.number, check.label));
            if !check.misses.is_empty() {
                text.push_str(&format!(" Earlier misses: {}", check.misses.join(" | ")));
            }
            if let Some(question) = &check.question {
                text.push_str(&format!(" Stored question: {question}"));
                if let Some(key) = &check.key {
                    text.push_str(&format!(" Key, hidden until they answer: {key}"));
                }
            }
        }
        let first = checks[0].number;
        text.push_str(&format!("\nYour first reply of this sitting begins with the words \"Quick check {first}:\", with no greeting or recap before them, and poses check {first}."));
        text.push_str(" Pose any later check the same way, as \"Quick check <number>:\", after the learner answers. Pose a stored question as written, and show none of its key before they answer. Without one, make a new problem the learner has not seen, answered in a line or two, in which they must use the skill without being prompted: the question never names or points at the skill, the label, the step or the earlier mistake. Ask them to solve or decide something, never to state the step itself. Choose it so that each earlier miss would give a different answer from the right one. After their answer, show lettered options for that problem: the right answer, and one wrong option for each earlier miss. Then record their first answer with learning_step: an observation that cites their message as learner_response, with attempt {label: exactly as above, question: the check as you posed it, first_try, help, cold, next: {question, answer_key}}, where next is the check's next problem, made the same way: it asks for a result and never names or points at the skill, the label, the method, the step or the earlier mistake, and each earlier miss gives it a different answer from the right one. cold is true when they answered with no help, and false with any help. first_try is right only when the answer shows that step done right: a result each earlier miss would have changed, and no reason that still holds an earlier miss. Otherwise it is wrong. A decline is recorded the same way with first_try declined and cold false.");
    }
    if !redos.is_empty() || !unlisted.is_empty() {
        text.push_str("\n\nPaper redos due:");
        for redo in redos {
            text.push_str(&format!("\n- {}: {} due", redo.gap_log, redo.due));
            if let Some(first) = &redo.first {
                text.push_str(&format!(", first {first}"));
            }
            text.push('.');
            for row in &redo.rows {
                text.push_str(&format!("\n  {}, tag \"{}\": ", row.name, row.tag));
                match (&row.question, &row.key) {
                    (Some(question), Some(key)) => text.push_str(&format!(
                        "stored question: {question} Key, hidden until they answer: {key}"
                    )),
                    (Some(question), None) => {
                        text.push_str(&format!("stored question: {question}"));
                    }
                    _ => text.push_str("no stored question."),
                }
            }
        }
        if !unlisted.is_empty() {
            text.push_str(&format!(
                "\n- {} more gap logs with redos due, {} due in all.",
                unlisted.len(),
                unlisted.iter().map(|redo| redo.due).sum::<u64>()
            ));
        }
        text.push_str("\nSay how many are due and which is first. They run as the first block of the paper session: load stage papers_loop, which picks and records them.");
    }
    text
}

pub(crate) fn carries_learner_message(request: &Value, result: &Value) -> bool {
    result["update_status"] == "committed"
        && (request.get("native_capture").is_some_and(|v| !v.is_null())
            || request["reported_events"]
                .as_array()
                .is_some_and(|events| events.iter().any(|e| e["speaker"] == "learner")))
}

pub(crate) fn notice_clause(workspace: &Workspace, by: Option<&str>) -> Option<String> {
    let sittings = Sittings::open_existing(workspace).ok()??;
    if let Ok(Some(left)) = sittings.checks_only_left(workspace, by, now_ms()) {
        return Some(checks_only::notice_clause(left));
    }
    if workspace.opener() == OpenerMode::Off {
        return None;
    }
    let counts = sittings.open_counts().ok()??;
    Some(format!(
        " Sitting quick checks: due {}, posed {}.",
        counts.due, counts.posed
    ))
}

pub(crate) fn set_opener(mode: OpenerMode) -> std::process::ExitCode {
    let result = std::env::current_dir()
        .and_then(|cwd| Workspace::discover(&cwd))
        .map_err(|e| e.to_string())
        .and_then(|workspace| {
            workspace.ok_or_else(|| {
                "no Magnesium study folder here; run `magnesium init` first".to_owned()
            })
        })
        .and_then(|workspace| workspace.set_opener(mode).map_err(|e| e.to_string()));
    match result {
        Ok(()) => {
            println!(
                "{}",
                match mode {
                    OpenerMode::On =>
                        "Opener on: each sitting opens with the quick checks and paper redos that are due. New material is not refused.",
                    OpenerMode::Block =>
                        "Opener on, with a block: each sitting opens with what is due, and new teaching stages are refused until each quick check is posed or declined.",
                    OpenerMode::Off =>
                        "Opener off: sittings no longer open with what is due. Quick checks are still scheduled.",
                }
            );
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("magnesium: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

pub(crate) mod checks_only;

#[cfg(test)]
mod tests;
