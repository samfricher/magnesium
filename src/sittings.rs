//! Sittings and quick checks.
//!
//! A sitting is one stretch of study. It begins with the first learner
//! message after [`SITTING_GAP_MS`] without one, and opens with what is due:
//! quick checks, which the runtime schedules from the attempt facts on
//! recorded observations, and paper redos, which gap logs schedule.
//!
//! This store sits beside the learner record, in `.magnesium/sittings.sqlite`,
//! because the learner store refuses any other schema version and a review
//! item there needs a memory. Nothing here is evidence about the learner: the
//! observations an attempt rides on are. A failure here never fails the tool
//! call or hook that triggered it.
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{Days, Local, NaiveDate, TimeZone};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde_json::{Value, json};

use crate::learner::{AttemptHelp, FirstTry};
use crate::workspace::{OpenerMode, Workspace};

pub(crate) const STORE_FILE: &str = "sittings.sqlite";
pub(crate) const LOG_FILE: &str = "sittings.log";
const STORE_VERSION: i64 = 1;
/// About three hours without a learner message ends a sitting.
pub(crate) const SITTING_GAP_MS: i64 = 3 * 60 * 60 * 1000;
/// About a minute each, so about three minutes at the start of a sitting.
const MAX_CHECKS_PER_SITTING: i64 = 3;
/// Days until the next check: after a miss, after the first pass, after the second.
const INTERVAL_DAYS: [u64; 3] = [1, 3, 7];
const PASSES_TO_CLOSE: i64 = 3;
const MAX_MISSES_KEPT: usize = 3;
const MAX_MISS_CHARS: usize = 240;
const MAX_LOGGED_SITTINGS: i64 = 1000;
const MAX_OPENER_BYTES: usize = 4096;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta(
  singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
  version INTEGER NOT NULL,
  last_learner_ms INTEGER
);
CREATE TABLE IF NOT EXISTS items(
  item_id TEXT PRIMARY KEY,
  label TEXT NOT NULL,
  label_key TEXT NOT NULL UNIQUE,
  status TEXT NOT NULL CHECK (status IN ('open','closed')),
  passes INTEGER NOT NULL DEFAULT 0,
  due_day TEXT,
  last_pass_day TEXT,
  misses_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(misses_json)),
  created_ms INTEGER NOT NULL,
  updated_ms INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS attempts(
  observation_id TEXT PRIMARY KEY,
  label_key TEXT NOT NULL,
  first_try TEXT NOT NULL,
  help TEXT NOT NULL,
  day TEXT NOT NULL,
  sitting_id TEXT,
  effect TEXT NOT NULL,
  created_ms INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS sittings(
  sitting_no INTEGER PRIMARY KEY,
  sitting_id TEXT NOT NULL UNIQUE,
  started_ms INTEGER NOT NULL,
  day TEXT NOT NULL,
  opener_delivered INTEGER NOT NULL DEFAULT 0,
  redos_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(redos_json))
);
CREATE TABLE IF NOT EXISTS sitting_items(
  sitting_id TEXT NOT NULL REFERENCES sittings(sitting_id),
  item_id TEXT NOT NULL REFERENCES items(item_id),
  number INTEGER NOT NULL,
  label TEXT NOT NULL,
  posed INTEGER NOT NULL DEFAULT 0,
  outcome TEXT CHECK (outcome IN ('passed','failed','helped','declined')),
  PRIMARY KEY (sitting_id, item_id)
);
CREATE INDEX IF NOT EXISTS item_due ON items(status, due_day);
";

/// One attempt from a committed `learning_step` update.
#[derive(Clone, Debug)]
pub(crate) struct RecordedAttempt {
    pub(crate) observation_id: String,
    pub(crate) label: String,
    pub(crate) first_try: FirstTry,
    pub(crate) help: AttemptHelp,
    /// The observation's description: for a miss, what went wrong.
    pub(crate) description: String,
}

pub(crate) struct Sittings {
    conn: Connection,
    log: PathBuf,
}

