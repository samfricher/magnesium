use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};
use std::time::{Duration, Instant};

use rusqlite::{Connection, OpenFlags, TransactionBehavior};

use super::{ManifestError, io_error, reject_managed_symlink, sqlite};
use crate::retrieval::coordinator::OperationCancellation;

/// SQLite `application_id` of the source cache: ASCII "MGSI", Magnesium source index.
pub(crate) const INDEX_APPLICATION_ID: i64 = 0x4d47_5349;
pub(crate) const INDEX_SCHEMA_VERSION: i64 = 10;
pub(crate) const INDEX_INITIALIZATION_BUDGET: Duration = Duration::from_secs(2);
pub(crate) const INDEX_DIAGNOSTIC_BUDGET: Duration = Duration::from_secs(5);

// Fresh caches are provisioned directly at today's schema. This derived cache
// has no historical readers or migrations; incompatible files remain untouched.
pub(crate) const INDEX_SCHEMA: &str = r#"
CREATE TABLE source_index_metadata (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    generation INTEGER NOT NULL CHECK (generation >= 0)
) STRICT;
INSERT INTO source_index_metadata(singleton, generation) VALUES (1, 0);

CREATE TABLE source_manifest (
    manifest_id INTEGER PRIMARY KEY,
    source_revision INTEGER NOT NULL DEFAULT 1 CHECK (source_revision >= 1),
    path_key BLOB NOT NULL UNIQUE,
    display_path TEXT NOT NULL,
    source_kind TEXT NOT NULL CHECK (source_kind IN ('text', 'pdf', 'excluded')),
    file_size INTEGER CHECK (file_size IS NULL OR file_size >= 0),
    modified_unix_ns INTEGER CHECK (modified_unix_ns IS NULL OR modified_unix_ns >= 0),
    content_sha256 TEXT CHECK (content_sha256 IS NULL OR length(content_sha256) = 64),
    sensitivity_class TEXT,
    exclusion_reason TEXT,
    index_status TEXT NOT NULL CHECK (
        index_status IN ('pending', 'indexed', 'stale', 'failed', 'excluded', 'deleted')
    ),
    present INTEGER NOT NULL CHECK (present IN (0, 1)),
    observed_generation INTEGER NOT NULL CHECK (observed_generation >= 1),
    last_error TEXT,
    -- Optional-converter provenance (#208): NULL means the native fast
    -- path produced the current generation. A worker-assisted generation
    -- records its method/version plus truthful coverage, so the source
    -- response never misattributes derived text to the native worker.
    extraction_method TEXT,
    extraction_method_version TEXT,
    extraction_coverage TEXT,
    extraction_missing_json TEXT,
    extraction_warnings_json TEXT
) STRICT;

-- This is the sole owner of source revisions. Material state changes invalidate
-- prepared extraction outcomes; identical assignments do not. Scan heartbeats
-- (observed_generation) and verified-content metadata refreshes (file_size and
-- modified_unix_ns) are deliberately excluded. Add future material fields here
-- and to the independent mutation tests. The inner revision-only update cannot
-- recurse, even with recursive_triggers enabled. It never changes projections.
CREATE TRIGGER source_manifest_revision
AFTER UPDATE OF
    path_key, display_path, source_kind, content_sha256, index_status,
    present, sensitivity_class, exclusion_reason, last_error,
    extraction_method, extraction_method_version, extraction_coverage,
    extraction_missing_json, extraction_warnings_json
ON source_manifest
WHEN
       NEW.path_key IS NOT OLD.path_key
    OR NEW.display_path IS NOT OLD.display_path
    OR NEW.source_kind IS NOT OLD.source_kind
    OR NEW.content_sha256 IS NOT OLD.content_sha256
    OR NEW.index_status IS NOT OLD.index_status
    OR NEW.present IS NOT OLD.present
    OR NEW.sensitivity_class IS NOT OLD.sensitivity_class
    OR NEW.exclusion_reason IS NOT OLD.exclusion_reason
    OR NEW.last_error IS NOT OLD.last_error
    OR NEW.extraction_method IS NOT OLD.extraction_method
    OR NEW.extraction_method_version IS NOT OLD.extraction_method_version
    OR NEW.extraction_coverage IS NOT OLD.extraction_coverage
    OR NEW.extraction_missing_json IS NOT OLD.extraction_missing_json
    OR NEW.extraction_warnings_json IS NOT OLD.extraction_warnings_json
BEGIN
    UPDATE source_manifest
    SET source_revision = OLD.source_revision + 1
    WHERE manifest_id = NEW.manifest_id;
END;

CREATE TABLE source_candidates (
    candidate_no INTEGER PRIMARY KEY,
    candidate_id TEXT NOT NULL UNIQUE CHECK (length(candidate_id) = 64),
    manifest_id INTEGER NOT NULL REFERENCES source_manifest(manifest_id) ON DELETE CASCADE,
    content_sha256 TEXT NOT NULL CHECK (length(content_sha256) = 64),
    anchor_kind TEXT NOT NULL CHECK (anchor_kind IN ('line', 'page')),
    anchor_start INTEGER NOT NULL CHECK (anchor_start >= 1),
    anchor_end INTEGER NOT NULL CHECK (anchor_end >= anchor_start),
    body TEXT,
    status TEXT NOT NULL CHECK (status IN ('current', 'stale')),
    chunk_kind TEXT NOT NULL DEFAULT 'fallback'
        CHECK (chunk_kind IN ('fallback', 'heading', 'page', 'module', 'symbol')),
    structural_path TEXT,
    language TEXT CHECK (
        language IS NULL OR language IN ('python', 'javascript', 'typescript', 'tsx')
    ),
    definitions_json TEXT NOT NULL DEFAULT '[]'
        CHECK (json_valid(definitions_json) AND json_type(definitions_json) = 'array'),
    imports_json TEXT NOT NULL DEFAULT '[]'
        CHECK (json_valid(imports_json) AND json_type(imports_json) = 'array'),
    references_json TEXT NOT NULL DEFAULT '[]'
        CHECK (json_valid(references_json) AND json_type(references_json) = 'array')
) STRICT;
CREATE INDEX source_candidates_current_by_manifest
    ON source_candidates(manifest_id, content_sha256, status, anchor_start, candidate_id);

CREATE TABLE source_reconciliation_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    scan_generation INTEGER NOT NULL CHECK (scan_generation >= 1),
    cursor_json TEXT NOT NULL
) STRICT;
CREATE VIRTUAL TABLE source_search USING fts5(
    path,
    structural_path,
    body,
    symbols,
    tokenize = "unicode61 tokenchars '_'"
);

