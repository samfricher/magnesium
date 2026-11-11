use super::*;
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
pub const SCHEMA_VERSION: i64 = 5;
pub(crate) const SCHEMA_SQL: &str = include_str!("schema.sql");
/// The tables schema 3 added. A fresh store and the upgrade from schema 2
/// both run it.
pub(crate) const SCHEMA_V3_SQL: &str = include_str!("schema_v3.sql");
/// The tables and columns schema 4 added. A fresh store and the upgrade from
/// schema 2 or 3 run it.
pub(crate) const SCHEMA_V4_SQL: &str = include_str!("schema_v4.sql");
/// The tables and columns schema 5 added. A fresh store and every upgrade
/// run it.
pub(crate) const SCHEMA_V5_SQL: &str = include_str!("schema_v5.sql");
/// The earlier schemas this runtime upgrades in place. Schema 2 is the last
/// release's; schemas 3 and 4 were only ever on the main branch.
const UPGRADEABLE_SCHEMA_VERSIONS: [i64; 3] = [2, 3, 4];
/// Beside the store: the copy made before an upgrade, kept as the way back.
pub const BACKUP_DIR: &str = "backups";
pub struct LearnerStore {
    pub conn: Connection,
    pub workspace_id: String,
}
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
pub fn new_id(prefix: &str) -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes)
        .map_err(|e| LearnerError::new("storage_error", e.to_string()))?;
    Ok(format!(
        "{prefix}-{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ))
}
/// Reserve runtime-issued identity shapes from new request-local aliases only.
/// This neither normalizes references nor decides whether a stored ID exists.
pub(crate) fn resembles_persistent_id(name: &str) -> bool {
    let Some((prefix, suffix)) = name.split_once('-') else {
        return false;
    };
    matches!(prefix, "e" | "o" | "m" | "s" | "r")
        && suffix.len() == 32
        && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
}
impl LearnerStore {
    pub fn initialize(path: &Path, workspace_id: &str) -> Result<Self> {
        if path.exists() {
            return Self::open(path, workspace_id);
        }
        reject_symlinks(path)?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(path) {
            Ok(file) => drop(file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Self::open(path, workspace_id);
            }
            Err(error) => return Err(LearnerError::new("storage_error", error.to_string())),
        }
        Self::open(path, workspace_id)
    }
    pub fn open(path: &Path, workspace_id: &str) -> Result<Self> {
        reject_symlinks(path)?;
        let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        // An empty file is a store whose first transaction never committed:
        // this process is creating it, another one is, or one stopped
        // part-way. Make it under the write lock unless another has by then.
        let length = std::fs::metadata(path)
            .map_err(|e| LearnerError::new("storage_error", e.to_string()))?
            .len();
        if length == 0 {
            configure(&conn)?;
            create_schema(&conn, workspace_id)?;
        }
        let meta = read_meta(&conn)?;
        if meta.1 != SCHEMA
            || (meta.2 != SCHEMA_VERSION && !UPGRADEABLE_SCHEMA_VERSIONS.contains(&meta.2))
        {
            return Err(LearnerError::new(
                "incompatible_store",
                "Unsupported storage family/version. Preserve the existing store and initialize a fresh workspace; this runtime upgrades only schemas 2, 3 and 4.",
            ));
        }
        if meta.0 != workspace_id {
            return Err(LearnerError::new(
                "workspace_mismatch",
                "Workspace identity differs",
            ));
        }
        configure(&conn)?;
        if meta.2 != SCHEMA_VERSION {
            upgrade(path, &conn)?;
        }
        Ok(Self {
            conn,
            workspace_id: workspace_id.into(),
        })
    }
    pub fn connection(&self) -> &Connection {
        &self.conn
    }
    pub fn binding(&self, host: &HostContext) -> Result<(Option<String>, bool, bool)> {
        Ok(self.conn.query_row("SELECT session_id,learning_active,recording_enabled FROM host_bindings WHERE host_kind=?1 AND host_thread_id=?2",params![host.host_kind,host.host_thread_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?.unwrap_or((None,false,true)))
    }
}
/// Whether `open` would upgrade this store from an earlier schema. Read-only:
/// it changes nothing and makes no copy, so a caller can ask first.
pub fn needs_upgrade(path: &Path) -> Result<bool> {
    reject_symlinks(path)?;
    // A missing or empty store is not an earlier version's. `open` reports
    // or repairs it.
    match std::fs::metadata(path) {
        Ok(meta) if meta.len() > 0 => {}
        Ok(_) => return Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(LearnerError::new("storage_error", error.to_string())),
    }
    let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.busy_timeout(Duration::from_secs(2))?;
    let version = read_meta(&conn)?.2;
    Ok(version != SCHEMA_VERSION && UPGRADEABLE_SCHEMA_VERSIONS.contains(&version))
}

fn create_schema(conn: &Connection, workspace_id: &str) -> Result<()> {
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
    let empty: bool = tx.query_row("SELECT count(*) = 0 FROM sqlite_master", [], |r| r.get(0))?;
    if empty {
        tx.execute_batch(SCHEMA_SQL)?;
        tx.execute_batch(SCHEMA_V3_SQL)?;
        tx.execute_batch(SCHEMA_V4_SQL)?;
        tx.execute_batch(SCHEMA_V5_SQL)?;
        tx.execute("INSERT INTO workspace_meta(singleton,workspace_id,storage_family,schema_version,created_at_ms) VALUES(1,?1,?2,?3,?4)",params![workspace_id,SCHEMA,SCHEMA_VERSION,now_ms()])?;
    }
    tx.commit()?;
    Ok(())
}
type Meta = (String, String, i64);
fn read_meta(conn: &Connection) -> Result<Meta> {
    conn.query_row(
        "SELECT workspace_id,storage_family,schema_version FROM workspace_meta WHERE singleton=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .map_err(|_| {
        LearnerError::new(
            "incompatible_store",
            "Expected a learner-model/v2 store. Preserve the existing store and initialize a fresh workspace.",
        )
    })
}

/// Upgrade a schema 2, 3 or 4 store in place, once. Under the write lock,
/// copy it to `backups/` beside it, then add the tables of each later schema,
/// read the help levels and readings the memories state in words, and raise
/// the version in one transaction. The copy is a whole store of the
/// old version, so the runtime that made it still opens it: put it back as
/// `state.sqlite` to go back. If any step fails, the store is left as it was.
fn upgrade(path: &Path, conn: &Connection) -> Result<()> {
    // Another process may be upgrading too. Copying takes a while for a large
    // store, so wait longer for the lock than an ordinary write does.
    conn.busy_timeout(Duration::from_secs(30))?;
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| -> Result<()> {
        let version: i64 = conn.query_row(
            "SELECT schema_version FROM workspace_meta WHERE singleton=1",
            [],
            |r| r.get(0),
        )?;
        if version == SCHEMA_VERSION {
            return Ok(());
        }
        backup_before_upgrade(path, version)?;
        conn.execute_batch(UPGRADE_META_SQL)?;
        if version == 2 {
            conn.execute_batch(SCHEMA_V3_SQL)?;
        }
        if version <= 3 {
            conn.execute_batch(SCHEMA_V4_SQL)?;
        }
        conn.execute_batch(SCHEMA_V5_SQL)?;
        crate::learner::help::import_phrases(conn, now_ms())?;
        conn.execute(
            "UPDATE workspace_meta SET schema_version=?1 WHERE singleton=1",
            [SCHEMA_VERSION],
        )?;
        Ok(())
    })();
    match result {
        Ok(()) => conn.execute_batch("COMMIT")?,
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(error);
        }
    }
    conn.busy_timeout(Duration::from_secs(2))?;
    Ok(())
}

