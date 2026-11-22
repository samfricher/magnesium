use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior};

use crate::workspace::Workspace;

pub(crate) const STORE_FILE: &str = "map.sqlite";
const STORE_VERSION: i64 = 1;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS meta(
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    version INTEGER NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE IF NOT EXISTS files(
    code TEXT PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    content_sha256 TEXT,
    anchor_kind TEXT CHECK (anchor_kind IS NULL OR anchor_kind IN ('line', 'page')),
    readable INTEGER NOT NULL CHECK (readable IN (0, 1)),
    added_ms INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS items(
    id TEXT PRIMARY KEY,
    file TEXT REFERENCES files(code) ON DELETE CASCADE,
    parent TEXT REFERENCES items(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('section', 'idea', 'exercise', 'question', 'example', 'reading', 'foundation', 'task')),
    origin TEXT NOT NULL CHECK (origin IN ('extracted', 'read', 'outline', 'written', 'lesson')),
    title TEXT NOT NULL CHECK (length(trim(title)) > 0),
    ref TEXT,
    note TEXT,
    anchor_start INTEGER CHECK (anchor_start IS NULL OR anchor_start >= 1),
    anchor_end INTEGER CHECK (anchor_end IS NULL OR anchor_end >= anchor_start),
    position INTEGER NOT NULL,
    content_sha256 TEXT,
    added_ms INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS items_by_file ON items(file, position);
CREATE INDEX IF NOT EXISTS items_by_parent ON items(parent, position);
CREATE TABLE IF NOT EXISTS uses(
    item TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    uses TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    origin TEXT NOT NULL CHECK (origin IN ('plan', 'task')),
    PRIMARY KEY(item, uses),
    CHECK (item <> uses)
) STRICT;
CREATE INDEX IF NOT EXISTS uses_backwards ON uses(uses);
CREATE TABLE IF NOT EXISTS targets(
    slug TEXT PRIMARY KEY,
    label TEXT NOT NULL CHECK (length(trim(label)) > 0),
    at TEXT,
    hours REAL CHECK (hours IS NULL OR hours >= 0),
    length_minutes INTEGER CHECK (length_minutes IS NULL OR length_minutes >= 0),
    goal TEXT,
    aids TEXT,
    words TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(words) AND json_type(words) = 'array'),
    sessions TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(sessions) AND json_type(sessions) = 'array'),
    updated_ms INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS scope(
    target TEXT NOT NULL REFERENCES targets(slug) ON DELETE CASCADE,
    ref TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('in', 'cut')),
    position INTEGER NOT NULL,
    reason TEXT,
    confirmed INTEGER NOT NULL DEFAULT 0 CHECK (confirmed IN (0, 1)),
    PRIMARY KEY(target, ref),
    CHECK (state = 'in' OR (reason IS NOT NULL AND length(trim(reason)) > 0))
) STRICT;
CREATE TABLE IF NOT EXISTS blocks(
    target TEXT NOT NULL REFERENCES targets(slug) ON DELETE CASCADE,
    n INTEGER NOT NULL CHECK (n >= 1),
    title TEXT NOT NULL CHECK (length(trim(title)) > 0),
    sections TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(sections) AND json_type(sections) = 'array'),
    tasks TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(tasks) AND json_type(tasks) = 'array'),
    minutes INTEGER CHECK (minutes IS NULL OR minutes >= 0),
    opens_with TEXT,
    note TEXT,
    done TEXT,
    spent INTEGER CHECK (spent IS NULL OR spent >= 0),
    PRIMARY KEY(target, n)
) STRICT;
CREATE TABLE IF NOT EXISTS carried(
    target TEXT NOT NULL REFERENCES targets(slug) ON DELETE CASCADE,
    idea TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    by_item TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    reserve TEXT REFERENCES items(id) ON DELETE SET NULL,
    PRIMARY KEY(target, idea),
    CHECK (idea <> by_item)
) STRICT;
CREATE TABLE IF NOT EXISTS traces(
    question TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    source TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    changed TEXT NOT NULL,
    PRIMARY KEY(question, source),
    CHECK (question <> source)
) STRICT;
CREATE TABLE IF NOT EXISTS retired(
    id TEXT PRIMARY KEY
) STRICT;
CREATE TABLE IF NOT EXISTS pages(
    path TEXT PRIMARY KEY,
    sha256 TEXT NOT NULL
) STRICT;
"#;