-- Matching preserves original text. Ranking omits repeated navigation only.
CREATE VIRTUAL TABLE source_search_ranked USING fts5(
    path,
    structural_path,
    body,
    symbols,
    tokenize = "unicode61 tokenchars '_'"
);
"#;

/// Ordinary readers do not create, migrate, or inspect the contents of a cache.
/// Read-only opening also prevents an incompatible file's journal recovery or
/// close-time WAL checkpoint from modifying it before ownership is established.
pub(crate) fn open_index(path: &Path) -> Result<Connection, ManifestError> {
    open_index_with_budget(
        path,
        INDEX_INITIALIZATION_BUDGET,
        &OperationCancellation::none(),
    )
}

pub(crate) fn open_index_with_budget(
    path: &Path,
    duration: Duration,
    cancellation: &OperationCancellation,
) -> Result<Connection, ManifestError> {
    let budget = IndexBudget::new(duration, cancellation, "open");
    let connection = connect(path, OpenFlags::SQLITE_OPEN_READ_ONLY, &budget, false)?;
    let result = validate_identity(&connection, path);
    budget.finish(&connection, path, result)?;
    Ok(connection)
}

/// Writers establish ownership on a read-only connection before opening a
/// writable handle. Recheck identity on that handle to reject a replaced cache.
pub(crate) fn open_index_for_write(path: &Path) -> Result<Connection, ManifestError> {
    let budget = IndexBudget::new(
        INDEX_INITIALIZATION_BUDGET,
        &OperationCancellation::none(),
        "open",
    );
    let checked = connect(path, OpenFlags::SQLITE_OPEN_READ_ONLY, &budget, false)?;
    budget.finish(&checked, path, validate_identity(&checked, path))?;
    let connection = connect(path, OpenFlags::SQLITE_OPEN_READ_WRITE, &budget, false)?;
    budget.finish(&connection, path, validate_identity(&connection, path))?;
    Ok(connection)
}

