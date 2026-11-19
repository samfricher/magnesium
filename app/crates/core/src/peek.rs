use std::fs;
use std::path::Path;

use chrono::NaiveDate;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

fn open(path: &Path) -> Option<Connection> {
    if !path.is_file() {
        return None;
    }
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()
}

#[derive(Clone, Debug)]
pub struct Check {
    pub label: String,
    pub passes: u8,
    pub closed: bool,
    pub due: Option<NaiveDate>,
    pub last_pass: Option<NaiveDate>,
    pub last_miss: Option<String>,
}

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

pub fn checks_due(root: &Path, today: NaiveDate) -> Option<u32> {
    let checks = checks(root)?;
    Some(
        checks
            .iter()
            .filter(|check| !check.closed && check.due.is_some_and(|due| due <= today))
            .count() as u32,
    )
}

pub fn goal_of_conversation(root: &Path, conversation: &str) -> Option<String> {
    if conversation.trim().is_empty() {
        return None;
    }
    let conn = open(&root.join(".magnesium/state.sqlite"))?;
    let thread = format!(":claude:{conversation}");
    conn.query_row(
        "SELECT ls.goal FROM host_bindings hb JOIN learning_sessions ls ON ls.session_id = hb.session_id \
         WHERE substr(hb.host_thread_id, -length(?1)) = ?1 ORDER BY ls.updated_at_ms DESC LIMIT 1",
        [&thread],
        |row| row.get::<_, String>(0),
    )
    .ok()
    .filter(|goal| !goal.trim().is_empty())
}

const SITTING_GAP_MS: i64 = 3 * 60 * 60 * 1000;
const CHECKS_PER_SITTING: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Opening {
    pub checks: u32,
    pub redos: u32,
}

pub fn opening(root: &Path, checks_due: u32, redos_due: u32, now_ms: i64) -> Option<Opening> {
    let checks = checks_due.min(CHECKS_PER_SITTING);
    if checks + redos_due == 0 || opener_off(root) {
        return None;
    }
    let begins = match last_learner_ms(root)? {
        None => true,
        Some(last) => now_ms - last >= SITTING_GAP_MS,
    };
    begins.then_some(Opening {
        checks,
        redos: redos_due,
    })
}

fn last_learner_ms(root: &Path) -> Option<Option<i64>> {
    let path = root.join(".magnesium/sittings.sqlite");
    if !path.exists() {
        return Some(None);
    }
    let conn = open(&path)?;
    match conn.query_row("SELECT last_learner_ms FROM meta", [], |row| {
        row.get::<_, Option<i64>>(0)
    }) {
        Ok(last) => Some(last),
        Err(rusqlite::Error::QueryReturnedNoRows) => Some(None),
        Err(_) => None,
    }
}

