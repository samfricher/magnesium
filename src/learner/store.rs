use super::*;
use rusqlite::{Connection, OptionalExtension, params};
use std::{path::Path, time::Duration};
pub const SCHEMA_VERSION: i64 = 2;
pub(crate) const SCHEMA_SQL: &str = include_str!("schema.sql");
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
        let meta=conn.query_row("SELECT workspace_id,storage_family,schema_version FROM workspace_meta WHERE singleton=1",[],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?))).map_err(|_|LearnerError::new("incompatible_store","Expected a fresh learner-model/v2 store. Preserve the existing store and initialize a fresh workspace; no migration is performed."))?;
        if meta.1 != SCHEMA || meta.2 != SCHEMA_VERSION {
            return Err(LearnerError::new(
                "incompatible_store",
                "Unsupported storage family/version. Preserve the existing store and initialize a fresh workspace; no migration is performed.",
            ));
        }
        if meta.0 != workspace_id {
            return Err(LearnerError::new(
                "workspace_mismatch",
                "Workspace identity differs",
            ));
        }
        configure(&conn)?;
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