/// Hold an existing index open for one bounded request. While this handle is
/// open, closing a per-file connection is not the last close, so SQLite does
/// not checkpoint and fsync the write-ahead log once per file; this handle's
/// close checkpoints once. Commits stay exactly as durable. `None` when the
/// index does not exist yet or cannot be opened: the request's own opens then
/// report any error as before.
pub(crate) fn retain_index_for_request(path: &Path) -> Option<Connection> {
    match path.try_exists() {
        Ok(true) => open_index_for_write(path).ok(),
        _ => None,
    }
}

/// Called only by explicit workspace initialization and reconciliation's
/// initialization owner. The immediate transaction arbitrates concurrent first
/// opens; identity and emptiness are checked again while holding its write lock.
pub(crate) fn initialize_index(
    path: &Path,
    duration: Duration,
    cancellation: &OperationCancellation,
) -> Result<Connection, ManifestError> {
    let budget = IndexBudget::new(duration, cancellation, "initialization");
    budget.check(path)?;
    validate_path(path)?;
    if path
        .try_exists()
        .map_err(|source| io_error("inspect source index", path, source))?
    {
        let checked = connect(path, OpenFlags::SQLITE_OPEN_READ_ONLY, &budget, true)?;
        // Identity and emptiness must describe one snapshot: another creator
        // can publish the schema between two otherwise independent reads.
        let snapshot = checked
            .unchecked_transaction()
            .map_err(|source| sqlite(path, "begin source-index ownership snapshot", source))?;
        let result = validate_initializable(&snapshot, path);
        drop(snapshot);
        budget.finish(&checked, path, result)?;
    }
    let mut connection = connect(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        &budget,
        true,
    )?;
    let result = (|| {
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| sqlite(path, "start source-index initialization", source))?;
        let fresh = validate_initializable(&transaction, path)?;
        if fresh {
            budget.check(path)?;
            transaction
                .execute_batch(INDEX_SCHEMA)
                .map_err(|source| sqlite(path, "initialize source-index schema", source))?;
            transaction
                .pragma_update(None, "application_id", INDEX_APPLICATION_ID)
                .map_err(|source| sqlite(path, "set source-index application ID", source))?;
            transaction
                .pragma_update(None, "user_version", INDEX_SCHEMA_VERSION)
                .map_err(|source| sqlite(path, "set source-index schema version", source))?;
        }
        budget.check(path)?;
        transaction
            .commit()
            .map_err(|source| sqlite(path, "commit source-index initialization", source))?;
        if fresh {
            // Another initializer can briefly hold a read or writer lock
            // after observing the committed final schema. Journal-mode
            // changes report BUSY immediately in that case, so retry only
            // within this initialization's existing deadline/cancellation.
            loop {
                budget.check(path)?;
                match connection.pragma_update(None, "journal_mode", "WAL") {
                    Ok(()) => break,
                    Err(source)
                        if matches!(
                            source.sqlite_error_code(),
                            Some(
                                rusqlite::ErrorCode::DatabaseBusy
                                    | rusqlite::ErrorCode::DatabaseLocked
                            )
                        ) =>
                    {
                        std::thread::sleep(
                            budget
                                .deadline
                                .saturating_duration_since(Instant::now())
                                .min(Duration::from_millis(2)),
                        );
                    }
                    Err(source) => return Err(sqlite(path, "enable source-index WAL", source)),
                }
            }
        }
        Ok(())
    })();
    budget.finish(&connection, path, result)?;
    Ok(connection)
}

