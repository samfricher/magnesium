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
        let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        configure(&conn)?;
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(crate::learner::store::SCHEMA_SQL)?;
        tx.execute_batch(SCHEMA_V3_SQL)?;
        tx.execute_batch(SCHEMA_V4_SQL)?;
        tx.execute_batch(SCHEMA_V5_SQL)?;
        tx.execute("INSERT INTO workspace_meta(singleton,workspace_id,storage_family,schema_version,created_at_ms) VALUES(1,?1,?2,?3,?4)",params![workspace_id,SCHEMA,SCHEMA_VERSION,now_ms()])?;
        tx.commit()?;
        Ok(Self {
            conn,
            workspace_id: workspace_id.into(),
        })
    }
    pub fn open(path: &Path, workspace_id: &str) -> Result<Self> {
        reject_symlinks(path)?;
        let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
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
}

#[cfg(test)]
mod upgrade_tests {
    use super::*;
    use std::collections::BTreeMap;

    /// The schema 2 SQL exactly as the previous release shipped it.
    const SCHEMA_2: &str = include_str!("../../tests/fixtures/learner_model/schema-2.sql");
    /// The schema 3 SQL exactly as the main branch had it before schema 4.
    const SCHEMA_3: &str = include_str!("../../tests/fixtures/learner_model/schema-3.sql");
    const WORKSPACE: &str = "upgrade-test";

    fn folder() -> PathBuf {
        let dir = std::env::temp_dir().join(new_id("schema-upgrade-test").unwrap());
        std::fs::create_dir(&dir).unwrap();
        dir
    }

    /// A store the previous runtime would have made, with a lesson in it.
    fn schema_2_store(dir: &Path) -> PathBuf {
        let path = dir.join("state.sqlite");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(SCHEMA_2).unwrap();
        conn.execute_batch(
            "INSERT INTO workspace_meta(singleton,workspace_id,storage_family,schema_version,memory_generation,observation_generation,event_generation,created_at_ms)
               VALUES(1,'upgrade-test','learner-model/v2',2,3,2,5,1000);
             INSERT INTO learning_sessions VALUES('s-1','Quadratic formula','active','{\"current_problem\":\"b\",\"working_diagnosis\":\"sign of b\",\"next_action\":\"check b\",\"plan_text\":null,\"memory_refs\":[],\"pending_task\":null}',2,1000,2000);
             INSERT INTO host_bindings VALUES('claude','conv','s-1',1,1);
             INSERT INTO learning_events(event_id,session_id,host_kind,host_thread_id,external_event_key,speaker,kind,origin,text,metadata_json,occurred_at_ms,captured_at_ms,processing)
               VALUES('e-1','s-1','claude','conv','p-1','learner','message','host_observed','b is 7','{}',1500,1500,'processed');
             INSERT INTO observations(observation_id,session_id,kind,description,conditions_text,assistance_text,source_refs_json,observed_at_ms,created_at_ms)
               VALUES('o-1','s-1','learner_work','Took b as 7, dropping its sign.','One equation.','none','[]',1500,1600);
             INSERT INTO observation_events VALUES('o-1','e-1','learner_response','[]');
             INSERT INTO learner_memories(memory_id,title,scope,summary,state_text,current_basis_json,lifecycle,revision,created_at_ms,updated_at_ms)
               VALUES('m-1','Sign of b','Quadratics','Drops the sign of b.','open','[{\"observation_id\":\"o-1\"}]','active',1,1600,1600);
             INSERT INTO memory_topics VALUES('m-1','quadratic formula');
             INSERT INTO memory_observations VALUES('m-1','o-1');
             INSERT INTO memory_changes VALUES('m-1',1,'new_observation','{}','first','[]',1600);
             INSERT INTO review_items VALUES('r-1','m-1',1,'recheck','sign',NULL,'suggested',1,1600);",
        )
        .unwrap();
        path
    }

    const OLD_TABLES: [&str; 14] = [
        "workspace_meta",
        "learning_sessions",
        "host_bindings",
        "learning_events",
        "observations",
        "observation_events",
        "learner_memories",
        "memory_topics",
        "memory_observations",
        "memory_changes",
        "operation_receipts",
        "review_items",
        "context_receipts",
        "memory_fts",
    ];