fn opener_off(root: &Path) -> bool {
    let Ok(text) = fs::read_to_string(root.join(".magnesium/config.toml")) else {
        return false;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            break;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() == "opener" {
            let value = value.split('#').next().unwrap_or("").trim();
            return value.trim_matches(|c| c == '"' || c == '\'') == "off";
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_next_message_opens_with_what_is_due_only_when_it_begins_a_sitting() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let now = 10 * SITTING_GAP_MS;
        assert_eq!(
            opening(root, 5, 0, now),
            Some(Opening {
                checks: 3,
                redos: 0
            })
        );
        assert_eq!(opening(root, 0, 0, now), None);
        let control = root.join(".magnesium");
        std::fs::create_dir_all(&control).unwrap();
        let conn = Connection::open(control.join("sittings.sqlite")).unwrap();
        conn.execute_batch("CREATE TABLE meta(singleton INTEGER PRIMARY KEY, version INTEGER, last_learner_ms INTEGER);")
            .unwrap();
        assert_eq!(
            opening(root, 1, 2, now),
            Some(Opening {
                checks: 1,
                redos: 2
            })
        );
        conn.execute(
            "INSERT INTO meta VALUES(1, 2, ?1)",
            [now - SITTING_GAP_MS / 3],
        )
        .unwrap();
        assert_eq!(opening(root, 1, 2, now), None);
        conn.execute("UPDATE meta SET last_learner_ms=?1", [now - SITTING_GAP_MS])
            .unwrap();
        assert_eq!(
            opening(root, 1, 0, now),
            Some(Opening {
                checks: 1,
                redos: 0
            })
        );
        std::fs::write(control.join("config.toml"), "opener = \"off\"\n").unwrap();
        assert_eq!(opening(root, 1, 0, now), None);
        std::fs::write(
            control.join("config.toml"),
            "opener = \"block\"\n[other]\nopener = \"off\"\n",
        )
        .unwrap();
        assert_eq!(
            opening(root, 1, 0, now),
            Some(Opening {
                checks: 1,
                redos: 0
            })
        );
    }

    #[test]
    fn a_missing_store_gives_nothing_and_creates_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(checks(dir.path()).map(|c| c.len()), Some(0));
        assert!(!dir.path().join(".magnesium").exists());
    }

    #[test]
    fn due_checks_are_counted() {
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
    }

    #[test]
    fn a_conversation_gets_its_own_lessons_goal_not_the_folders_last() {
        let dir = tempfile::tempdir().unwrap();
        let control = dir.path().join(".magnesium");
        std::fs::create_dir_all(&control).unwrap();
        assert_eq!(goal_of_conversation(dir.path(), "window-1"), None);
        let conn = Connection::open(control.join("state.sqlite")).unwrap();
        conn.execute_batch(
            "CREATE TABLE learning_sessions(session_id TEXT PRIMARY KEY, goal TEXT, updated_at_ms INTEGER);
             CREATE TABLE host_bindings(host_kind TEXT, host_thread_id TEXT, session_id TEXT, PRIMARY KEY(host_kind, host_thread_id));
             INSERT INTO learning_sessions VALUES('s-window', 'Continue', 1);
             INSERT INTO learning_sessions VALUES('s-terminal', 'Hypothesis tests from the terminal', 2);
             INSERT INTO host_bindings VALUES('claude', 'magnesium-h1:claude:window-1', 's-window');
             INSERT INTO host_bindings VALUES('claude', 'magnesium-h1:claude:terminal-7', 's-terminal');",
        )
        .unwrap();
        drop(conn);
        assert_eq!(
            goal_of_conversation(dir.path(), "window-1").as_deref(),
            Some("Continue")
        );
        assert_eq!(
            goal_of_conversation(dir.path(), "terminal-7").as_deref(),
            Some("Hypothesis tests from the terminal")
        );
        assert_eq!(goal_of_conversation(dir.path(), "window-2"), None);
        assert_eq!(goal_of_conversation(dir.path(), "1"), None);
        assert_eq!(goal_of_conversation(dir.path(), ""), None);
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

    #[test]
    fn the_sitting_rule_is_the_runtimes_own() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../src/sittings.rs");
        let text = std::fs::read_to_string(&source).unwrap_or_else(|problem| {
            panic!(
                "the runtime's {} could not be read: {problem}",
                source.display()
            )
        });
        let constant = |name: &str| -> i64 {
            let line = text
                .lines()
                .find(|line| line.contains(&format!("const {name}:")))
                .unwrap_or_else(|| panic!("the runtime has no constant named {name} now"));
            let value = line
                .split('=')
                .nth(1)
                .unwrap_or_else(|| panic!("no value on: {line}"))
                .trim()
                .trim_end_matches(';');
            value
                .split('*')
                .map(|part| {
                    part.trim()
                        .replace('_', "")
                        .parse::<i64>()
                        .unwrap_or_else(|_| {
                            panic!("{name} is no longer a product of whole numbers: {line}")
                        })
                })
                .product()
        };
        assert_eq!(
            constant("SITTING_GAP_MS"),
            SITTING_GAP_MS,
            "the gap that begins a sitting"
        );
        assert_eq!(
            constant("MAX_CHECKS_PER_SITTING"),
            i64::from(CHECKS_PER_SITTING),
            "the checks one sitting takes"
        );
        assert!(
            text.contains("last_learner_ms INTEGER"),
            "the runtime's meta table has no last_learner_ms column now"
        );
        assert!(
            text.contains("SELECT last_learner_ms FROM meta"),
            "the runtime no longer reads last_learner_ms from meta"
        );
    }
}