/// The quick-check counts a turn's notice states, while any is still open.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Counts {
    pub(crate) due: i64,
    pub(crate) posed: i64,
}

pub(crate) fn now_ms() -> i64 {
    crate::learner::store::now_ms()
}

pub(crate) fn local_day(ms: i64) -> NaiveDate {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map_or(NaiveDate::MIN, |time| time.date_naive())
}

fn day_after(day: NaiveDate, days: u64) -> NaiveDate {
    day.checked_add_days(Days::new(days)).unwrap_or(day)
}

/// Labels match whatever their case and spacing.
pub(crate) fn label_key(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
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

impl Sittings {
    /// Open the store, creating it first. Only a write calls this, so a read
    /// never leaves a new file in the study folder.
    pub(crate) fn open(workspace: &Workspace) -> Result<Self, String> {
        Self::open_with(workspace, true)?.ok_or_else(|| error("store missing"))
    }

    /// Open the store if it exists.
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
        let conn = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(error)?;
        conn.busy_timeout(Duration::from_secs(2)).map_err(error)?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(error)?;
        conn.execute_batch(SCHEMA).map_err(error)?;
        conn.execute(
            "INSERT OR IGNORE INTO meta(singleton,version) VALUES(1,?1)",
            [STORE_VERSION],
        )
        .map_err(error)?;
        let version: i64 = conn
            .query_row("SELECT version FROM meta", [], |r| r.get(0))
            .map_err(error)?;
        if version != STORE_VERSION {
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

    /// Note a learner message. The first one after the gap begins a sitting,
    /// which takes the quick checks due that day. Returns whether one began.
    pub(crate) fn note_learner_message(&mut self, now: i64) -> Result<bool, String> {
        let tx = self.write()?;
        let last: Option<i64> = tx
            .query_row("SELECT last_learner_ms FROM meta", [], |r| r.get(0))
            .map_err(error)?;
        tx.execute("UPDATE meta SET last_learner_ms=?1", [now])
            .map_err(error)?;
        let begins = last.is_none_or(|last| now - last >= SITTING_GAP_MS);
        if begins {
            begin(&tx, now)?;
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

    /// Quick checks of the current sitting, while any is neither posed nor
    /// declined. An answer counts as posed.
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

    /// The opener for the first tool result of a sitting, once. It names the
    /// quick checks taken at the start and the paper redos due today. Before
    /// it reads the gap logs, an older one is converted.
    pub(crate) fn take_opener(
        &mut self,
        workspace: &Workspace,
        now: i64,
    ) -> Result<Option<String>, String> {
        let Some((sitting, delivered)) = self.current()? else {
            return Ok(None);
        };
        if delivered {
            return Ok(None);
        }
        let today = local_day(now);
        crate::digest::convert_gap_logs(workspace, today);
        let redos = crate::digest::due_redos(workspace.root(), today);
        let checks = self.checks(&sitting)?;
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
        if claimed == 0 || (checks.is_empty() && redos.is_empty()) {
            return Ok(None);
        }
        self.write_log();
        Ok(Some(opener_text(&checks, &redos)))
    }

    /// The current sitting's opener as it stands, without claiming it.
    pub(crate) fn opener_now(
        &self,
        workspace: &Workspace,
        now: i64,
    ) -> Result<Option<String>, String> {
        let Some((sitting, _)) = self.current()? else {
            return Ok(None);
        };
        let redos = crate::digest::due_redos(workspace.root(), local_day(now));
        let checks = self.checks(&sitting)?;
        Ok((!checks.is_empty() || !redos.is_empty()).then(|| opener_text(&checks, &redos)))
    }

    /// (number, label, misses) of each quick check in a sitting.
    fn checks(&self, sitting: &str) -> Result<Vec<(i64, String, Vec<String>)>, String> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT s.number, s.label, i.misses_json FROM sitting_items s JOIN items i ON i.item_id=s.item_id WHERE s.sitting_id=?1 ORDER BY s.number",
            )
            .map_err(error)?;
        statement
            .query_map([sitting], |r| {
                let misses: String = r.get(2)?;
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    serde_json::from_str(&misses).unwrap_or_default(),
                ))
            })
            .map_err(error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(error)
    }

    /// The tutor's reply poses check n when it says "Quick check n".
    pub(crate) fn note_reply(&mut self, reply: &str) -> Result<(), String> {
        let Some((sitting, _)) = self.current()? else {
            return Ok(());
        };
        let reply = reply.to_lowercase();
        let posed = self
            .checks(&sitting)?
            .into_iter()
            .filter(|(number, ..)| says_check(&reply, *number))
            .map(|(number, ..)| number)
            .collect::<Vec<_>>();
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

    /// Apply attempts from a committed update. Each observation counts once,
    /// so replaying the same request changes nothing.
    pub(crate) fn apply_attempts(
        &mut self,
        attempts: &[RecordedAttempt],
        now: i64,
    ) -> Result<(), String> {
        if attempts.is_empty() {
            return Ok(());
        }
        let today = local_day(now);
        let tx = self.write()?;
        let sitting: Option<String> = tx
            .query_row(
                "SELECT sitting_id FROM sittings ORDER BY sitting_no DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(error)?;
        for attempt in attempts {
            let seen: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM attempts WHERE observation_id=?1)",
                    [&attempt.observation_id],
                    |r| r.get(0),
                )
                .map_err(error)?;
            if seen {
                continue;
            }
            let effect = apply(&tx, attempt, sitting.as_deref(), today, now)?;
            tx.execute(
                "INSERT INTO attempts(observation_id,label_key,first_try,help,day,sitting_id,effect,created_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    attempt.observation_id,
                    label_key(&attempt.label),
                    attempt.first_try.as_str(),
                    attempt.help.as_str(),
                    today.to_string(),
                    sitting,
                    effect,
                    now
                ],
            )
            .map_err(error)?;
        }
        tx.commit().map_err(error)?;
        self.write_log();
        Ok(())
    }

    /// Rewrite the local log, one JSON line per sitting, from this store.
    fn write_log(&self) {
        if let Ok(lines) = self.log_lines() {
            let _ = crate::workspace::write_private_atomic(&self.log, lines.as_bytes());
        }
    }

    fn log_lines(&self) -> Result<String, String> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT sitting_id,started_ms,redos_json FROM (SELECT * FROM sittings ORDER BY sitting_no DESC LIMIT ?1) ORDER BY sitting_no",
            )
            .map_err(error)?;
        let sittings = statement
            .query_map([MAX_LOGGED_SITTINGS], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
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
        for (sitting, started, redos) in sittings {
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

fn begin(tx: &Transaction<'_>, now: i64) -> Result<(), String> {
    let today = local_day(now).to_string();
    let sitting = crate::learner::store::new_id("sitting").map_err(error)?;
    tx.execute(
        "INSERT INTO sittings(sitting_id,started_ms,day) VALUES(?1,?2,?3)",
        params![sitting, now, today],
    )
    .map_err(error)?;
    tx.execute(
        "INSERT INTO sitting_items(sitting_id,item_id,number,label)
         SELECT ?1, item_id, row_number() OVER (ORDER BY due_day, created_ms, seq), label
         FROM (SELECT rowid AS seq, * FROM items WHERE status='open' AND due_day<=?2 ORDER BY due_day, created_ms, rowid LIMIT ?3)",
        params![sitting, today, MAX_CHECKS_PER_SITTING],
    )
    .map_err(error)?;
    Ok(())
}

/// One quick check, as stored.
struct Item {
    id: String,
    open: bool,
    passes: i64,
    due_day: Option<String>,
    last_pass_day: Option<String>,
    misses: String,
}

/// What one attempt does to its quick check and to the current sitting.
fn apply(
    tx: &Transaction<'_>,
    attempt: &RecordedAttempt,
    sitting: Option<&str>,
    today: NaiveDate,
    now: i64,
) -> Result<&'static str, String> {
    let key = label_key(&attempt.label);
    let label = attempt
        .label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let item = tx
        .query_row(
            "SELECT item_id,status,passes,due_day,last_pass_day,misses_json FROM items WHERE label_key=?1",
            [&key],
            |r| {
                Ok(Item {
                    id: r.get(0)?,
                    open: r.get::<_, String>(1)? == "open",
                    passes: r.get(2)?,
                    due_day: r.get(3)?,
                    last_pass_day: r.get(4)?,
                    misses: r.get(5)?,
                })
            },
        )
        .optional()
        .map_err(error)?;
    let in_sitting = |item_id: &str| -> Result<bool, String> {
        let Some(sitting) = sitting else {
            return Ok(false);
        };
        tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sitting_items WHERE sitting_id=?1 AND item_id=?2 AND outcome IS NULL)",
            params![sitting, item_id],
            |r| r.get(0),
        )
        .map_err(error)
    };
    let settle = |item_id: &str, outcome: &str| -> Result<(), String> {
        if in_sitting(item_id)? {
            tx.execute(
                "UPDATE sitting_items SET outcome=?3, posed=1 WHERE sitting_id=?1 AND item_id=?2",
                params![sitting, item_id, outcome],
            )
            .map_err(error)?;
        }
        Ok(())
    };
    match attempt.first_try {
        FirstTry::Wrong => {
            let miss = clip(&attempt.description, MAX_MISS_CHARS);
            let due = day_after(today, INTERVAL_DAYS[0]).to_string();
            match item {
                None => {
                    let id = crate::learner::store::new_id("check").map_err(error)?;
                    tx.execute(
                        "INSERT INTO items(item_id,label,label_key,status,passes,due_day,misses_json,created_ms,updated_ms) VALUES(?1,?2,?3,'open',0,?4,?5,?6,?6)",
                        params![id, label, key, due, json!([miss]).to_string(), now],
                    )
                    .map_err(error)?;
                }
                Some(Item { id, misses, .. }) => {
                    let mut misses: Vec<String> = serde_json::from_str(&misses).unwrap_or_default();
                    misses.retain(|kept| kept != &miss);
                    misses.push(miss);
                    let excess = misses.len().saturating_sub(MAX_MISSES_KEPT);
                    misses.drain(..excess);
                    settle(&id, "failed")?;
                    tx.execute(
                        "UPDATE items SET label=?2,status='open',passes=0,due_day=?3,last_pass_day=NULL,misses_json=?4,updated_ms=?5 WHERE item_id=?1",
                        params![id, label, due, json!(misses).to_string(), now],
                    )
                    .map_err(error)?;
                }
            }
            Ok("scheduled")
        }
        FirstTry::Right => {
            let Some(Item {
                id,
                open,
                passes,
                due_day,
                last_pass_day,
                ..
            }) = item
            else {
                return Ok("none");
            };
            let today_text = today.to_string();
            let due = open && due_day.as_deref().is_some_and(|d| d <= today_text.as_str());
            if !due {
                return Ok("none");
            }
            if attempt.help != AttemptHelp::None {
                settle(&id, "helped")?;
                return Ok("helped");
            }
            if last_pass_day.as_deref() == Some(today_text.as_str()) {
                return Ok("none");
            }
            let passes = passes + 1;
            let (status, due) = if passes >= PASSES_TO_CLOSE {
                ("closed", None)
            } else {
                let days = INTERVAL_DAYS[usize::try_from(passes).unwrap_or(2).min(2)];
                ("open", Some(day_after(today, days).to_string()))
            };
            settle(&id, "passed")?;
            tx.execute(
                "UPDATE items SET status=?2,passes=?3,due_day=?4,last_pass_day=?5,updated_ms=?6 WHERE item_id=?1",
                params![id, status, passes, due, today_text, now],
            )
            .map_err(error)?;
            Ok(if status == "closed" {
                "closed"
            } else {
                "passed"
            })
        }
        FirstTry::Declined => {
            let Some(Item { id, .. }) = item else {
                return Ok("none");
            };
            if !in_sitting(&id)? {
                return Ok("none");
            }
            settle(&id, "declined")?;
            Ok("declined")
        }
    }
}