/// The version check lives in a table constraint, which SQLite cannot alter,
/// so the one-row table is rebuilt with the same columns and data.
const UPGRADE_META_SQL: &str = "
CREATE TABLE workspace_meta_next (
  singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
  workspace_id TEXT NOT NULL UNIQUE,
  storage_family TEXT NOT NULL CHECK (storage_family = 'learner-model/v2'),
  schema_version INTEGER NOT NULL CHECK (schema_version = 5),
  memory_generation INTEGER NOT NULL DEFAULT 0,
  observation_generation INTEGER NOT NULL DEFAULT 0,
  event_generation INTEGER NOT NULL DEFAULT 0,
  review_generation INTEGER NOT NULL DEFAULT 0,
  memory_fts_generation INTEGER NOT NULL DEFAULT 0,
  observation_fts_generation INTEGER NOT NULL DEFAULT 0,
  created_at_ms INTEGER NOT NULL
);
INSERT INTO workspace_meta_next
  SELECT singleton,workspace_id,storage_family,5,memory_generation,
         observation_generation,event_generation,review_generation,
         memory_fts_generation,observation_fts_generation,created_at_ms
  FROM workspace_meta;
DROP TABLE workspace_meta;
ALTER TABLE workspace_meta_next RENAME TO workspace_meta;
";