    fn rows(conn: &Connection, table: &str) -> Vec<Vec<String>> {
        let mut statement = conn.prepare(&format!("SELECT * FROM {table}")).unwrap();
        let width = statement.column_count();
        statement
            .query_map([], |r| {
                (0..width)
                    .map(|i| {
                        Ok(match r.get_ref(i)? {
                            rusqlite::types::ValueRef::Null => "NULL".to_owned(),
                            rusqlite::types::ValueRef::Integer(v) => v.to_string(),
                            rusqlite::types::ValueRef::Real(v) => v.to_string(),
                            rusqlite::types::ValueRef::Text(v) => {
                                String::from_utf8_lossy(v).into_owned()
                            }
                            rusqlite::types::ValueRef::Blob(v) => format!("{v:?}"),
                        })
                    })
                    .collect()
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    /// Every table and index, with its SQL minus spacing and quotes.
    fn shape(conn: &Connection) -> BTreeMap<String, String> {
        let mut statement = conn
            .prepare("SELECT name,sql FROM sqlite_master WHERE sql IS NOT NULL")
            .unwrap();
        statement
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .unwrap()
            .map(|row| {
                let (name, sql) = row.unwrap();
                (
                    name,
                    sql.chars()
                        .filter(|c| !c.is_whitespace() && *c != '"')
                        .collect(),
                )
            })
            .collect()
    }

    fn backups(dir: &Path) -> Vec<PathBuf> {
        match std::fs::read_dir(dir.join(BACKUP_DIR)) {
            Ok(entries) => entries.map(|e| e.unwrap().path()).collect(),
            Err(_) => Vec::new(),
        }
    }

    #[test]
    fn a_schema_2_store_upgrades_in_place_with_its_history_and_keeps_a_schema_2_copy() {
        let dir = folder();
        let path = schema_2_store(&dir);
        let before: Vec<_> = {
            let conn = Connection::open(&path).unwrap();
            OLD_TABLES.iter().map(|t| rows(&conn, t)).collect()
        };

        let store = LearnerStore::open(&path, WORKSPACE).unwrap();
        let version: i64 = store
            .conn
            .query_row("SELECT schema_version FROM workspace_meta", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(version, 5);
        // The history is intact: every old row is still there, unchanged but
        // for the version number.
        for (table, old) in OLD_TABLES.iter().zip(&before) {
            let mut old = old.clone();
            if *table == "workspace_meta" {
                old[0][3] = "5".into();
            }
            assert_eq!(&rows(&store.conn, table), &old, "{table}");
        }
        let attempts: i64 = store
            .conn
            .query_row("SELECT count(*) FROM attempts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(attempts, 0);

        // The copy is the schema 2 store as it was, which the previous runtime
        // opens: same family, version 2, same workspace, same rows.
        let copies = backups(&dir);
        assert_eq!(copies.len(), 1, "{copies:?}");
        let copy = Connection::open(&copies[0]).unwrap();
        assert_eq!(
            read_meta(&copy).unwrap(),
            (WORKSPACE.to_owned(), SCHEMA.to_owned(), 2)
        );
        for (table, old) in OLD_TABLES.iter().zip(&before) {
            assert_eq!(&rows(&copy, table), old, "{table}");
        }
        let fresh_schema_2 = Connection::open_in_memory().unwrap();
        fresh_schema_2.execute_batch(SCHEMA_2).unwrap();
        assert_eq!(shape(&copy), shape(&fresh_schema_2));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&copies[0]).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "the copy is private: {mode:o}");
        }

        // Opening it again changes nothing and makes no second copy.
        drop(store);
        LearnerStore::open(&path, WORKSPACE).unwrap();
        assert_eq!(backups(&dir).len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_upgraded_store_has_exactly_the_tables_of_a_fresh_one() {
        let dir = folder();
        let upgraded = LearnerStore::open(&schema_2_store(&dir), WORKSPACE).unwrap();
        let fresh = LearnerStore::initialize(&dir.join("fresh.sqlite"), WORKSPACE).unwrap();
        assert_eq!(shape(&upgraded.conn), shape(&fresh.conn));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A store the main branch made at schema 3, with a lesson and one
    /// attempt in it.
    fn schema_3_store(dir: &Path) -> PathBuf {
        let path = dir.join("state.sqlite");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(SCHEMA_3).unwrap();
        conn.execute_batch(
            "INSERT INTO workspace_meta(singleton,workspace_id,storage_family,schema_version,created_at_ms)
               VALUES(1,'upgrade-test','learner-model/v2',3,1000);
             INSERT INTO learning_sessions VALUES('s-1','Quadratic formula','active','{}',1,1000,2000);
             INSERT INTO learning_events(event_id,session_id,host_kind,host_thread_id,external_event_key,speaker,kind,origin,text,metadata_json,occurred_at_ms,captured_at_ms,processing)
               VALUES('e-1','s-1','claude','conv','p-1','learner','message','host_observed','b is 7','{}',1500,1500,'processed');
             INSERT INTO observations(observation_id,session_id,kind,description,conditions_text,assistance_text,source_refs_json,observed_at_ms,created_at_ms)
               VALUES('o-1','s-1','learner_work','Took b as 7.','','none','[]',1500,1600);
             INSERT INTO attempts(attempt_id,observation_id,response_event_id,session_id,question,label,label_key,mark,help,cold,answered_at_ms,created_at_ms)
               VALUES('a-1','o-1','e-1','s-1','b in 2x²−7x+3','sign of b','sign of b','wrong','none',0,1500,1600);",
        )
        .unwrap();
        path
    }

    #[test]
    fn a_schema_3_store_upgrades_to_5_with_its_attempts_and_keeps_a_schema_3_copy() {
        let dir = folder();
        let path = schema_3_store(&dir);
        let before = {
            let conn = Connection::open(&path).unwrap();
            rows(&conn, "attempts")
        };
        let store = LearnerStore::open(&path, WORKSPACE).unwrap();
        let version: i64 = store
            .conn
            .query_row("SELECT schema_version FROM workspace_meta", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(version, 5);
        // Each attempt keeps its row and gains slow, kind_key and no_start:
        // false, none and false.
        let after = rows(&store.conn, "attempts");
        assert_eq!(after.len(), 1);
        assert_eq!(after[0][..before[0].len()], before[0][..]);
        assert_eq!(after[0][before[0].len()..], ["0", "NULL", "0"]);
        let copies = backups(&dir);
        assert_eq!(copies.len(), 1, "{copies:?}");
        assert!(
            copies[0]
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("state-schema-3-")
        );
        let copy = Connection::open(&copies[0]).unwrap();
        assert_eq!(read_meta(&copy).unwrap().2, 3);
        let fresh_schema_3 = Connection::open_in_memory().unwrap();
        fresh_schema_3.execute_batch(SCHEMA_3).unwrap();
        assert_eq!(shape(&copy), shape(&fresh_schema_3));
        let fresh = LearnerStore::initialize(&dir.join("fresh.sqlite"), WORKSPACE).unwrap();
        assert_eq!(shape(&store.conn), shape(&fresh.conn));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The schema 4 SQL exactly as the main branch had it before schema 5.
    const SCHEMA_4: &str = include_str!("../../tests/fixtures/learner_model/schema-4.sql");

    /// A store the main branch made at schema 4, whose memories state help
    /// levels and readings in words, as the tutor used to write them.
    fn schema_4_store(dir: &Path) -> PathBuf {
        let path = dir.join("state.sqlite");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(SCHEMA_4).unwrap();
        conn.execute_batch(
            "INSERT INTO workspace_meta(singleton,workspace_id,storage_family,schema_version,created_at_ms)
               VALUES(1,'upgrade-test','learner-model/v2',4,1000);
             INSERT INTO learning_sessions VALUES('s-1','Probability','active','{}',1,1000,2000);
             INSERT INTO learning_events(event_id,session_id,host_kind,host_thread_id,external_event_key,speaker,kind,origin,text,metadata_json,occurred_at_ms,captured_at_ms,processing)
               VALUES('e-1','s-1','claude','conv','p-1','learner','message','host_observed','no idea','{}',1500,1500,'processed');
             INSERT INTO observations(observation_id,session_id,kind,description,conditions_text,assistance_text,source_refs_json,observed_at_ms,created_at_ms)
               VALUES('o-1','s-1','learner_work','No start.','','none','[]',1500,1600);
             INSERT INTO learner_memories(memory_id,title,scope,summary,state_text,current_basis_json,lifecycle,revision,created_at_ms,updated_at_ms)
               VALUES('m-1','Expected value','Probability','Needs a start.','Applies E = sum x P(x) after help; example step (Discrete expected value): full','[{\"observation_id\":\"o-1\"}]','active',1,1600,1600),
                     ('m-2','Expected value again','Probability','Merged.','example step (discrete  expected value): partly worked','[{\"observation_id\":\"o-1\"}]','active',1,1600,1600),
                     ('m-3','Chain rule','Calculus','Sound.','sound from MockA Q3 part b','[{\"observation_id\":\"o-1\"}]','active',1,1600,1600),
                     ('m-4','Old','Calculus','Retired.','example step (limits): full; sound from Diagnostic Q1','[{\"observation_id\":\"o-1\"}]','retired',1,1600,1600);
             INSERT INTO memory_topics VALUES('m-3','chain-rule'),('m-4','limits');",
        )
        .unwrap();
        path
    }

    #[test]
    fn a_schema_4_store_upgrades_to_5_and_reads_the_steps_and_readings_its_memories_state() {
        let dir = folder();
        let path = schema_4_store(&dir);
        let store = LearnerStore::open(&path, WORKSPACE).unwrap();
        let version: i64 = store
            .conn
            .query_row("SELECT schema_version FROM workspace_meta", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(version, 5);
        // Two memories state a step for one kind: the less help is kept, as
        // a merge kept it. A retired memory states nothing.
        assert_eq!(
            rows(&store.conn, "help_levels")
                .into_iter()
                .map(|row| row[..6].to_vec())
                .collect::<Vec<_>>(),
            [[
                "discrete expected value",
                "discrete expected value",
                "partial",
                "m-2",
                "partial",
                "NULL"
            ]]
        );
        assert_eq!(
            rows(&store.conn, "imported_readings")
                .into_iter()
                .map(|row| row[..5].to_vec())
                .collect::<Vec<_>>(),
            [[
                "chain-rule",
                "chain-rule",
                "sound",
                "sound from MockA Q3 part b",
                "m-3"
            ]]
        );
        let copies = backups(&dir);
        assert_eq!(copies.len(), 1, "{copies:?}");
        let copy = Connection::open(&copies[0]).unwrap();
        assert_eq!(read_meta(&copy).unwrap().2, 4);
        let fresh_schema_4 = Connection::open_in_memory().unwrap();
        fresh_schema_4.execute_batch(SCHEMA_4).unwrap();
        assert_eq!(shape(&copy), shape(&fresh_schema_4));
        let fresh = LearnerStore::initialize(&dir.join("fresh.sqlite"), WORKSPACE).unwrap();
        assert_eq!(shape(&store.conn), shape(&fresh.conn));
        // Opening it again reads nothing twice.
        drop(store);
        let again = LearnerStore::open(&path, WORKSPACE).unwrap();
        assert_eq!(rows(&again.conn, "help_levels").len(), 1);
        assert_eq!(backups(&dir).len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_failed_upgrade_leaves_the_store_at_schema_2_unchanged() {
        let dir = folder();
        let path = schema_2_store(&dir);
        // A file where the backups folder goes stops the copy, so the upgrade
        // must not start.
        std::fs::write(dir.join(BACKUP_DIR), "not a folder").unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(LearnerStore::open(&path, WORKSPACE).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_checkpoint_saved_before_goal_steps_still_reads() {
        let old = r#"{"current_problem":"b","working_diagnosis":"sign of b","next_action":"check b","plan_text":null,"memory_refs":[],"pending_task":null}"#;
        let checkpoint: Checkpoint = serde_json::from_str(old).unwrap();
        assert!(checkpoint.goal_steps.is_empty());
        // And one without steps saves as it did before.
        assert_eq!(
            serde_json::to_value(&checkpoint).unwrap(),
            serde_json::from_str::<serde_json::Value>(old).unwrap()
        );
        let with_steps: Checkpoint = serde_json::from_str(
            r#"{"current_problem":"b","working_diagnosis":"","next_action":"","plan_text":null,"pending_task":null,
                "goal_steps":[{"step":"read b with its sign","status":"done"},{"step":"use the formula","status":"doing"}]}"#,
        )
        .unwrap();
        assert_eq!(with_steps.goal_steps[1].status, GoalStepStatus::Doing);
    }
}
