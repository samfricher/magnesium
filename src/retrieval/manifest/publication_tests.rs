use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, TransactionBehavior, params, types::Value};

use super::convert::ConverterProvenance;
use super::publication::{PreparedOutcome, prepare_extraction, publish_extraction};
use super::schema::{INDEX_INITIALIZATION_BUDGET, initialize_index, open_index_for_write};
use super::status::stale_candidates;
use super::{CurrentSource, ManifestError, sha256_hex};
use crate::request::{OperationCancellation, enter_request_cancellation};
use crate::retrieval::SensitivityClass;
use crate::retrieval::chunking::{
    ChunkDraft, ChunkKind, SourceImport, SymbolDefinition, SymbolReference,
};
use crate::workspace::native_path_key;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "magnesium-source-publication-{}-{unique}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(root.join(crate::workspace::CONTROL_DIR).join("index")).unwrap();
        let fixture = Self(root);
        let connection = initialize_index(
            &fixture.index(),
            INDEX_INITIALIZATION_BUDGET,
            &OperationCancellation::none(),
        )
        .unwrap();
        connection
            .execute(
                "INSERT INTO source_manifest(manifest_id, path_key, display_path, source_kind,
                content_sha256, index_status, present, observed_generation)
             VALUES (1, ?1, 'lesson.md', 'text', ?2, 'pending', 1, 1)",
                params![native_path_key(Path::new("lesson.md")), sha256_hex(b"h1")],
            )
            .unwrap();
        fixture
    }

    fn index(&self) -> PathBuf {
        self.0
            .join(crate::workspace::CONTROL_DIR)
            .join("index/source-search.sqlite")
    }

    fn connection(&self) -> Connection {
        open_index_for_write(&self.index()).unwrap()
    }

    fn seeded(&self) -> Connection {
        let mut connection = self.connection();
        let source = current_source(&connection);
        let outcome = outcome(&source, OutcomeKind::Ready, "seed");
        publish_extraction(
            &mut connection,
            &self.index(),
            &source,
            &outcome,
            &OperationCancellation::none(),
        )
        .unwrap();
        connection.execute_batch("UPDATE source_manifest SET index_status = 'pending', last_error = 'previous diagnostic' WHERE manifest_id = 1;").unwrap();
        connection
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn current_source(connection: &Connection) -> CurrentSource {
    connection.query_row(
        "SELECT manifest_id, display_path, source_kind, content_sha256, index_status, source_revision
         FROM source_manifest WHERE manifest_id = 1",
        [],
        |row| Ok(CurrentSource {
            manifest_id: row.get(0)?, display_path: row.get(1)?, source_kind: row.get(2)?,
            content_sha256: row.get(3)?, index_status: row.get(4)?, source_revision: row.get(5)?,
        }),
    ).unwrap()
}

fn drafts(label: &str) -> Vec<ChunkDraft> {
    (1..=2)
        .map(|line| ChunkDraft {
            kind: ChunkKind::Symbol,
            line_start: line,
            line_end: line,
            body: format!("{label} source body {line}"),
            structural_path: Some(format!("{label}_symbol_{line}")),
            language: Some("python"),
            definitions: vec![SymbolDefinition {
                name: format!("{label}_symbol_{line}"),
                kind: "function".into(),
                line_start: line,
                line_end: line,
            }],
            imports: vec![SourceImport {
                module: "helpers".into(),
                names: vec!["normalize".into()],
                line_start: line,
                line_end: line,
            }],
            references: vec![SymbolReference {
                name: "normalize".into(),
                line,
            }],
        })
        .collect()
}

#[derive(Clone, Copy, Debug)]
enum OutcomeKind {
    Ready,
    Failed,
    Changed,
    Sensitive,
}

const OUTCOMES: [OutcomeKind; 4] = [
    OutcomeKind::Ready,
    OutcomeKind::Failed,
    OutcomeKind::Changed,
    OutcomeKind::Sensitive,
];

fn outcome(source: &CurrentSource, kind: OutcomeKind, label: &str) -> PreparedOutcome {
    match kind {
        OutcomeKind::Ready => PreparedOutcome::Ready(
            prepare_extraction(
                source,
                drafts(label),
                Some(&ConverterProvenance {
                    method: format!("{label}_converter"),
                    method_version: "fixture-1".into(),
                    coverage: "partial".into(),
                    missing: vec!["page 3".into()],
                    warnings: vec![format!("{label} warning")],
                }),
            )
            .unwrap(),
        ),
        OutcomeKind::Failed => PreparedOutcome::Failed {
            diagnostic: format!("{label} parsing failed"),
        },
        OutcomeKind::Changed => PreparedOutcome::ChangedDuringRead,
        OutcomeKind::Sensitive => PreparedOutcome::Sensitive(SensitivityClass::Credential),
    }
}

type Snapshot = BTreeMap<String, Vec<Vec<Value>>>;

fn snapshot(connection: &Connection) -> Snapshot {
    let tables = connection.prepare("SELECT name FROM sqlite_schema WHERE type = 'table' AND name LIKE 'source_%' ORDER BY name").unwrap()
        .query_map([], |row| row.get::<_, String>(0)).unwrap()
        .collect::<rusqlite::Result<Vec<_>>>().unwrap();
    tables
        .into_iter()
        .map(|table| {
            let selection = if matches!(table.as_str(), "source_search" | "source_search_ranked") {
                "rowid, *"
            } else {
                "*"
            };
            let mut statement = connection
                .prepare(&format!("SELECT {selection} FROM \"{table}\""))
                .unwrap();
            let columns = statement.column_count();
            let ordering = (1..=columns)
                .map(|column| column.to_string())
                .collect::<Vec<_>>()
                .join(",");
            statement = connection
                .prepare(&format!(
                    "SELECT {selection} FROM \"{table}\" ORDER BY {ordering}"
                ))
                .unwrap();
            let rows = statement
                .query_map([], |row| {
                    (0..columns)
                        .map(|column| row.get::<_, Value>(column))
                        .collect::<rusqlite::Result<Vec<_>>>()
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            (table, rows)
        })
        .collect()
}

fn paused_publisher(
    index: PathBuf,
    source: CurrentSource,
    prepared: PreparedOutcome,
) -> (
    mpsc::Receiver<()>,
    mpsc::Sender<()>,
    std::thread::JoinHandle<Result<super::PublishedSource, ManifestError>>,
) {
    let (reached_tx, reached_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let resume_rx = Mutex::new(resume_rx);
    let handle = std::thread::spawn(move || {
        let mut connection = open_index_for_write(&index).unwrap();
        let cancellation =
            OperationCancellation::none().with_test_hook(Arc::new(move |checkpoint| {
                if checkpoint == "source_publication_before_lock" {
                    reached_tx.send(()).unwrap();
                    resume_rx
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(20))
                        .unwrap();
                }
            }));
        let _scope = enter_request_cancellation(&cancellation);
        publish_extraction(&mut connection, &index, &source, &prepared, &cancellation)
    });
    (reached_rx, resume_tx, handle)
}

#[derive(Clone, Copy, Debug)]
enum NewerState {
    ContentReady,
    Excluded,
    MovedReady,
    SameHashReady,
    AbaReady,
    Failed,
}

fn publish_newer(connection: &mut Connection, index: &Path, state: NewerState) {
    match state {
        NewerState::ContentReady => {
            connection
                .execute(
                    "UPDATE source_manifest SET content_sha256 = ?1 WHERE manifest_id = 1",
                    [sha256_hex(b"h2")],
                )
                .unwrap();
        }
        NewerState::MovedReady => {
            connection.execute("UPDATE source_manifest SET path_key = ?1, display_path = 'moved.md' WHERE manifest_id = 1", [native_path_key(Path::new("moved.md"))]).unwrap();
        }
        NewerState::AbaReady => {
            for bytes in [b"h2", b"h1"] {
                connection
                    .execute(
                        "UPDATE source_manifest SET content_sha256 = ?1 WHERE manifest_id = 1",
                        [sha256_hex(bytes)],
                    )
                    .unwrap();
            }
        }
        _ => {}
    }
    let source = current_source(connection);
    let kind = match state {
        NewerState::Excluded => OutcomeKind::Sensitive,
        NewerState::Failed => OutcomeKind::Failed,
        _ => OutcomeKind::Ready,
    };
    let prepared = outcome(&source, kind, "newer");
    publish_extraction(
        connection,
        index,
        &source,
        &prepared,
        &OperationCancellation::none(),
    )
    .unwrap();
}

#[test]
fn all_four_old_outcomes_are_noops_against_six_newer_states_with_two_real_connections() {
    // Fixed bounded Cartesian enumeration; no randomized seed or timing oracle.
    for kind in OUTCOMES {
        for newer in [
            NewerState::ContentReady,
            NewerState::Excluded,
            NewerState::MovedReady,
            NewerState::SameHashReady,
            NewerState::AbaReady,
            NewerState::Failed,
        ] {
            let fixture = Fixture::new();
            let mut winner = fixture.seeded();
            let source = current_source(&winner);
            let old = outcome(&source, kind, "old");
            let (reached, resume, publisher) = paused_publisher(fixture.index(), source, old);
            reached.recv_timeout(Duration::from_secs(20)).unwrap();
            publish_newer(&mut winner, &fixture.index(), newer);
            if !matches!(newer, NewerState::Excluded | NewerState::Failed) {
                assert_projections_aligned(
                    &winner,
                    if matches!(newer, NewerState::MovedReady) {
                        "moved.md"
                    } else {
                        "lesson.md"
                    },
                    "newer",
                );
            }
            if matches!(newer, NewerState::MovedReady) {
                assert_moved_source(&winner);
            }
            let before = snapshot(&winner);
            resume.send(()).unwrap();
            let result = publisher.join().unwrap();
            assert!(
                matches!(result, Err(ManifestError::SourceSuperseded)),
                "old={kind:?}, newer={newer:?}: {result:?}"
            );
            assert_eq!(snapshot(&winner), before, "old={kind:?}, newer={newer:?}");
            if matches!(newer, NewerState::MovedReady) {
                assert_moved_source(&winner);
            }
            // An unchanged scan heartbeat cannot damage the winning result.
            winner.execute_batch("UPDATE source_manifest SET observed_generation = observed_generation + 1 WHERE manifest_id = 1;").unwrap();
            let status: String = winner
                .query_row(
                    "SELECT index_status FROM source_manifest WHERE manifest_id = 1",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                status,
                match newer {
                    NewerState::Excluded => "excluded",
                    NewerState::Failed => "failed",
                    _ => "indexed",
                }
            );
        }
    }
}

fn assert_moved_source(connection: &Connection) {
    let manifest: (String, Vec<u8>) = connection
        .query_row(
            "SELECT display_path, path_key FROM source_manifest WHERE manifest_id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        manifest,
        (
            "moved.md".to_owned(),
            native_path_key(Path::new("moved.md"))
        )
    );
    for table in ["source_search", "source_search_ranked"] {
        let old_paths: i64 = connection
            .query_row(
                &format!("SELECT count(*) FROM {table} WHERE path = 'lesson.md'"),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(old_paths, 0, "old path leaked into {table}");
    }
}

#[test]
fn heartbeat_and_same_content_metadata_do_not_supersede_a_paused_ready_publication() {
    let fixture = Fixture::new();
    let connection = fixture.seeded();
    let source = current_source(&connection);
    let expected_revision = source.source_revision;
    let old = outcome(&source, OutcomeKind::Ready, "allowed");
    let (reached, resume, publisher) = paused_publisher(fixture.index(), source, old);
    reached.recv_timeout(Duration::from_secs(20)).unwrap();
    connection.execute_batch("UPDATE source_manifest SET observed_generation = 2, file_size = 7, modified_unix_ns = 42, display_path = display_path, content_sha256 = content_sha256 WHERE manifest_id = 1;").unwrap();
    assert_eq!(
        current_source(&connection).source_revision,
        expected_revision
    );
    resume.send(()).unwrap();
    publisher.join().unwrap().unwrap();
    let current = current_source(&connection);
    assert_eq!(current.source_revision, expected_revision + 1);
    assert_eq!(current.index_status, "indexed");
    assert_projections_aligned(&connection, "lesson.md", "allowed");
}

fn assert_projections_aligned(connection: &Connection, path: &str, label: &str) {
    let count: i64 = connection
        .query_row(
            "SELECT count(*) FROM source_candidates WHERE status = 'current'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
    for table in ["source_search", "source_search_ranked"] {
        let mismatches: i64 = connection.query_row(&format!(
            "SELECT count(*) FROM {table} f LEFT JOIN source_candidates c ON c.candidate_no = f.rowid
             WHERE c.status IS NOT 'current' OR c.body IS NOT f.body OR f.path IS NOT ?1
                OR c.structural_path IS NOT f.structural_path OR c.content_sha256 IS NOT
                  (SELECT content_sha256 FROM source_manifest WHERE manifest_id = c.manifest_id)"
        ), [path], |row| row.get(0)).unwrap();
        assert_eq!(mismatches, 0);
        let rows: i64 = connection
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(rows, 2);
    }
    let body: String = connection.query_row("SELECT body FROM source_candidates WHERE status = 'current' ORDER BY candidate_no LIMIT 1", [], |row| row.get(0)).unwrap();
    assert!(body.starts_with(label));
    let provenance: (String, String, String, String, String) = connection.query_row(
        "SELECT extraction_method, extraction_method_version, extraction_coverage, extraction_missing_json, extraction_warnings_json FROM source_manifest WHERE manifest_id = 1",
        [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)),
    ).unwrap();
    assert_eq!(
        provenance,
        (
            format!("{label}_converter"),
            "fixture-1".into(),
            "partial".into(),
            "[\"page 3\"]".into(),
            format!("[\"{label} warning\"]")
        )
    );
}

#[test]
fn each_current_outcome_keeps_its_intended_effect_and_advances_once() {
    for kind in OUTCOMES {
        let fixture = Fixture::new();
        let mut connection = fixture.seeded();
        let source = current_source(&connection);
        let before = snapshot(&connection);
        let prepared = outcome(&source, kind, "current");
        publish_extraction(
            &mut connection,
            &fixture.index(),
            &source,
            &prepared,
            &OperationCancellation::none(),
        )
        .unwrap();
        let (status, revision, diagnostic): (String, i64, Option<String>) = connection.query_row("SELECT index_status, source_revision, last_error FROM source_manifest WHERE manifest_id = 1", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
        assert_eq!(revision, source.source_revision + 1);
        assert_eq!(
            status,
            match kind {
                OutcomeKind::Ready => "indexed",
                OutcomeKind::Failed => "failed",
                OutcomeKind::Changed => "stale",
                OutcomeKind::Sensitive => "excluded",
            }
        );
        match kind {
            OutcomeKind::Ready => {
                assert_eq!(diagnostic, None);
                assert_projections_aligned(&connection, "lesson.md", "current");
            }
            OutcomeKind::Failed => {
                assert_eq!(diagnostic.as_deref(), Some("current parsing failed"));
                let after = snapshot(&connection);
                for (table, rows) in &before {
                    if table.starts_with("source_candidates") || table.starts_with("source_search")
                    {
                        assert_eq!(
                            after[table], *rows,
                            "failure must preserve existing {table}"
                        );
                    }
                }
            }
            OutcomeKind::Changed | OutcomeKind::Sensitive => {
                for table in ["source_search", "source_search_ranked"] {
                    let count: i64 = connection
                        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                            row.get(0)
                        })
                        .unwrap();
                    assert_eq!(count, 0);
                }
                let live: i64 = connection.query_row("SELECT count(*) FROM source_candidates WHERE status IS NOT 'stale' OR body IS NOT NULL", [], |row| row.get(0)).unwrap();
                assert_eq!(live, 0);
                if matches!(kind, OutcomeKind::Sensitive) {
                    let sensitive: (String, Option<String>, String, String, i64) = connection.query_row("SELECT source_kind, content_sha256, sensitivity_class, exclusion_reason, present FROM source_manifest WHERE manifest_id = 1", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))).unwrap();
                    assert_eq!(
                        sensitive,
                        (
                            "excluded".into(),
                            None,
                            "credential".into(),
                            "sensitive_extracted".into(),
                            1
                        )
                    );
                    let provenance: i64 = connection.query_row("SELECT count(*) FROM source_manifest WHERE extraction_method IS NOT NULL OR extraction_method_version IS NOT NULL OR extraction_coverage IS NOT NULL OR extraction_missing_json IS NOT NULL OR extraction_warnings_json IS NOT NULL", [], |row| row.get(0)).unwrap();
                    assert_eq!(provenance, 0);
                } else {
                    assert_eq!(
                        diagnostic.as_deref(),
                        Some("source changed after reconciliation")
                    );
                }
            }
        }
    }
}

#[test]
fn failure_then_fresh_ready_retry_works_but_older_same_version_ready_is_superseded() {
    let fixture = Fixture::new();
    let mut connection = fixture.seeded();
    let original = current_source(&connection);
    let old_ready = outcome(&original, OutcomeKind::Ready, "old");
    let (reached, resume, publisher) =
        paused_publisher(fixture.index(), original.clone(), old_ready);
    reached.recv_timeout(Duration::from_secs(20)).unwrap();
    let failed = outcome(&original, OutcomeKind::Failed, "first");
    publish_extraction(
        &mut connection,
        &fixture.index(),
        &original,
        &failed,
        &OperationCancellation::none(),
    )
    .unwrap();
    let before = snapshot(&connection);
    resume.send(()).unwrap();
    assert!(matches!(
        publisher.join().unwrap(),
        Err(ManifestError::SourceSuperseded)
    ));
    assert_eq!(snapshot(&connection), before);
    let fresh = current_source(&connection);
    assert_eq!(fresh.index_status, "failed");
    let retry = outcome(&fresh, OutcomeKind::Ready, "retry");
    publish_extraction(
        &mut connection,
        &fixture.index(),
        &fresh,
        &retry,
        &OperationCancellation::none(),
    )
    .unwrap();
    assert_projections_aligned(&connection, "lesson.md", "retry");
}

#[test]
fn all_old_outcomes_are_noops_after_logical_tombstone_or_physical_manifest_absence() {
    for physical in [false, true] {
        for kind in OUTCOMES {
            let fixture = Fixture::new();
            let mut winner = fixture.seeded();
            let source = current_source(&winner);
            let prepared = outcome(&source, kind, "deleted_old");
            let (reached, resume, publisher) = paused_publisher(fixture.index(), source, prepared);
            reached.recv_timeout(Duration::from_secs(20)).unwrap();
            let transaction = winner
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            stale_candidates(&transaction, 1, &fixture.index()).unwrap();
            transaction.execute_batch(if physical { "DELETE FROM source_manifest WHERE manifest_id = 1;" } else { "UPDATE source_manifest SET present = 0, index_status = 'deleted' WHERE manifest_id = 1;" }).unwrap();
            transaction.commit().unwrap();
            let before = snapshot(&winner);
            resume.send(()).unwrap();
            assert!(matches!(
                publisher.join().unwrap(),
                Err(ManifestError::SourceSuperseded)
            ));
            assert_eq!(
                snapshot(&winner),
                before,
                "physical={physical}, kind={kind:?}"
            );
            let rows: i64 = winner
                .query_row("SELECT count(*) FROM source_manifest", [], |row| row.get(0))
                .unwrap();
            assert_eq!(rows, i64::from(!physical));
        }
    }
}

#[test]
fn cancellation_at_each_precommit_boundary_rolls_back_every_table_and_revision() {
    for kind in OUTCOMES {
        for boundary in [
            "source_publication_before_lock",
            "source_publication_after_lock",
            "source_publication_after_revision_read",
            "source_publication_between_fts_writes",
            "source_publication_after_candidate",
            "source_publication_before_manifest",
            "source_publication_before_commit",
        ] {
            if !matches!(kind, OutcomeKind::Ready)
                && matches!(
                    boundary,
                    "source_publication_between_fts_writes"
                        | "source_publication_after_candidate"
                        | "source_publication_before_manifest"
                )
            {
                continue;
            }
            let fixture = Fixture::new();
            let mut connection = fixture.seeded();
            let source = current_source(&connection);
            let prepared = outcome(&source, kind, "cancelled");
            let before = snapshot(&connection);
            let flag = Arc::new(AtomicBool::new(false));
            let trigger = flag.clone();
            let reached = Arc::new(AtomicUsize::new(0));
            let observed = reached.clone();
            let cancellation = OperationCancellation::with_flag(flag).with_test_hook(Arc::new(
                move |checkpoint| {
                    if checkpoint == boundary {
                        observed.fetch_add(1, Ordering::Relaxed);
                        trigger.store(true, Ordering::Relaxed);
                    }
                },
            ));
            let result = {
                let _scope = enter_request_cancellation(&cancellation);
                publish_extraction(
                    &mut connection,
                    &fixture.index(),
                    &source,
                    &prepared,
                    &cancellation,
                )
            };
            assert!(
                matches!(result, Err(ManifestError::Cancelled)),
                "{boundary}: {result:?}"
            );
            assert_eq!(
                reached.load(Ordering::Relaxed),
                1,
                "{boundary} was not exercised"
            );
            assert_eq!(snapshot(&connection), before, "{kind:?}, {boundary}");
        }
    }
}

#[test]
fn cancellation_while_waiting_for_a_real_writer_lock_leaves_state_unchanged() {
    let fixture = Fixture::new();
    let mut blocker = fixture.seeded();
    let source = current_source(&blocker);
    let prepared = outcome(&source, OutcomeKind::Ready, "waiting");
    let before = snapshot(&blocker);
    let transaction = blocker
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let index = fixture.index();
    let (waiting_tx, waiting_rx) = mpsc::channel();
    let publisher = std::thread::spawn(move || {
        let mut connection = open_index_for_write(&index).unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        let trigger = flag.clone();
        let cancellation =
            OperationCancellation::with_flag(flag).with_test_hook(Arc::new(move |checkpoint| {
                if checkpoint == "source_publication_waiting_for_lock" {
                    trigger.store(true, Ordering::Relaxed);
                    waiting_tx.send(()).unwrap();
                }
            }));
        let _scope = enter_request_cancellation(&cancellation);
        publish_extraction(&mut connection, &index, &source, &prepared, &cancellation)
    });
    waiting_rx.recv_timeout(Duration::from_secs(20)).unwrap();
    assert!(matches!(
        publisher.join().unwrap(),
        Err(ManifestError::Cancelled)
    ));
    transaction.rollback().unwrap();
    assert_eq!(snapshot(&blocker), before);
}

#[test]
fn cancellation_after_commit_reports_completion_and_keeps_the_committed_state() {
    let fixture = Fixture::new();
    let mut connection = fixture.seeded();
    let source = current_source(&connection);
    let prepared = outcome(&source, OutcomeKind::Ready, "committed");
    let flag = Arc::new(AtomicBool::new(false));
    let trigger = flag.clone();
    let cancellation = OperationCancellation::with_flag(flag.clone()).with_test_hook(Arc::new(
        move |checkpoint| {
            if checkpoint == "source_publication_after_commit" {
                trigger.store(true, Ordering::Relaxed);
            }
        },
    ));
    let result = {
        let _scope = enter_request_cancellation(&cancellation);
        publish_extraction(
            &mut connection,
            &fixture.index(),
            &source,
            &prepared,
            &cancellation,
        )
    };
    result.unwrap();
    assert!(
        flag.load(Ordering::Relaxed),
        "after-commit checkpoint was not exercised"
    );
    let current = current_source(&connection);
    assert_eq!(current.index_status, "indexed");
    assert_eq!(current.source_revision, source.source_revision + 1);
    assert_projections_aligned(&connection, "lesson.md", "committed");
}

#[test]
fn actual_sqlite_failure_between_fts_writes_before_manifest_and_at_commit_rolls_back() {
    for (fault, expected_action) in [
        (
            "CREATE TRIGGER reject_matching_fts BEFORE INSERT ON source_search_content BEGIN SELECT RAISE(ABORT, 'injected matching FTS write failure'); END;",
            "index source candidate for search",
        ),
        (
            "CREATE TRIGGER reject_manifest BEFORE UPDATE OF index_status ON source_manifest WHEN NEW.index_status = 'indexed' BEGIN SELECT RAISE(ABORT, 'injected manifest finalization failure'); END;",
            "complete source extraction",
        ),
        (
            "CREATE TABLE rejected_publication(manifest_id INTEGER REFERENCES source_manifest(manifest_id) DEFERRABLE INITIALLY DEFERRED) STRICT; CREATE TRIGGER reject_commit AFTER UPDATE OF index_status ON source_manifest WHEN NEW.index_status = 'indexed' BEGIN INSERT INTO rejected_publication VALUES (999); END;",
            "commit guarded source publication",
        ),
    ] {
        let fixture = Fixture::new();
        let mut connection = fixture.seeded();
        let source = current_source(&connection);
        let prepared = outcome(&source, OutcomeKind::Ready, "rejected");
        connection.execute_batch(fault).unwrap();
        let before = snapshot(&connection);
        let result = publish_extraction(
            &mut connection,
            &fixture.index(),
            &source,
            &prepared,
            &OperationCancellation::none(),
        );
        assert!(
            matches!(&result, Err(ManifestError::Sqlite { action, .. }) if *action == expected_action),
            "expected {expected_action}: {result:?}"
        );
        assert_eq!(snapshot(&connection), before, "{expected_action}");
        assert_eq!(
            current_source(&connection).source_revision,
            source.source_revision
        );
        if expected_action == "commit guarded source publication" {
            let leaked: i64 = connection
                .query_row("SELECT count(*) FROM rejected_publication", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(leaked, 0);
        }
    }
}

#[test]
fn missing_manifest_table_is_a_storage_error_instead_of_a_superseded_or_empty_result() {
    let fixture = Fixture::new();
    let mut connection = fixture.connection();
    let source = current_source(&connection);
    let prepared = outcome(&source, OutcomeKind::Ready, "unreadable");
    connection
        .execute_batch("DROP TABLE source_manifest;")
        .unwrap();
    let before = snapshot(&connection);
    let result = publish_extraction(
        &mut connection,
        &fixture.index(),
        &source,
        &prepared,
        &OperationCancellation::none(),
    );
    assert!(matches!(
        result,
        Err(ManifestError::Sqlite {
            action: "read source publication revision",
            ..
        })
    ));
    assert_eq!(snapshot(&connection), before);
}