/// Expensive diagnostics are explicit and bounded, never hidden in queries.
/// Corruption is an error with a manual cache-rebuild instruction, not no hits.
pub(crate) fn validate_index_integrity(
    path: &Path,
    duration: Duration,
    cancellation: &OperationCancellation,
) -> Result<(), ManifestError> {
    let budget = IndexBudget::new(duration, cancellation, "integrity diagnostic");
    let connection = connect(path, OpenFlags::SQLITE_OPEN_READ_ONLY, &budget, false)?;
    let result = (|| {
        validate_identity(&connection, path)?;
        budget.check(path)?;
        let integrity: String = connection
            .query_row("PRAGMA quick_check(1)", [], |row| row.get(0))
            .map_err(|source| sqlite(path, "validate source index", source))?;
        if integrity != "ok" {
            return Err(incompatible(
                path,
                &format!("failed integrity validation: {integrity}"),
            ));
        }
        Ok(())
    })();
    budget.finish(&connection, path, result)
}

fn validate_path(path: &Path) -> Result<(), ManifestError> {
    if path
        .file_name()
        .is_none_or(|name| name != "source-search.sqlite")
        || path
            .parent()
            .and_then(Path::file_name)
            .is_none_or(|name| name != "index")
        || path
            .parent()
            .and_then(Path::parent)
            .and_then(Path::file_name)
            .is_none_or(|name| name != crate::workspace::CONTROL_DIR)
    {
        return Err(ManifestError::Invalid(format!(
            "source index {} must be the workspace {}/index/source-search.sqlite",
            path.display(),
            crate::workspace::CONTROL_DIR
        )));
    }
    reject_managed_symlink(path)?;
    for suffix in ["-wal", "-shm", "-journal"] {
        reject_managed_symlink(&sidecar_path(path, suffix))?;
    }
    Ok(())
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// SQLite's read-only WAL connections can update shared-memory read marks.
/// Reject unowned/incompatible main-file headers before SQLite sees the path,
/// so foreign databases and their sidecars remain byte-preserved. Our creator
/// commits the final identity before enabling WAL. An old cache with a newer
/// identity only in WAL must be checkpointed by its owner or rebuilt explicitly.
/// This bounded preflight is not an integrity check; SQLite still validates the
/// actual connection's identity, and initialization checks the schema under lock.
fn validate_header_ownership(
    path: &Path,
    allow_empty: bool,
    budget: &IndexBudget,
) -> Result<(), ManifestError> {
    loop {
        budget.cancellation.check()?;
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(error) if allow_empty && error.kind() == std::io::ErrorKind::NotFound => {
                if has_sidecars(path)? {
                    return Err(incompatible(
                        path,
                        "is missing but has preexisting SQLite sidecars",
                    ));
                }
                return Ok(());
            }
            Err(source) => return Err(io_error("inspect source-index header", path, source)),
        };
        let mut header = Vec::with_capacity(72);
        // Fixed-size ownership read, independent of source-cache row count.
        let bytes = file
            .by_ref()
            .take(72)
            .read_to_end(&mut header)
            .map_err(|source| io_error("inspect source-index header", path, source))?;
        if bytes != 0 {
            if bytes != 72 {
                return Err(incompatible(path, "has an incomplete SQLite header"));
            }
            if &header[..16] != b"SQLite format 3\0" {
                return Err(incompatible(path, "is not a SQLite source cache"));
            }
            let application = i64::from(u32::from_be_bytes(header[68..72].try_into().unwrap()));
            let version = i64::from(u32::from_be_bytes(header[60..64].try_into().unwrap()));
            if (application, version) == (INDEX_APPLICATION_ID, INDEX_SCHEMA_VERSION) {
                return Ok(());
            }
            if !allow_empty || (application, version) != (0, 0) {
                return Err(wrong_identity(path, (application, version)));
            }
        } else if !allow_empty {
            return Err(wrong_identity(path, (0, 0)));
        }
        if !has_sidecars(path)? {
            return Ok(());
        }
        // A concurrent initializer may be publishing the final header while
        // its rollback journal exists. Wait/recheck under our explicit budget;
        // never let SQLite recover, truncate or adopt an unowned artifact.
        let remaining = budget.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(incompatible(
                path,
                "has no committed Magnesium identity and has preexisting SQLite sidecars; initialization ownership wait budget exhausted",
            ));
        }
        std::thread::sleep(remaining.min(Duration::from_millis(2)));
    }
}