fn says_check(reply_lowercase: &str, number: i64) -> bool {
    let needle = format!("quick check {number}");
    reply_lowercase.match_indices(&needle).any(|(start, _)| {
        !reply_lowercase[start + needle.len()..]
            .chars()
            .next()
            .is_some_and(|next| next.is_ascii_digit())
    })
}

fn opener_text(checks: &[(i64, String, Vec<String>)], redos: &[crate::digest::DueRedos]) -> String {
    let mut text = String::from(
        "Sitting opener. A new sitting has begun: open it with what is due, before new material. The learner may decline any of it at no cost, and a skip, no-quiz or stop request wins.",
    );
    if !checks.is_empty() {
        text.push_str("\n\nQuick checks, one at a time, about a minute each:");
        for (number, label, misses) in checks {
            text.push_str(&format!("\n{number}. Label \"{label}\"."));
            if !misses.is_empty() {
                text.push_str(&format!(" Earlier misses: {}", misses.join(" | ")));
            }
        }
        text.push_str("\nBegin each with \"Quick check <number>:\" and pose a new instance the learner has not seen, answered in a line or two. After their answer, show lettered options for that instance: the right answer, and one wrong option for each earlier miss. Then record their first answer with learning_step: an observation that cites their message as learner_response, with attempt {label: exactly as above, first_try: right or wrong, help}. A decline is recorded the same way with first_try declined.");
    }
    if !redos.is_empty() {
        text.push_str("\n\nPaper redos due:");
        for redo in redos {
            text.push_str(&format!("\n- {}: {} due", redo.gap_log, redo.due));
            if let Some(first) = &redo.first {
                text.push_str(&format!(", first {first}"));
            }
            text.push('.');
        }
        text.push_str("\nSay how many are due and which is first. They run as the first block of the paper session: load stage papers_loop, which picks and records them.");
    }
    if text.len() > MAX_OPENER_BYTES {
        let mut end = MAX_OPENER_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text
}

/// The attempts in a committed update, with their observation IDs from its
/// `references`. Nothing for a rejected or uncommitted update.
pub(crate) fn recorded_attempts(request: &Value, result: &Value) -> Vec<RecordedAttempt> {
    if result["update_status"] != "committed" {
        return Vec::new();
    }
    request["observations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|observation| {
            let attempt: crate::learner::Attempt =
                serde_json::from_value(observation.get("attempt")?.clone()).ok()?;
            let client_ref = observation["client_ref"].as_str()?;
            let observation_id = result["references"][client_ref].as_str()?.to_owned();
            Some(RecordedAttempt {
                observation_id,
                label: attempt.label.trim().to_owned(),
                first_try: attempt.first_try,
                help: attempt.help,
                description: observation["description"].as_str().unwrap_or("").to_owned(),
            })
        })
        .collect()
}