pub(crate) struct MapStore {
    pub(crate) conn: Connection,
}

fn error(error: impl std::fmt::Display) -> String {
    format!("course map: {error}")
}

pub(crate) fn store_path(workspace: &Workspace) -> PathBuf {
    workspace.control().join(STORE_FILE)
}

fn refuse_links(path: &Path) -> Result<(), String> {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let candidate = PathBuf::from(format!("{}{suffix}", path.display()));
        if std::fs::symlink_metadata(&candidate).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(error("a symlinked course map store is not allowed"));
        }
    }
    Ok(())
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

impl MapStore {
    pub(crate) fn open(workspace: &Workspace) -> Result<Self, String> {
        let path = store_path(workspace);
        crate::workspace::reject_symlink_components(&path, workspace.control()).map_err(error)?;
        refuse_links(&path)?;
        create_private(&path)?;
        let conn =
            Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_WRITE).map_err(error)?;
        Self::prepare(conn)
    }

    pub(crate) fn open_existing(workspace: &Workspace) -> Result<Option<Self>, String> {
        let path = store_path(workspace);
        crate::workspace::reject_symlink_components(&path, workspace.control()).map_err(error)?;
        refuse_links(&path)?;
        if !path.try_exists().map_err(error)? {
            return Ok(None);
        }
        let conn =
            Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_WRITE).map_err(error)?;
        Self::prepare(conn).map(Some)
    }

    #[cfg(test)]
    pub(crate) fn in_memory() -> Result<Self, String> {
        Self::prepare(Connection::open_in_memory().map_err(error)?)
    }

    fn prepare(conn: Connection) -> Result<Self, String> {
        conn.busy_timeout(Duration::from_secs(5)).map_err(error)?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(error)?;
        crate::workspace::apply_sync_policy(&conn).map_err(error)?;
        conn.execute_batch(SCHEMA).map_err(error)?;
        let has_ref: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('items') WHERE name = 'ref')",
                [],
                |r| r.get(0),
            )
            .map_err(error)?;
        if !has_ref {
            conn.execute_batch("ALTER TABLE items ADD COLUMN ref TEXT;")
                .map_err(error)?;
        }
        conn.execute(
            "INSERT OR IGNORE INTO meta(singleton, version, revision) VALUES (1, ?1, 0)",
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
        Ok(Self { conn })
    }

    pub(crate) fn write(&mut self) -> Result<Transaction<'_>, String> {
        self.conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(error)
    }

    pub(crate) fn page_hash(&self, path: &str) -> Result<Option<String>, String> {
        self.conn
            .query_row("SELECT sha256 FROM pages WHERE path = ?1", [path], |r| {
                r.get(0)
            })
            .optional()
            .map_err(error)
    }

    pub(crate) fn remember_page(&self, path: &str, sha: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO pages(path, sha256) VALUES (?1, ?2)
                 ON CONFLICT(path) DO UPDATE SET sha256 = excluded.sha256",
                [path, sha],
            )
            .map(|_| ())
            .map_err(error)
    }
}

pub(crate) fn bump(tx: &Transaction<'_>) -> Result<i64, String> {
    tx.execute("UPDATE meta SET revision = revision + 1", [])
        .map_err(error)?;
    tx.query_row("SELECT revision FROM meta", [], |r| r.get(0))
        .map_err(error)
}

pub(crate) fn fail(e: impl std::fmt::Display) -> String {
    error(e)
}