fn has_sidecars(path: &Path) -> Result<bool, ManifestError> {
    for suffix in ["-wal", "-shm", "-journal"] {
        let sidecar = sidecar_path(path, suffix);
        if sidecar
            .try_exists()
            .map_err(|source| io_error("inspect source-index sidecar", &sidecar, source))?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn connect(
    path: &Path,
    flags: OpenFlags,
    budget: &IndexBudget,
    allow_empty: bool,
) -> Result<Connection, ManifestError> {
    budget.check(path)?;
    validate_path(path)?;
    validate_header_ownership(path, allow_empty, budget)?;
    let connection = Connection::open_with_flags(path, flags)
        .map_err(|source| sqlite(path, "open source index", source))?;
    #[cfg(test)]
    sql_trace::attach(&connection);
    connection
        .busy_timeout(budget.deadline.saturating_duration_since(Instant::now()))
        .map_err(|source| sqlite(path, "configure source index timeout", source))?;
    let cancellation = budget.cancellation.clone();
    let deadline = budget.deadline;
    let reason = Arc::clone(&budget.reason);
    connection
        .progress_handler(
            1000,
            Some(move || {
                let cause = if cancellation.is_cancelled() {
                    1
                } else if Instant::now() >= deadline {
                    2
                } else {
                    0
                };
                if cause != 0 {
                    reason.store(cause, Ordering::Relaxed);
                }
                cause != 0
            }),
        )
        .map_err(|source| sqlite(path, "install source-index budget handler", source))?;
    // Connection-local only: no persistent journal or database settings before
    // identity validation. A foreign database is never opened for writing.
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(|source| sqlite(path, "enable source-index foreign keys", source))?;
    Ok(connection)
}

fn identity(connection: &Connection, path: &Path) -> Result<(i64, i64), ManifestError> {
    let application_id = connection
        .pragma_query_value(None, "application_id", |row| row.get(0))
        .map_err(|source| sqlite(path, "read source-index application ID", source))?;
    let schema_version = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|source| sqlite(path, "read source-index schema version", source))?;
    Ok((application_id, schema_version))
}

fn validate_identity(connection: &Connection, path: &Path) -> Result<(), ManifestError> {
    let found = identity(connection, path)?;
    if found != (INDEX_APPLICATION_ID, INDEX_SCHEMA_VERSION) {
        return Err(wrong_identity(path, found));
    }
    Ok(())
}

fn validate_initializable(connection: &Connection, path: &Path) -> Result<bool, ManifestError> {
    let found = identity(connection, path)?;
    if found == (INDEX_APPLICATION_ID, INDEX_SCHEMA_VERSION) {
        return Ok(false);
    }
    if found == (0, 0) {
        let objects: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
                [],
                |row| row.get(0),
            )
            .map_err(|source| sqlite(path, "inspect empty source-index schema", source))?;
        if objects == 0 {
            return Ok(true);
        }
    }
    Err(wrong_identity(path, found))
}

fn wrong_identity(path: &Path, (application_id, schema_version): (i64, i64)) -> ManifestError {
    incompatible(
        path,
        &format!(
            "has application ID {application_id} and schema version {schema_version}; expected {INDEX_APPLICATION_ID} and {INDEX_SCHEMA_VERSION}"
        ),
    )
}