/// A committed update that carried learner input is a learner message, for
/// hosts with no prompt hook.
pub(crate) fn carries_learner_message(request: &Value, result: &Value) -> bool {
    result["update_status"] == "committed"
        && (request.get("native_capture").is_some_and(|v| !v.is_null())
            || request["reported_events"]
                .as_array()
                .is_some_and(|events| events.iter().any(|e| e["speaker"] == "learner")))
}

/// The clause a turn's notice carries while a quick check is open.
pub(crate) fn notice_clause(workspace: &Workspace) -> Option<String> {
    if workspace.opener() == OpenerMode::Off {
        return None;
    }
    let counts = Sittings::open_existing(workspace)
        .ok()??
        .open_counts()
        .ok()??;
    Some(format!(
        " Sitting quick checks: due {}, posed {}.",
        counts.due, counts.posed
    ))
}

/// `magnesium opener on|off|block` in the current study folder.
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

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: i64 = 60 * 60 * 1000;
    const DAY: i64 = 24 * HOUR;

    fn fixture() -> (PathBuf, Workspace) {
        let root =
            std::env::temp_dir().join(crate::learner::store::new_id("sittings-test").unwrap());
        std::fs::create_dir_all(root.join(crate::workspace::CONTROL_DIR)).unwrap();
        std::fs::write(
            root.join(crate::workspace::CONTROL_DIR).join("config.toml"),
            "schema_version = 1\nworkspace_id = \"sittings-test\"\n",
        )
        .unwrap();
        let workspace = Workspace::discover(&root).unwrap().unwrap();
        (root, workspace)
    }

    fn attempt(id: &str, label: &str, first_try: FirstTry, help: AttemptHelp) -> RecordedAttempt {
        RecordedAttempt {
            observation_id: id.into(),
            label: label.into(),
            first_try,
            help,
            description: format!("{id}: called b the unknown"),
        }
    }

    /// Noon local time, `days` after a fixed day, so day arithmetic is plain.
    fn noon(days: i64) -> i64 {
        let day = NaiveDate::from_ymd_opt(2026, 11, 2).unwrap();
        let noon = day.and_hms_opt(12, 0, 0).unwrap();
        Local
            .from_local_datetime(&noon)
            .single()
            .unwrap()
            .timestamp_millis()
            + days * DAY
    }

    #[test]
    fn a_miss_returns_after_one_three_and_seven_days_and_closes_after_three_passes() {
        let (root, workspace) = fixture();
        let mut s = Sittings::open(&workspace).unwrap();
        s.note_learner_message(noon(0)).unwrap();
        s.apply_attempts(
            &[attempt(
                "o1",
                "Sign of b",
                FirstTry::Wrong,
                AttemptHelp::None,
            )],
            noon(0),
        )
        .unwrap();
        // A right answer the same day is no pass: the check is not due yet.
        s.apply_attempts(
            &[attempt(
                "o2",
                "sign of  B",
                FirstTry::Right,
                AttemptHelp::None,
            )],
            noon(0),
        )
        .unwrap();
        let state = |s: &Sittings| -> (String, i64, Option<String>) {
            s.conn
                .query_row("SELECT status,passes,due_day FROM items", [], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })
                .unwrap()
        };
        assert_eq!(
            state(&s),
            ("open".into(), 0, Some(local_day(noon(1)).to_string()))
        );

        // Day 1: a new sitting takes it, the reply poses it, a right answer passes it.
        assert!(s.note_learner_message(noon(1)).unwrap());
        assert_eq!(s.open_counts().unwrap(), Some(Counts { due: 1, posed: 0 }));
        let opener = s.take_opener(&workspace, noon(1)).unwrap().unwrap();
        assert!(
            opener.contains("1. Label \"Sign of b\". Earlier misses: o1: called b the unknown"),
            "{opener}"
        );
        assert_eq!(
            s.take_opener(&workspace, noon(1)).unwrap(),
            None,
            "once per sitting"
        );
        s.note_reply("Quick check 1: in 3x² − 7x + 2, what is b?")
            .unwrap();
        assert_eq!(s.open_counts().unwrap(), None);
        s.apply_attempts(
            &[attempt(
                "o3",
                "sign of b",
                FirstTry::Right,
                AttemptHelp::None,
            )],
            noon(1),
        )
        .unwrap();
        assert_eq!(
            state(&s),
            ("open".into(), 1, Some(local_day(noon(4)).to_string()))
        );

        // Replaying the same observation changes nothing.
        s.apply_attempts(
            &[attempt(
                "o3",
                "sign of b",
                FirstTry::Right,
                AttemptHelp::None,
            )],
            noon(4),
        )
        .unwrap();
        assert_eq!(state(&s).1, 1);

        // Day 4: a helped answer is no pass; an unaided one is.
        assert!(s.note_learner_message(noon(4)).unwrap());
        s.apply_attempts(
            &[attempt(
                "o4",
                "sign of b",
                FirstTry::Right,
                AttemptHelp::Hint,
            )],
            noon(4),
        )
        .unwrap();
        assert_eq!(state(&s).1, 1);
        s.apply_attempts(
            &[attempt(
                "o5",
                "sign of b",
                FirstTry::Right,
                AttemptHelp::None,
            )],
            noon(4),
        )
        .unwrap();
        assert_eq!(
            state(&s),
            ("open".into(), 2, Some(local_day(noon(11)).to_string()))
        );

        // Day 11: the third pass closes it, and nothing is due after.
        s.note_learner_message(noon(11)).unwrap();
        s.apply_attempts(
            &[attempt(
                "o6",
                "sign of b",
                FirstTry::Right,
                AttemptHelp::None,
            )],
            noon(11),
        )
        .unwrap();
        assert_eq!(state(&s), ("closed".into(), 3, None));
        assert!(s.note_learner_message(noon(30)).unwrap());
        assert_eq!(s.open_counts().unwrap(), None);
        assert_eq!(s.take_opener(&workspace, noon(30)).unwrap(), None);

        // A miss reopens it for the next day, with both misses known.
        s.apply_attempts(
            &[attempt(
                "o7",
                "sign of b",
                FirstTry::Wrong,
                AttemptHelp::None,
            )],
            noon(30),
        )
        .unwrap();
        assert_eq!(
            state(&s),
            ("open".into(), 0, Some(local_day(noon(31)).to_string()))
        );
        let log = std::fs::read_to_string(root.join(".magnesium").join(LOG_FILE)).unwrap();
        assert_eq!(log.lines().count(), 5, "{log}");
        let second: Value = serde_json::from_str(log.lines().nth(1).unwrap()).unwrap();
        assert_eq!(second["due"], json!(["Sign of b"]));
        assert_eq!(second["posed"], json!(["Sign of b"]));
        assert_eq!(second["passed"], json!(["Sign of b"]));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_sitting_begins_after_three_hours_and_takes_at_most_three_checks() {
        let (root, workspace) = fixture();
        let mut s = Sittings::open(&workspace).unwrap();
        assert!(s.note_learner_message(noon(0)).unwrap());
        let misses = (1..=5)
            .map(|n| {
                attempt(
                    &format!("o{n}"),
                    &format!("skill {n}"),
                    FirstTry::Wrong,
                    AttemptHelp::None,
                )
            })
            .collect::<Vec<_>>();
        s.apply_attempts(&misses, noon(0)).unwrap();
        assert!(!s.note_learner_message(noon(0) + 2 * HOUR).unwrap());
        assert!(
            !s.note_learner_message(noon(0) + 4 * HOUR + HOUR / 2)
                .unwrap(),
            "2.5 hours after the last message"
        );
        assert!(s.note_learner_message(noon(1)).unwrap());
        assert_eq!(s.open_counts().unwrap(), Some(Counts { due: 3, posed: 0 }));
        // "Quick check 1" is not "Quick check 12"; a decline settles a check.
        s.note_reply("Quick check 12 is not here.").unwrap();
        assert_eq!(s.open_counts().unwrap(), Some(Counts { due: 3, posed: 0 }));
        s.note_reply("QUICK CHECK 2: try this.").unwrap();
        s.apply_attempts(
            &[attempt(
                "d1",
                "skill 1",
                FirstTry::Declined,
                AttemptHelp::None,
            )],
            noon(1),
        )
        .unwrap();
        assert_eq!(s.open_counts().unwrap(), Some(Counts { due: 3, posed: 2 }));
        // A decline is free: the check stays due for the next sitting.
        assert!(s.note_learner_message(noon(2)).unwrap());
        let opener = s.take_opener(&workspace, noon(2)).unwrap().unwrap();
        assert!(opener.contains("Label \"skill 1\""), "{opener}");
        std::fs::remove_dir_all(root).unwrap();
    }
}