/// Copy the store, as it is under the caller's write lock, to a new private
/// file in `backups/`, and check the copy before anything changes.
fn backup_before_upgrade(path: &Path, version: i64) -> Result<PathBuf> {
    let storage = |e: std::io::Error| LearnerError::new("storage_error", e.to_string());
    let directory = path
        .parent()
        .ok_or_else(|| LearnerError::new("storage_error", "store has no folder"))?
        .join(BACKUP_DIR);
    match std::fs::symlink_metadata(&directory) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
        Ok(_) => {
            return Err(LearnerError::new(
                "workspace_mismatch",
                "The store's backups path must be a real folder",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            crate::workspace::create_private_dir(&directory).map_err(storage)?;
        }
        Err(error) => return Err(storage(error)),
    }
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let backup = directory.join(format!(
        "state-schema-{version}-{stamp}-{}.sqlite",
        new_id("b")?
            .trim_start_matches("b-")
            .get(..8)
            .unwrap_or("0")
    ));
    // VACUUM INTO writes only into a missing or empty file. Make it empty and
    // private first, so the copy is never readable by others.
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    drop(options.open(&backup).map_err(storage)?);
    crate::workspace::restrict_private_path(&backup).map_err(storage)?;
    // A second connection reads while the caller holds the write lock, so the
    // copy is the store exactly as the upgrade finds it.
    let reader = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    reader.busy_timeout(Duration::from_secs(30))?;
    reader.execute(
        "VACUUM INTO ?1",
        [backup
            .to_str()
            .ok_or_else(|| LearnerError::new("storage_error", "backup path is not UTF-8"))?],
    )?;
    drop(reader);
    let copy = Connection::open_with_flags(&backup, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let check: String = copy.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    let meta = read_meta(&copy)?;
    if check != "ok" || meta.2 != version {
        return Err(LearnerError::new(
            "storage_error",
            format!("the copy made before upgrading failed its check: {check}"),
        ));
    }
    Ok(backup)
}

fn configure(conn: &Connection) -> Result<()> {
    conn.busy_timeout(Duration::from_secs(2))?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;
    crate::workspace::apply_sync_policy(conn)?;
    Ok(())
}

fn reject_symlinks(path: &Path) -> Result<()> {
    for candidate in [
        path.to_path_buf(),
        std::path::PathBuf::from(format!("{}-wal", path.display())),
        std::path::PathBuf::from(format!("{}-shm", path.display())),
        std::path::PathBuf::from(format!("{}-journal", path.display())),
    ] {
        if std::fs::symlink_metadata(&candidate).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(LearnerError::new(
                "workspace_mismatch",
                "Symlink database/sidecar is not allowed",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod schema_revision_tests {
    use super::*;

    #[test]
    fn prior_schema_is_rejected_without_migration_or_data_loss() {
        let dir = std::env::temp_dir().join(new_id("prior-schema-test").unwrap());
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("old.sqlite");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE workspace_meta(singleton INTEGER PRIMARY KEY, workspace_id TEXT, storage_family TEXT, schema_version INTEGER); INSERT INTO workspace_meta VALUES(1,'old-workspace','learner-model/v2',1); CREATE TABLE preserved(value TEXT); INSERT INTO preserved VALUES('original learner work');").unwrap();
        drop(conn);
        let before = std::fs::read(&path).unwrap();
        let error = LearnerStore::open(&path, "old-workspace").err().unwrap();
        assert_eq!(error.code, "incompatible_store");
        assert_eq!(std::fs::read(&path).unwrap(), before);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_empty_store_file_left_by_an_interrupted_initialize_is_initialized() {
        let dir = std::env::temp_dir().join(new_id("empty-store-test").unwrap());
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("state.sqlite");
        let workspace = |store: &LearnerStore| -> String {
            store
                .conn
                .query_row("SELECT workspace_id FROM workspace_meta", [], |r| r.get(0))
                .unwrap()
        };
        // The way back in through init opens the file it finds.
        std::fs::File::create(&path).unwrap();
        let opened = LearnerStore::open(&path, "empty-store").unwrap();
        assert_eq!(workspace(&opened), "empty-store");
        opened
            .conn
            .execute_batch("INSERT INTO operation_receipts VALUES('kept','h','{}',0)")
            .unwrap();
        drop(opened);
        std::fs::write(dir.join("other.sqlite"), b"").unwrap();
        let initialized = LearnerStore::initialize(&dir.join("other.sqlite"), "other").unwrap();
        assert_eq!(workspace(&initialized), "other");
        // A store holding data is opened as it is, never made again.
        for store in [
            LearnerStore::initialize(&path, "empty-store").unwrap(),
            LearnerStore::open(&path, "empty-store").unwrap(),
        ] {
            let kept: i64 = store
                .conn
                .query_row(
                    "SELECT count(*) FROM operation_receipts WHERE request_id='kept'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(kept, 1);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod upgrade_tests;