fn incompatible(path: &Path, detail: &str) -> ManifestError {
    ManifestError::Invalid(format!(
        "source index {} {detail}. The file was not rebuilt. To rebuild the derived source cache, stop Magnesium processes, preserve and move this file and any matching -wal/-shm/-journal sidecars out of {control}/index together, then run magnesium init and retry the source query. Do not move or delete {control}/state.sqlite; preserve foreign databases for their owner.",
        path.display(),
        control = crate::workspace::CONTROL_DIR
    ))
}

struct IndexBudget {
    deadline: Instant,
    cancellation: OperationCancellation,
    reason: Arc<AtomicU8>,
    operation: &'static str,
}

impl IndexBudget {
    fn new(
        duration: Duration,
        cancellation: &OperationCancellation,
        operation: &'static str,
    ) -> Self {
        Self {
            deadline: Instant::now() + duration,
            cancellation: cancellation.clone(),
            reason: Arc::new(AtomicU8::new(0)),
            operation,
        }
    }

    fn check(&self, path: &Path) -> Result<(), ManifestError> {
        self.cancellation.check()?;
        if Instant::now() >= self.deadline {
            return Err(ManifestError::Invalid(format!(
                "source index {} {} budget exhausted; the operation is incomplete, retry with a larger explicit budget",
                path.display(),
                self.operation
            )));
        }
        Ok(())
    }

    fn finish<T>(
        &self,
        connection: &Connection,
        path: &Path,
        result: Result<T, ManifestError>,
    ) -> Result<T, ManifestError> {
        connection
            .progress_handler(0, None::<fn() -> bool>)
            .map_err(|source| sqlite(path, "remove source-index budget handler", source))?;
        if self.reason.load(Ordering::Relaxed) == 1 {
            return Err(ManifestError::Cancelled);
        }
        self.check(path)?;
        result
    }
}

#[cfg(test)]
pub(crate) mod sql_trace {
    use rusqlite::Connection;
    use std::cell::RefCell;

    type StatementHook = Box<dyn FnMut(&str)>;

    thread_local! {
        static STATEMENTS: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
        static HOOK: RefCell<Option<StatementHook>> = const { RefCell::new(None) };
    }

    pub(crate) fn attach(connection: &Connection) {
        if STATEMENTS.with(|slot| slot.borrow().is_some()) {
            connection.trace_v2(
                rusqlite::trace::TraceEventCodes::SQLITE_TRACE_STMT
                    | rusqlite::trace::TraceEventCodes::SQLITE_TRACE_ROW,
                Some(|event| {
                    if let rusqlite::trace::TraceEvent::Row(statement) = &event
                        && statement
                            .sql()
                            .starts_with("WITH ranked_matches AS MATERIALIZED")
                    {
                        HOOK.with(|slot| {
                            if let Some(hook) = slot.borrow_mut().as_mut() {
                                hook("ranked row");
                            }
                        });
                    }
                    if let rusqlite::trace::TraceEvent::Stmt(_, sql) = event {
                        HOOK.with(|slot| {
                            if let Some(hook) = slot.borrow_mut().as_mut() {
                                hook(sql);
                            }
                        });
                        STATEMENTS.with(|slot| {
                            if let Some(statements) = slot.borrow_mut().as_mut() {
                                statements.push(sql.to_owned());
                            }
                        });
                    }
                }),
            );
        }
    }

    pub(crate) fn record<T>(operation: impl FnOnce() -> T) -> (T, Vec<String>) {
        STATEMENTS.with(|slot| assert!(slot.replace(Some(Vec::new())).is_none()));
        let result = operation();
        HOOK.with(|slot| slot.take());
        let statements = STATEMENTS.with(|slot| slot.take().unwrap());
        (result, statements)
    }
    pub(crate) fn record_with_hook<T>(
        hook: impl FnMut(&str) + 'static,
        operation: impl FnOnce() -> T,
    ) -> (T, Vec<String>) {
        HOOK.with(|slot| assert!(slot.replace(Some(Box::new(hook))).is_none()));
        record(operation)
    }
}
