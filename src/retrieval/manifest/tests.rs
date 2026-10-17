use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pdf_extract::{Document, Object, Stream, content::Content, content::Operation, dictionary};
use rusqlite::{Connection, params};

use super::extract::*;
use super::pdf::*;
use super::reconcile::*;
use super::schema::*;
use super::status::*;
use super::*;
use crate::retrieval::DiscoveryReport;
use crate::retrieval::chunking::ChunkKind;
use crate::workspace::native_path_key;

static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

#[test]
fn ordinary_source_index_open_is_identity_only() {
    let fixture = Fixture::new("identity-only-open");
    drop(
        initialize_index(
            &fixture.index(),
            INDEX_INITIALIZATION_BUDGET,
            &crate::retrieval::coordinator::OperationCancellation::none(),
        )
        .unwrap(),
    );
    let (result, statements) = sql_trace::record(|| open_index(&fixture.index()));
    drop(result.unwrap());
    eprintln!("repeated source-index open SQL: {statements:#?}");
    assert!(
        !statements.iter().any(|sql| {
            let sql = sql.to_ascii_lowercase();
            [
                "quick_check",
                "alter ",
                "create ",
                "drop ",
                "rebuild",
                "vacuum",
                "source_candidates",
            ]
            .iter()
            .any(|forbidden| sql.contains(forbidden))
        }),
        "ordinary open performed provisioning or whole-index work: {statements:#?}"
    );
    assert!(
        statements.len() <= 3,
        "ordinary open must only configure foreign keys and read two identity PRAGMAs: {statements:#?}"
    );
}

#[test]
fn fresh_source_index_creates_final_schema_directly() {
    let fixture = Fixture::new("direct-final-schema");
    let (result, statements) = sql_trace::record(|| {
        initialize_index(
            &fixture.index(),
            INDEX_INITIALIZATION_BUDGET,
            &crate::retrieval::coordinator::OperationCancellation::none(),
        )
    });
    let connection = result.unwrap();
    let columns: i64 = connection.query_row(
        "SELECT count(*) FROM pragma_table_info('source_candidates') WHERE name IN ('chunk_kind', 'structural_path', 'language', 'definitions_json', 'imports_json', 'references_json')", [], |row| row.get(0)).unwrap();
    assert_eq!(columns, 6);
    let numeric_key: (String, i64) = connection.query_row(
        "SELECT type, pk FROM pragma_table_info('source_candidates') WHERE name = 'candidate_no'",
        [], |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap();
    assert_eq!(numeric_key, ("INTEGER".to_owned(), 1));
    for table in ["source_search", "source_search_ranked"] {
        let metadata_copies: i64 = connection.query_row(
            "SELECT count(*) FROM pragma_table_info(?1) WHERE name IN ('candidate_id', 'manifest_id')",
            [table], |row| row.get(0),
        ).unwrap();
        assert_eq!(metadata_copies, 0);
    }
    assert!(
        !statements.iter().any(|sql| {
            let sql = sql.to_ascii_lowercase();
            sql.contains("alter table")
                || sql.contains("drop table")
                || sql.contains("source_candidates_v3")
        }),
        "fresh initialization replayed historical schema surgery: {statements:#?}"
    );
}

#[test]
fn foreign_wal_cache_and_sidecars_remain_byte_preserved() {
    for set_identity_before_wal in [false, true] {
        let fixture = Fixture::new("foreign-wal");
        let index = fixture.index();
        let owner = Connection::open(&index).unwrap();
        if set_identity_before_wal {
            owner.pragma_update(None, "application_id", 31).unwrap();
        }
        owner.pragma_update(None, "journal_mode", "WAL").unwrap();
        owner.execute_batch("CREATE TABLE foreign_private(value TEXT); INSERT INTO foreign_private VALUES ('private owner content'); PRAGMA application_id = 31; PRAGMA user_version = 99;").unwrap();
        let artifacts = |path: &Path| {
            ["", "-wal", "-shm"].map(|suffix| {
                let mut file = path.as_os_str().to_os_string();
                file.push(suffix);
                fs::read(PathBuf::from(file)).unwrap()
            })
        };
        let before = artifacts(&index);
        for error in [
            open_index(&index).unwrap_err(),
            open_index_for_write(&index).unwrap_err(),
            initialize_index(
                &index,
                INDEX_INITIALIZATION_BUDGET,
                &crate::retrieval::OperationCancellation::none(),
            )
            .unwrap_err(),
        ] {
            assert!(
                error.to_string().contains("preserve foreign databases"),
                "{error}"
            );
        }
        assert_eq!(
            artifacts(&index),
            before,
            "foreign main, WAL and shared-memory bytes must stay unchanged"
        );
    }
}

#[test]
fn missing_main_file_does_not_adopt_or_modify_orphaned_sidecars() {
    for suffix in ["-wal", "-shm", "-journal"] {
        let fixture = Fixture::new("orphaned-index-sidecar");
        let index = fixture.index();
        let mut sidecar = index.as_os_str().to_os_string();
        sidecar.push(suffix);
        let sidecar = PathBuf::from(sidecar);
        fs::write(&sidecar, b"foreign orphaned SQLite artifact").unwrap();
        let before = fs::read(&sidecar).unwrap();
        let error = initialize_index(
            &index,
            INDEX_INITIALIZATION_BUDGET,
            &crate::retrieval::OperationCancellation::none(),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("preexisting SQLite sidecars"),
            "{error}"
        );
        assert!(!index.exists());
        assert_eq!(fs::read(&sidecar).unwrap(), before);
    }
}

#[test]
fn explicit_source_index_diagnostics_report_integrity_failure_without_rebuilding() {
    let fixture = Fixture::new("explicit-integrity-failure");
    let index = fixture.index();
    let connection = initialize_index(
        &index,
        INDEX_INITIALIZATION_BUDGET,
        &crate::retrieval::OperationCancellation::none(),
    )
    .unwrap();
    connection.execute_batch("CREATE TABLE broken_fixture(value TEXT); INSERT INTO broken_fixture VALUES (NULL); PRAGMA writable_schema=ON; UPDATE sqlite_schema SET sql='CREATE TABLE broken_fixture(value TEXT NOT NULL)' WHERE name='broken_fixture';").unwrap();
    drop(connection);
    let before = fs::read(&index).unwrap();
    drop(open_index(&index).expect("ordinary identity open does not scan table data"));
    let error = validate_index_integrity(
        &index,
        INDEX_DIAGNOSTIC_BUDGET,
        &crate::retrieval::OperationCancellation::none(),
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("failed integrity validation"),
        "{error}"
    );
    assert!(error.to_string().contains("run magnesium init"));
    assert_eq!(fs::read(&index).unwrap(), before);
}

#[test]
fn empty_main_file_never_adopts_an_unowned_rollback_journal() {
    let fixture = Fixture::new("zero-main-orphan-journal");
    let index = fixture.index();
    fs::write(&index, b"").unwrap();
    let journal = index.with_file_name("source-search.sqlite-journal");
    fs::write(&journal, b"foreign orphaned rollback journal").unwrap();
    let before = fs::read(&journal).unwrap();
    let error = initialize_index(
        &index,
        Duration::from_millis(20),
        &crate::retrieval::OperationCancellation::none(),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("ownership wait budget exhausted"),
        "{error}"
    );
    assert!(fs::read(&index).unwrap().is_empty());
    assert_eq!(fs::read(&journal).unwrap(), before);
}

#[test]
fn ordinary_source_index_open_never_creates_a_missing_cache() {
    let fixture = Fixture::new("no-create-open");
    assert!(open_index(&fixture.index()).is_err());
    assert!(!fixture.index().exists());
}

#[test]
fn concurrent_source_index_initialization_creates_one_final_schema() {
    let fixture = Fixture::new("concurrent-initialization");
    let gate = std::sync::Arc::new(std::sync::Barrier::new(8));
    std::thread::scope(|scope| {
        let mut workers = Vec::new();
        for _ in 0..8 {
            let gate = gate.clone();
            let index = fixture.index();
            workers.push(scope.spawn(move || {
                gate.wait();
                drop(
                    initialize_index(
                        &index,
                        Duration::from_secs(10),
                        &crate::retrieval::OperationCancellation::none(),
                    )
                    .unwrap(),
                );
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }
    });
    let connection = open_index(&fixture.index()).unwrap();
    let generation: i64 = connection
        .query_row("SELECT generation FROM source_index_metadata", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(generation, 0);
    validate_index_integrity(
        &fixture.index(),
        INDEX_DIAGNOSTIC_BUDGET,
        &crate::retrieval::OperationCancellation::none(),
    )
    .unwrap();
}

#[test]
fn source_index_creation_and_integrity_have_explicit_budgets_and_cancellation() {
    let fixture = Fixture::new("explicit-index-budgets");
    let cancellation = crate::retrieval::OperationCancellation::none();
    let error = initialize_index(&fixture.index(), Duration::ZERO, &cancellation).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("initialization budget exhausted")
    );
    assert!(!fixture.index().exists());
    drop(initialize_index(&fixture.index(), INDEX_INITIALIZATION_BUDGET, &cancellation).unwrap());
    let (result, statements) = sql_trace::record(|| {
        validate_index_integrity(&fixture.index(), INDEX_DIAGNOSTIC_BUDGET, &cancellation)
    });
    result.unwrap();
    assert_eq!(
        statements
            .iter()
            .filter(|sql| sql.starts_with("PRAGMA quick_check"))
            .count(),
        1
    );
    assert!(
        validate_index_integrity(&fixture.index(), Duration::ZERO, &cancellation)
            .unwrap_err()
            .to_string()
            .contains("integrity diagnostic budget exhausted")
    );
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    assert!(matches!(
        validate_index_integrity(
            &fixture.index(),
            INDEX_DIAGNOSTIC_BUDGET,
            &crate::retrieval::OperationCancellation::with_flag(flag)
        ),
        Err(ManifestError::Cancelled)
    ));
}

#[test]
fn running_source_integrity_diagnostic_observes_cancellation() {
    let fixture = Fixture::new("cancel-integrity-progress");
    let connection = initialize_index(
        &fixture.index(),
        INDEX_INITIALIZATION_BUDGET,
        &crate::retrieval::OperationCancellation::none(),
    )
    .unwrap();
    connection.execute_batch("CREATE TABLE diagnostic_fixture(value TEXT); WITH RECURSIVE numbers(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM numbers WHERE n<2000) INSERT INTO diagnostic_fixture SELECT printf('%0100d', n) FROM numbers;").unwrap();
    drop(connection);
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let trigger = flag.clone();
    let (result, _) = sql_trace::record_with_hook(
        move |sql| {
            if sql.starts_with("PRAGMA quick_check") {
                trigger.store(true, Ordering::Relaxed);
            }
        },
        || {
            validate_index_integrity(
                &fixture.index(),
                INDEX_DIAGNOSTIC_BUDGET,
                &crate::retrieval::OperationCancellation::with_flag(flag),
            )
        },
    );
    assert!(
        matches!(result, Err(ManifestError::Cancelled)),
        "{result:?}"
    );
}

#[test]
fn source_candidate_writes_take_ownership_before_fts_maintenance() {
    let fixture = Fixture::new("candidate-write-ownership");
    fixture.write("lesson.md", b"source candidate transaction fixture\n");
    reconcile_manifest(&fixture.root, &fixture.index()).unwrap();
    let (result, statements) = sql_trace::record(|| {
        extract_source(&fixture.root, &fixture.index(), Path::new("lesson.md"))
    });
    result.unwrap();
    let starts = statements
        .iter()
        .filter(|sql| sql.starts_with("BEGIN"))
        .collect::<Vec<_>>();
    assert_eq!(
        starts,
        ["BEGIN IMMEDIATE"],
        "FTS DELETE can read before writing; acquire ownership before its snapshot: {statements:#?}"
    );
}

#[test]
fn semantic_chunks_persist_heading_and_code_structure_with_exact_provenance() {
    let fixture = Fixture::new("semantic-chunks");
    fixture.write("guide.md", b"# Overview\nintro\n## Details\nmechanism\n");
    fixture.write(
            "worker.py",
            b"from helpers import normalize\n\ndef clean(value):\n    return normalize(value)\n\nclass Runner:\n    def run(self, value):\n        return clean(value)\n",
        );
    fixture.write(
            "store.ts",
            b"import { parse } from './parse';\nexport function load(input: string) {\n  return parse(input);\n}\n",
        );
    fixture.write("long.txt", &vec![b'x'; 20_000]);
    let index = fixture.index();
    reconcile_manifest(&fixture.root, &index).expect("reconcile structured sources");

    let markdown = extract_source(&fixture.root, &index, Path::new("guide.md"))
        .expect("chunk Markdown headings");
    assert_eq!(markdown.len(), 2);
    assert_eq!(markdown[0].path, "guide.md");
    assert_eq!(markdown[0].chunk_kind, ChunkKind::Heading);
    assert_eq!(markdown[0].structural_path.as_deref(), Some("Overview"));
    assert_eq!((markdown[0].anchor_start, markdown[0].anchor_end), (1, 2));
    assert_eq!(
        markdown[1].structural_path.as_deref(),
        Some("Overview > Details")
    );
    assert_eq!((markdown[1].anchor_start, markdown[1].anchor_end), (3, 4));

    let python = extract_source(&fixture.root, &index, Path::new("worker.py"))
        .expect("chunk Python symbols");
    let clean = python
        .iter()
        .find(|chunk| chunk.structural_path.as_deref() == Some("clean"))
        .expect("persist Python function");
    assert_eq!(clean.language.as_deref(), Some("python"));
    assert_eq!((clean.anchor_start, clean.anchor_end), (3, 4));
    assert!(clean.imports.is_empty());
    assert!(
        python
            .iter()
            .flat_map(|chunk| &chunk.imports)
            .any(|source_import| source_import.module == "helpers")
    );
    assert!(
        clean
            .references
            .iter()
            .any(|reference| reference.name == "normalize")
    );
    let runner = python
        .iter()
        .find(|chunk| chunk.structural_path.as_deref() == Some("Runner"))
        .expect("persist Python class");
    assert!(
        runner
            .definitions
            .iter()
            .any(|definition| definition.name == "run" && definition.kind == "function")
    );

    let typescript = extract_source(&fixture.root, &index, Path::new("store.ts"))
        .expect("chunk TypeScript symbols");
    let load = typescript
        .iter()
        .find(|chunk| chunk.structural_path.as_deref() == Some("load"))
        .expect("persist TypeScript function");
    assert_eq!(load.language.as_deref(), Some("typescript"));
    assert!(load.imports.is_empty());
    assert!(
        typescript
            .iter()
            .flat_map(|chunk| &chunk.imports)
            .any(|source_import| source_import.module == "./parse")
    );
    assert!(
        load.references
            .iter()
            .any(|reference| reference.name == "parse")
    );

    let long = extract_source(&fixture.root, &index, Path::new("long.txt"))
        .expect("persist bounded pieces of one long line");
    assert_eq!(long.len(), 2);
    assert!(long.iter().all(|chunk| {
        chunk.anchor_start == 1 && chunk.anchor_end == 1 && chunk.body.len() <= 16 * 1_024
    }));

    let concept_tables: i64 = Connection::open(index)
        .expect("open rebuildable source index")
        .query_row(
            "SELECT count(*) FROM sqlite_schema
                 WHERE type = 'table' AND name LIKE '%concept%'",
            [],
            |row| row.get(0),
        )
        .expect("prove chunking has no concept table");
    assert_eq!(concept_tables, 0);
}

#[test]
fn renaming_one_symbol_preserves_unaffected_chunk_identity() {
    let fixture = Fixture::new("stable-symbols");
    fixture.write(
        "module.py",
        b"def alpha(value):\n    return value + 1\n\ndef beta(value):\n    return value * 2\n",
    );
    let index = fixture.index();
    reconcile_manifest(&fixture.root, &index).expect("reconcile original module");
    let before = extract_source(&fixture.root, &index, Path::new("module.py"))
        .expect("extract original symbols");
    let alpha = before
        .iter()
        .find(|chunk| chunk.structural_path.as_deref() == Some("alpha"))
        .expect("alpha chunk")
        .candidate_id
        .clone();
    let beta = before
        .iter()
        .find(|chunk| chunk.structural_path.as_deref() == Some("beta"))
        .expect("beta chunk")
        .candidate_id
        .clone();

    fixture.write(
        "module.py",
        b"def gamma(value):\n    return value + 1\n\ndef beta(value):\n    return value * 2\n",
    );
    reconcile_manifest(&fixture.root, &index).expect("reconcile renamed symbol");
    let after = extract_source(&fixture.root, &index, Path::new("module.py"))
        .expect("extract renamed symbols");
    let gamma = after
        .iter()
        .find(|chunk| chunk.structural_path.as_deref() == Some("gamma"))
        .expect("gamma chunk");
    let beta_after = after
        .iter()
        .find(|chunk| chunk.structural_path.as_deref() == Some("beta"))
        .expect("unchanged beta chunk");
    assert_ne!(gamma.candidate_id, alpha);
    assert_eq!(beta_after.candidate_id, beta);
    let stale_alpha: (String, Option<String>) = Connection::open(index)
        .expect("open source index")
        .query_row(
            "SELECT status, body FROM source_candidates WHERE candidate_id = ?1",
            [alpha],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read stale renamed symbol");
    assert_eq!(stale_alpha, ("stale".into(), None));
}

#[test]
fn unchanged_reconciliation_skips_content_and_changes_stale_prior_anchors() {
    let fixture = Fixture::new("incremental");
    fixture.write("lesson.md", b"alpha\nbeta\n");
    fixture.write("duplicate.md", b"alpha\nbeta\n");
    fixture.write("moving.txt", b"move me\n");
    fixture.write("deleted.rs", b"fn old() {}\n");
    fixture.write(".env", b"TOKEN=secret\n");
    let canonical = fixture
        .root
        .join(crate::workspace::CONTROL_DIR)
        .join("state.sqlite");
    fixture.write(
        &format!("{}/state.sqlite", crate::workspace::CONTROL_DIR),
        b"canonical sentinel",
    );
    let index = fixture.index();

    let first = reconcile_manifest(&fixture.root, &index).expect("first reconciliation");
    assert_eq!(first.content_reads, 4);
    let lesson =
        extract_source(&fixture.root, &index, Path::new("lesson.md")).expect("extract text source");
    let duplicate = extract_source(&fixture.root, &index, Path::new("duplicate.md"))
        .expect("extract duplicate-content source");
    assert_eq!(lesson.len(), 1);
    assert_eq!(lesson[0].anchor_kind, AnchorKind::Line);
    assert_eq!((lesson[0].anchor_start, lesson[0].anchor_end), (1, 2));
    assert_eq!(lesson[0].body, "alpha\nbeta\n");
    assert_ne!(lesson[0].candidate_id, duplicate[0].candidate_id);
    let metadata: (String, String, i64, bool, String, String) = Connection::open(&index)
        .expect("open source index")
        .query_row(
            "SELECT display_path, source_kind, file_size,
                        modified_unix_ns IS NOT NULL, content_sha256, index_status
                 FROM source_manifest WHERE display_path = 'lesson.md'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .expect("read complete source metadata");
    assert_eq!(metadata.0, "lesson.md");
    assert_eq!(metadata.1, "text");
    assert_eq!(metadata.2, 11);
    assert!(metadata.3);
    assert_eq!(metadata.4.len(), 64);
    assert_eq!(metadata.5, "indexed");
    let move_id = manifest_id(&index, "moving.txt");

    let second = reconcile_manifest(&fixture.root, &index).expect("unchanged reconciliation");
    assert_eq!(second.content_reads, 0);
    assert_eq!(
        second.changed + second.created + second.moved + second.deleted,
        0
    );

    fixture.write("lesson.md", b"alpha changed\nbeta\ngamma\n");
    fs::rename(
        fixture.root.join("moving.txt"),
        fixture.root.join("moved.txt"),
    )
    .expect("move source");
    fs::remove_file(fixture.root.join("deleted.rs")).expect("delete source");
    let third = reconcile_manifest(&fixture.root, &index).expect("changed reconciliation");
    assert_eq!(third.changed, 1);
    assert_eq!(third.moved, 1);
    assert_eq!(third.deleted, 1);
    assert_eq!(manifest_id(&index, "moved.txt"), move_id);
    let connection = Connection::open(&index).expect("open source index");
    let stale: (String, Option<String>) = connection
        .query_row(
            "SELECT status, body FROM source_candidates WHERE candidate_id = ?1",
            [&lesson[0].candidate_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read stale anchor");
    assert_eq!(stale, ("stale".to_owned(), None));
    assert_eq!(
        fs::read(canonical).expect("read canonical sentinel"),
        b"canonical sentinel"
    );
    let sensitive: (Option<String>, Option<String>, String) = connection
        .query_row(
            "SELECT content_sha256, sensitivity_class, index_status
                 FROM source_manifest WHERE display_path = '.env'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read sensitive manifest row");
    assert_eq!(
        sensitive,
        (None, Some("environment".to_owned()), "excluded".to_owned())
    );
}

#[test]
fn incompatible_source_caches_are_rejected_byte_preserved_with_rebuild_guidance() {
    for (application, version) in [
        (INDEX_APPLICATION_ID, 1),
        (INDEX_APPLICATION_ID, 5),
        (INDEX_APPLICATION_ID, 7),
        (INDEX_APPLICATION_ID, 8),
        (INDEX_APPLICATION_ID, 9),
        (INDEX_APPLICATION_ID, 11),
        (27, INDEX_SCHEMA_VERSION),
        (0, 0),
    ] {
        let fixture = Fixture::new("incompatible-cache");
        let index = fixture.index();
        let connection = Connection::open(&index).unwrap();
        connection.execute_batch("CREATE TABLE foreign_sentinel(value TEXT); INSERT INTO foreign_sentinel VALUES ('preserve fixture-old-index-secret');").unwrap();
        connection
            .pragma_update(None, "application_id", application)
            .unwrap();
        connection
            .pragma_update(None, "user_version", version)
            .unwrap();
        drop(connection);
        let original = fs::read(&index).unwrap();
        for error in [
            open_index(&index).unwrap_err(),
            open_index_for_write(&index).unwrap_err(),
            initialize_index(
                &index,
                INDEX_INITIALIZATION_BUDGET,
                &crate::retrieval::coordinator::OperationCancellation::none(),
            )
            .unwrap_err(),
        ] {
            assert!(error.to_string().contains("run magnesium init"), "{error}");
            assert!(error.to_string().contains(&format!(
                "Do not move or delete {}/state.sqlite",
                crate::workspace::CONTROL_DIR
            )));
        }
        assert_eq!(fs::read(&index).unwrap(), original);
        assert_eq!(
            fs::read_dir(index.parent().unwrap()).unwrap().count(),
            1,
            "incompatible open must not create journals or sidecars"
        );
    }
}

#[test]
fn cancellation_during_pdf_extraction_aborts_before_storing_candidates() {
    // The production token must reach the extraction stage: cancellation that
    // starts after extraction begins aborts before candidates are stored and
    // is not recorded as a source defect.
    let fixture = Fixture::new("pdf-cancelled-extraction");
    fixture.write("book.pdf", &pdf_fixture());
    let index = fixture.index();
    reconcile_manifest(&fixture.root, &index).expect("reconcile PDF source");

    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cancellation =
        crate::retrieval::OperationCancellation::with_flag(std::sync::Arc::clone(&flag));
    let mut after_open = |_: &Path| Ok(());
    let error = extract_source_with_hooks(
        &fixture.root,
        &index,
        Path::new("book.pdf"),
        Some(Box::new(move |_bytes| {
            // Extraction has started; cancellation begins mid-extraction.
            flag.store(true, Ordering::Relaxed);
            Ok(vec!["extracted page".to_owned()])
        })),
        Some(&cancellation),
        &mut after_open,
    )
    .expect_err("cancelled extraction must abort");
    assert!(
        matches!(error, ManifestError::Cancelled),
        "unexpected extraction cancellation error: {error}"
    );
    let connection = Connection::open(&index).expect("open source index");
    let candidates: i64 = connection
        .query_row(
            "SELECT count(*) FROM source_candidates c
                 JOIN source_manifest m ON m.manifest_id = c.manifest_id
                 WHERE m.display_path = 'book.pdf'",
            [],
            |row| row.get(0),
        )
        .expect("count cancelled PDF candidates");
    assert_eq!(
        candidates, 0,
        "cancelled extraction must store no candidates"
    );
    let status: String = connection
        .query_row(
            "SELECT index_status FROM source_manifest WHERE display_path = 'book.pdf'",
            [],
            |row| row.get(0),
        )
        .expect("read cancelled PDF status");
    assert_ne!(
        status, "failed",
        "cancellation must not be recorded as a source defect"
    );
}

#[test]
fn private_pdf_process_preserves_pages_rejects_malformed_input_and_times_out() {
    let fixture = Fixture::new("pdf");
    let pdf = pdf_fixture();
    fixture.write("book.pdf", &pdf);
    let index = fixture.index();
    reconcile_manifest(&fixture.root, &index).expect("reconcile PDF source");
    let pages = extract_source_with_pdf(&fixture.root, &index, Path::new("book.pdf"), |bytes| {
        run_pdf_command(pdf_helper("extract"), bytes, Duration::from_secs(2))
    })
    .expect("extract PDF in private process");
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].anchor_kind, AnchorKind::Page);
    assert_eq!(pages[0].chunk_kind, ChunkKind::Page);
    assert_eq!(pages[0].structural_path.as_deref(), Some("page 1"));
    assert_eq!((pages[0].anchor_start, pages[0].anchor_end), (1, 1));
    assert_eq!((pages[1].anchor_start, pages[1].anchor_end), (2, 2));
    assert!(pages[0].body.contains("Independent alpha anchor"));
    assert!(pages[1].body.contains("Independent beta anchor"));

    for malformed in [b"not a PDF".as_slice(), &pdf[..pdf.len() / 2]] {
        let error = run_pdf_command(pdf_helper("extract"), malformed, Duration::from_secs(2))
            .expect_err("reject malformed PDF");
        assert!(matches!(
            error,
            ManifestError::Pdf(_) | ManifestError::PdfContent(_)
        ));
    }

    fixture.write("malformed.pdf", b"not a PDF");
    reconcile_manifest(&fixture.root, &index).expect("reconcile malformed PDF source");
    extract_source_with_pdf(&fixture.root, &index, Path::new("malformed.pdf"), |bytes| {
        run_pdf_command(pdf_helper("extract"), bytes, Duration::from_secs(2))
    })
    .expect_err("record malformed PDF failure");
    let failed: (String, i64) = Connection::open(&index)
        .expect("open source index")
        .query_row(
            "SELECT index_status,
                        (SELECT count(*) FROM source_candidates c
                         WHERE c.manifest_id = m.manifest_id)
                 FROM source_manifest m WHERE display_path = 'malformed.pdf'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read malformed PDF status");
    assert_eq!(failed, ("failed".to_owned(), 0));

    let timeout = run_pdf_command(pdf_helper("sleep"), b"", Duration::from_millis(40))
        .expect_err("kill expired PDF helper");
    assert!(matches!(timeout, ManifestError::PdfTimeout));
}

#[test]
fn extracted_pdf_secret_is_excluded_but_ordinary_pdf_remains_searchable() {
    let fixture = Fixture::new("pdf-sensitive");
    fixture.write("compressed.pdf", &compressed_secret_pdf_fixture());
    fixture.write("ordinary.pdf", &pdf_fixture());
    let index = fixture.index();
    reconcile_manifest(&fixture.root, &index).expect("reconcile PDF sensitivity fixture");
    let secret = extract_source_with_pdf(
        &fixture.root,
        &index,
        Path::new("compressed.pdf"),
        |bytes| run_pdf_command(pdf_helper("extract"), bytes, Duration::from_secs(2)),
    )
    .expect("exclude extracted PDF secret");
    assert!(secret.is_empty());
    let connection = Connection::open(&index).expect("open sensitive PDF index");
    let secret_state: (String, Option<String>, Option<String>) = connection
        .query_row(
            "SELECT source_kind, sensitivity_class, content_sha256
                 FROM source_manifest WHERE display_path = 'compressed.pdf'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read sensitive PDF state");
    let secret_candidates: i64 = connection
        .query_row(
            "SELECT count(*) FROM source_candidates c
                 JOIN source_manifest m ON m.manifest_id = c.manifest_id
                 WHERE m.display_path = 'compressed.pdf'",
            [],
            |row| row.get(0),
        )
        .expect("count sensitive PDF candidates");
    assert_eq!(
        (
            secret_state.0,
            secret_state.1,
            secret_state.2,
            secret_candidates
        ),
        (
            "excluded".to_owned(),
            Some("credential".to_owned()),
            None,
            0
        )
    );

    let ordinary =
        extract_source_with_pdf(&fixture.root, &index, Path::new("ordinary.pdf"), |bytes| {
            run_pdf_command(pdf_helper("extract"), bytes, Duration::from_secs(2))
        })
        .expect("extract ordinary PDF");
    assert!(
        ordinary
            .iter()
            .any(|candidate| { candidate.body.contains("Independent alpha anchor") })
    );
    for entry in fs::read_dir(index.parent().expect("PDF index parent")).expect("read PDF index") {
        let path = entry.expect("read PDF index entry").path();
        if path.is_file() {
            assert!(
                !fs::read(path)
                    .expect("read PDF index artifact")
                    .windows(b"fixture-pdf-secret".len())
                    .any(|window| window == b"fixture-pdf-secret")
            );
        }
    }
}

#[test]
fn production_extract_source_reconciles_and_extracts_real_pdf() {
    let fixture = Fixture::new("pdf-production");
    let pdf = pdf_fixture();
    fixture.write("real_book.pdf", &pdf);
    let index = fixture.index();
    reconcile_manifest(&fixture.root, &index).expect("reconcile real PDF source");
    let pages = extract_source(&fixture.root, &index, Path::new("real_book.pdf"))
        .expect("extract real PDF through production extract_source");
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].anchor_kind, AnchorKind::Page);
    assert_eq!(pages[0].chunk_kind, ChunkKind::Page);
    assert_eq!(pages[0].structural_path.as_deref(), Some("page 1"));
    assert!(pages[0].body.contains("Independent alpha anchor"));
    assert!(pages[1].body.contains("Independent beta anchor"));

    let scratch_dir = fixture
        .root
        .join(crate::workspace::CONTROL_DIR)
        .join("scratch");
    if scratch_dir.is_dir() {
        let remaining_tmp = fs::read_dir(&scratch_dir)
            .expect("read scratch")
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "tmp"))
            .count();
        assert_eq!(remaining_tmp, 0, "RAII guard must clean up PDF snapshots");
    }
}

#[test]
fn pdf_page_and_output_bounds_are_explicit() {
    let too_many = vec![String::new(); MAX_PDF_PAGES + 1];
    assert!(validate_pdf_pages(too_many).is_err());
    assert!(validate_pdf_pages(vec!["x".repeat(MAX_PDF_TEXT_BYTES + 1)]).is_err());
}

#[test]
fn content_secret_is_excluded_without_hash_or_candidate() {
    let fixture = Fixture::new("sensitive-content");
    fixture.write(
        "notes.md",
        b"Safe heading\nAWS_SECRET_ACCESS_KEY=fixture-secret\n",
    );
    let index = fixture.index();

    let report = reconcile_manifest(&fixture.root, &index).expect("reconcile secret fixture");

    assert!(report.excluded >= 1);
    let connection = open_index(&index).expect("open source index");
    let sensitive: (Option<String>, Option<String>, String, i64) = connection
        .query_row(
            "SELECT content_sha256, sensitivity_class, index_status,
                        (SELECT count(*) FROM source_candidates candidate
                         WHERE candidate.manifest_id = source_manifest.manifest_id)
                 FROM source_manifest WHERE display_path = 'notes.md'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("read sensitive manifest");
    assert_eq!(
        sensitive,
        (
            None,
            Some("credential".to_owned()),
            "excluded".to_owned(),
            0
        )
    );
}

#[test]
fn content_secret_marker_split_across_hash_chunks_is_excluded() {
    let fixture = Fixture::new("sensitive-split");
    let mut bytes = vec![b'a'; 64 * 1024 - 8];
    bytes.extend_from_slice(b"AWS_SECRET_");
    bytes.extend_from_slice(b"ACCESS_KEY=split-secret\n");
    fixture.write("split.txt", &bytes);
    let index = fixture.index();

    reconcile_manifest(&fixture.root, &index).expect("reconcile split secret fixture");

    let connection = open_index(&index).expect("open source index");
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM source_manifest
                     WHERE display_path = 'split.txt' AND index_status = 'excluded'
                       AND content_sha256 IS NULL",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("read split secret status"),
        1
    );
}

#[test]
fn assignment_secret_markers_are_excluded_without_matching_harmless_words() {
    let fixture = Fixture::new("sensitive-assignments");
    fixture.write(
        "config.txt",
        b"Documentation mentions \"password\" as a field name.\n{\"password\":\"hunter2\"}\n",
    );
    fixture.write(
            "documentation.txt",
            b"tokenization: describes lexical splitting\nDocumentation mentions \"password\" as a field name without an assignment.\n",
        );
    let index = fixture.index();

    reconcile_manifest(&fixture.root, &index).expect("reconcile assignment fixture");

    let connection = open_index(&index).expect("open source index");
    let state: (String, Option<String>, i64) = connection
        .query_row(
            "SELECT index_status, sensitivity_class,
                        (SELECT count(*) FROM source_candidates candidate
                         WHERE candidate.manifest_id = source_manifest.manifest_id)
                 FROM source_manifest WHERE display_path = 'config.txt'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read assignment marker status");
    assert_eq!(
        state,
        ("excluded".to_owned(), Some("credential".to_owned()), 0)
    );
    let documentation = extract_source(&fixture.root, &index, Path::new("documentation.txt"))
        .expect("extract harmless assignment documentation");
    assert_eq!(documentation.len(), 1);
    assert!(documentation[0].body.contains("without an assignment"));
}

#[cfg(unix)]
#[test]
fn reconciliation_and_extraction_reject_symlink_swaps_after_open() {
    let fixture = Fixture::new("symlink-source");
    fixture.write("lesson.md", b"safe\n");
    let index = fixture.index();
    let outside = fixture.root.with_extension("outside.md");
    fs::write(&outside, b"outside secret\n").expect("write outside file");
    let source =
        fs::canonicalize(fixture.root.join("lesson.md")).expect("resolve fixture source path");
    let mut swap_after_open = |opened: &Path| {
        assert_eq!(opened, source);
        fs::remove_file(&source).expect("remove opened source");
        std::os::unix::fs::symlink(&outside, &source).expect("replace opened source with symlink");
        Ok(())
    };
    let error = reconcile_manifest_with_hook(&fixture.root, &index, &mut swap_after_open)
        .expect_err("reject reconciliation source swap");
    assert!(error.to_string().contains("changed while opening"));
    let external_hash_count: i64 = Connection::open(&index)
        .expect("open source index")
        .query_row(
            "SELECT count(*) FROM source_manifest WHERE content_sha256 = ?1",
            [sha256_hex(b"outside secret\n")],
            |row| row.get(0),
        )
        .expect("count external hashes");
    assert_eq!(external_hash_count, 0);

    fs::remove_file(&source).expect("remove substituted symlink");
    fixture.write("lesson.md", b"safe\n");
    reconcile_manifest(&fixture.root, &index).expect("reconcile safe source");
    let mut swap_after_open = |opened: &Path| {
        assert_eq!(opened, source);
        fs::remove_file(&source).expect("remove opened source");
        std::os::unix::fs::symlink(&outside, &source).expect("replace opened source with symlink");
        Ok(())
    };
    let error = extract_source_with_hooks(
        &fixture.root,
        &index,
        Path::new("lesson.md"),
        None,
        None,
        &mut swap_after_open,
    )
    .expect_err("reject extraction source swap");
    assert!(error.to_string().contains("changed while opening"));
    let state: (String, i64, i64) = Connection::open(&index)
        .expect("open source index")
        .query_row(
            "SELECT index_status,
                        (SELECT count(*) FROM source_candidates),
                        (SELECT count(*) FROM source_manifest WHERE content_sha256 = ?1)
                 FROM source_manifest WHERE display_path = 'lesson.md'",
            [sha256_hex(b"outside secret\n")],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read source state after extraction swap");
    assert_eq!(state, ("pending".to_owned(), 0, 0));

    let substituted = Fixture::new("symlink-index");
    let external_index = substituted.root.with_extension("outside.sqlite");
    fs::write(&external_index, b"external sentinel").expect("write external index");
    std::os::unix::fs::symlink(&external_index, substituted.index())
        .expect("replace index with symlink");
    let error = reconcile_manifest(&substituted.root, &substituted.index())
        .expect_err("reject index symlink");
    assert!(error.to_string().contains("symbolic link"));
    assert_eq!(
        fs::read(&external_index).expect("read external index"),
        b"external sentinel"
    );
    fs::remove_file(source).expect("remove substituted source symlink");
    fs::remove_file(outside).expect("remove outside source");
    fs::remove_file(external_index).expect("remove external index");
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn manifest_keys_preserve_distinct_non_utf8_paths() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let fixture = Fixture::new("native-paths");
    for bytes in [b"source-\xfe.txt".to_vec(), b"source-\xff.txt".to_vec()] {
        fs::write(fixture.root.join(OsString::from_vec(bytes)), b"same\n")
            .expect("write non-UTF-8 source");
    }
    let index = fixture.index();
    let report = reconcile_manifest(&fixture.root, &index).expect("reconcile native paths");
    assert_eq!(report.content_reads, 2);
    let counts: (i64, i64) = Connection::open(index)
        .expect("open source index")
        .query_row(
            "SELECT count(*), count(DISTINCT path_key)
                 FROM source_manifest WHERE source_kind = 'text'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("count native manifest keys");
    assert_eq!(counts, (2, 2));
}

#[test]
#[ignore = "spawned by private_pdf_process_preserves_pages_rejects_malformed_input_and_times_out"]
fn pdf_process_helper() {
    match std::env::var("MAGNESIUM_PDF_TEST_HELPER").as_deref() {
        Ok("extract") => {
            let status = run_pdf_extractor();
            std::process::exit(if status == std::process::ExitCode::SUCCESS {
                0
            } else {
                1
            });
        }
        Ok("failed-content-reply") => {
            use std::io::Write;
            let reply = serde_json::to_vec(&PdfWorkerReply::ContentFailure {
                diagnostic: "parser rejected content".into(),
            })
            .unwrap();
            let mut output = std::io::stdout().lock();
            writeln!(output, "{PDF_PROTOCOL_MARKER}{}", reply.len()).unwrap();
            output.write_all(&reply).unwrap();
            output.flush().unwrap();
            std::process::exit(1);
        }
        Ok("sleep") => {
            std::thread::sleep(Duration::from_secs(5));
            std::process::exit(0);
        }
        _ => {}
    }
}

fn pdf_helper(mode: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("current test executable"));
    command
        .args([
            "--ignored",
            "--exact",
            "retrieval::manifest::tests::pdf_process_helper",
            "--nocapture",
        ])
        .env("MAGNESIUM_PDF_TEST_HELPER", mode);
    command
}

fn manifest_id(index: &Path, display_path: &str) -> i64 {
    Connection::open(index)
        .expect("open index")
        .query_row(
            "SELECT manifest_id FROM source_manifest WHERE display_path = ?1 AND present = 1",
            [display_path],
            |row| row.get(0),
        )
        .expect("read manifest ID")
}

fn pdf_fixture() -> Vec<u8> {
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources_id = document.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let page_ids = ["Independent alpha anchor", "Independent beta anchor"].map(|text| {
        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), 18.into()]),
                Operation::new("Td", vec![72.into(), 720.into()]),
                Operation::new("Tj", vec![Object::string_literal(text)]),
                Operation::new("ET", vec![]),
            ],
        };
        let content_id = document.add_object(Stream::new(
            dictionary! {},
            content.encode().expect("encode PDF text stream"),
        ));
        document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        })
    });
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => page_ids.map(Object::Reference).to_vec(),
            "Count" => 2,
        }),
    );
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    document.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).expect("serialize PDF fixture");
    bytes
}

fn compressed_secret_pdf_fixture() -> Vec<u8> {
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources_id = document.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let compressed = vec![
        120, 156, 115, 10, 81, 208, 119, 51, 84, 48, 180, 80, 8, 73, 83, 48, 55, 2, 34, 3, 133,
        144, 20, 5, 141, 130, 196, 226, 226, 242, 252, 162, 20, 43, 133, 180, 204, 138, 146, 210,
        162, 84, 221, 130, 148, 52, 221, 226, 212, 228, 162, 212, 18, 77, 133, 144, 44, 5, 215, 16,
        0, 231, 79, 17, 208,
    ];
    let content_id = document.add_object(Stream::new(
        dictionary! { "Filter" => "FlateDecode" },
        compressed,
    ));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    });
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1,
        }),
    );
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    document.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    document
        .save_to(&mut bytes)
        .expect("serialize compressed PDF");
    bytes
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let sequence = NEXT_ROOT.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test clock after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "magnesium-manifest-{}-{name}-{nanos}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create fixture root");
        fs::create_dir_all(root.join(crate::workspace::CONTROL_DIR).join("index"))
            .expect("create Magnesium index directory");
        Self { root }
    }

    fn index(&self) -> PathBuf {
        self.root
            .join(crate::workspace::CONTROL_DIR)
            .join("index/source-search.sqlite")
    }

    fn write(&self, relative: &str, contents: &[u8]) {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().expect("fixture file parent"))
            .expect("create fixture parent");
        fs::write(path, contents).expect("write fixture file");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

use crate::retrieval::discover_root;

fn reconcile_manifest(
    workspace_root: &Path,
    index_path: &Path,
) -> Result<ManifestReport, ManifestError> {
    let mut after_open = |_: &Path| Ok(());
    reconcile_manifest_with_hook(workspace_root, index_path, &mut after_open)
}

fn reconcile_manifest_with_hook(
    workspace_root: &Path,
    index_path: &Path,
    after_open: &mut impl FnMut(&Path) -> Result<(), ManifestError>,
) -> Result<ManifestReport, ManifestError> {
    let discovery = discover_root(workspace_root)?;
    reconcile_discovery(index_path, &discovery, after_open)
}

fn reconcile_discovery(
    index_path: &Path,
    discovery: &DiscoveryReport,
    after_open: &mut impl FnMut(&Path) -> Result<(), ManifestError>,
) -> Result<ManifestReport, ManifestError> {
    let mut connection = initialize_index(
        index_path,
        INDEX_INITIALIZATION_BUDGET,
        &crate::retrieval::coordinator::OperationCancellation::none(),
    )?;
    let transaction = connection
        .transaction()
        .map_err(|source| ManifestError::Sqlite {
            action: "start source manifest transaction",
            path: index_path.to_path_buf(),
            source,
        })?;
    let generation: i64 = transaction
        .query_row(
            "SELECT generation + 1 FROM source_index_metadata WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|source| sqlite(index_path, "read source manifest generation", source))?;
    let cancellation = crate::retrieval::OperationCancellation::none();
    let existing = load_existing(&transaction, index_path, &cancellation)?;
    let current_keys = discovery
        .entries
        .iter()
        .map(|entry| native_path_key(&entry.relative_path))
        .collect::<HashSet<_>>();
    let mut progress = ReconcileProgress {
        moved_ids: HashSet::new(),
        moves: super::MovePolicy::Any,
        cancellation: &cancellation,
        report: empty_report(generation),
        after_open,
    };

    for entry in &discovery.entries {
        reconcile_entry(
            &transaction,
            index_path,
            discovery,
            entry,
            &existing,
            &current_keys,
            &mut progress,
        )?;
    }

    for (path_key, record) in &existing {
        if record.present
            && !current_keys.contains(path_key)
            && !progress.moved_ids.contains(&record.manifest_id)
        {
            stale_candidates(&transaction, record.manifest_id, index_path)?;
            transaction
                .execute(
                    "UPDATE source_manifest
                     SET index_status = 'deleted', present = 0, observed_generation = ?2,
                         last_error = NULL
                     WHERE manifest_id = ?1",
                    params![record.manifest_id, generation],
                )
                .map_err(|source| sqlite(index_path, "mark deleted manifest source", source))?;
            progress.report.deleted += 1;
        }
    }
    transaction
        .execute(
            "DELETE FROM source_reconciliation_state WHERE singleton = 1",
            [],
        )
        .map_err(|source| sqlite(index_path, "cancel bounded reconciliation cursor", source))?;
    transaction
        .execute(
            "UPDATE source_index_metadata SET generation = ?1 WHERE singleton = 1",
            [generation],
        )
        .map_err(|source| sqlite(index_path, "advance source manifest generation", source))?;
    transaction
        .commit()
        .map_err(|source| sqlite(index_path, "commit source manifest", source))?;
    Ok(progress.report)
}

#[test]
fn open_workspace_source_rejects_traversal_and_symlink_before_any_bytes() {
    // LEARN-206 reuses this boundary for original-byte inspection: absolute
    // paths, traversal, symlinks and non-files fail before any read, so a
    // `sources` caller can never escape the workspace through a crafted path.
    use super::source::open_workspace_source;

    let fixture = Fixture::new("open-boundary");
    fixture.write("lesson.md", b"bounded original\n");
    let mut after_open = |_: &Path| Ok(());
    for bad in [
        PathBuf::from("/etc/hostname"),
        PathBuf::from("../outside.md"),
        PathBuf::from("notes/../../outside.md"),
    ] {
        assert!(
            open_workspace_source(&fixture.root, &bad, &mut after_open).is_err(),
            "traversal must be rejected: {}",
            bad.display()
        );
    }
    assert!(
        open_workspace_source(&fixture.root, Path::new("lesson.md"), &mut after_open).is_ok(),
        "ordinary workspace-relative sources still open"
    );
}

/// Two-page PDF with empty content streams: the native text layer is
/// blank, which is exactly the image-only/scanned shape an optional
/// local converter exists for.
fn blank_pdf_fixture() -> Vec<u8> {
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let page_ids = (0..2)
        .map(|_| {
            let content = Content { operations: vec![] };
            let content_id = document.add_object(Stream::new(
                dictionary! {},
                content.encode().expect("encode blank PDF stream"),
            ));
            document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            })
        })
        .collect::<Vec<_>>();
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => page_ids.iter().map(|id| Object::Reference(*id)).collect::<Vec<_>>(),
            "Count" => 2,
        }),
    );
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    document.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    document
        .save_to(&mut bytes)
        .expect("serialize blank PDF fixture");
    bytes
}

fn current_source_for(index: &Path, display_path: &str) -> CurrentSource {
    let connection = Connection::open(index).expect("open source index");
    connection
        .query_row(
            "SELECT manifest_id, display_path, source_kind, content_sha256, index_status, source_revision
             FROM source_manifest WHERE display_path = ?1",
            [display_path],
            |row| {
                Ok(CurrentSource {
                    manifest_id: row.get(0)?,
                    display_path: row.get(1)?,
                    source_kind: row.get(2)?,
                    content_sha256: row.get(3)?,
                    index_status: row.get(4)?,
                    source_revision: row.get(5)?,
                })
            },
        )
        .expect("read manifest source")
}

/// Fake conversion worker: a shell script serving one canned framed
/// response. Configs pass explicitly; no environment is touched.
#[cfg(unix)]
struct ConversionWorker {
    dir: PathBuf,
}

#[cfg(unix)]
impl ConversionWorker {
    fn new(response: &[u8], mode: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "magnesium-conversion-{}-{name}-{nanos}",
            std::process::id(),
            name = "hook",
            nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("test clock")
                .as_nanos(),
        ));
        fs::create_dir_all(&root).expect("fake worker dir");
        let response_path = root.join("response.bin");
        fs::write(&response_path, response).expect("fake worker response");
        let script = match mode {
            "success" => format!("cat \"{}\"\n", response_path.display()),
            "sentinel" => format!(
                "touch \"{}\"\ncat \"{}\"\n",
                root.join("spawned").display(),
                response_path.display()
            ),
            _ => unreachable!("fake worker mode"),
        };
        let path = root.join("worker.sh");
        fs::write(&path, format!("#!/bin/sh\ncat >/dev/null\n{script}"))
            .expect("fake worker script");
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                .expect("fake worker executable");
        }
        Self { dir: root }
    }

    fn config(&self) -> super::convert::WorkerConfig {
        super::convert::WorkerConfig {
            executable: self.dir.join("worker.sh"),
        }
    }

    fn framed(body: &[u8]) -> Vec<u8> {
        let mut framed = format!("MAGNESIUM_DOCS_V1 {}\n", body.len()).into_bytes();
        framed.extend_from_slice(body);
        framed
    }

    fn response_for(sha: &str, texts: &[&str]) -> Vec<u8> {
        let blocks: Vec<serde_json::Value> = texts
            .iter()
            .enumerate()
            .map(|(index, text)| {
                let page = index as u64 + 1;
                serde_json::json!({"anchor_kind": "page", "anchor_start": page, "anchor_end": page, "text": text})
            })
            .collect();
        serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "method": "fake-worker",
            "method_version": "hook-test",
            "source_sha256": sha,
            "blocks": blocks,
            "coverage": "complete",
        }))
        .expect("fake worker response JSON")
    }
}

#[cfg(unix)]
impl Drop for ConversionWorker {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn blank_pdf_without_worker_keeps_the_native_result() {
    // No worker is configured in this process, so the #208 hook is inert
    // and the core install behaves exactly as before: blank pages index
    // as blank candidates with truthful unavailable coverage downstream.
    let fixture = Fixture::new("blank-pdf-native");
    fixture.write("blank.pdf", &blank_pdf_fixture());
    let index = fixture.index();
    reconcile_manifest(&fixture.root, &index).expect("reconcile blank PDF");
    let candidates = extract_source_with_cancellation(
        &fixture.root,
        &index,
        Path::new("blank.pdf"),
        &crate::retrieval::OperationCancellation::none(),
    )
    .expect("extract blank PDF");
    assert_eq!(candidates.len(), 2, "both blank pages index");
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate.body.trim().is_empty()),
        "native blank pages stay blank without a worker"
    );
}

#[test]
#[cfg(unix)]
fn blank_pdf_uses_a_configured_worker_and_maps_anchors() {
    use super::extract::try_worker_for_blank_pdf;

    let fixture = Fixture::new("blank-pdf-worker");
    let pdf = blank_pdf_fixture();
    fixture.write("blank.pdf", &pdf);
    let index = fixture.index();
    reconcile_manifest(&fixture.root, &index).expect("reconcile blank PDF");
    let source = current_source_for(&index, "blank.pdf");
    let body = ConversionWorker::response_for(&source.content_sha256, &["First", "Second"]);
    let worker = ConversionWorker::new(&ConversionWorker::framed(&body), "success");
    let connection = Connection::open(&index).expect("open source index");
    let native = Ok(vec!["".to_owned(), "  ".to_owned()]);
    let drafts = try_worker_for_blank_pdf(
        &source,
        &native,
        &fixture.root.join("blank.pdf"),
        Some(&worker.config()),
        None,
    )
    .expect("worker conversion")
    .expect("worker supplies drafts for a blank layer");
    match drafts {
        (super::extract::PdfOutcome::Drafts(drafts), Some(provenance)) => {
            assert_eq!(drafts.len(), 2);
            assert_eq!(drafts[0].body, "First");
            assert_eq!(drafts[1].body, "Second");
            // Converter identity travels beside the drafts so the derived
            // index never misattributes worker text to the native worker.
            assert_eq!(provenance.method, "fake-worker");
            assert_eq!(provenance.method_version, "hook-test");
            assert_eq!(provenance.coverage, "complete");
        }
        unexpected => panic!("worker must supply drafts with provenance: {unexpected:?}"),
    }
    drop(connection);
}

#[test]
#[cfg(unix)]
fn corrupt_worker_output_preserves_the_native_result() {
    use super::extract::try_worker_for_blank_pdf;

    let fixture = Fixture::new("blank-pdf-junk-worker");
    fixture.write("blank.pdf", &blank_pdf_fixture());
    let index = fixture.index();
    reconcile_manifest(&fixture.root, &index).expect("reconcile blank PDF");
    let source = current_source_for(&index, "blank.pdf");
    let worker = ConversionWorker::new(b"not framed at all", "success");
    let native: Result<Vec<String>, ManifestError> = Ok(vec!["".to_owned()]);
    let outcome = try_worker_for_blank_pdf(
        &source,
        &native,
        &fixture.root.join("blank.pdf"),
        Some(&worker.config()),
        None,
    )
    .expect("junk worker output must not fail extraction");
    assert!(outcome.is_none(), "corrupt output keeps the native result");
}

#[test]
#[cfg(unix)]
fn readable_pages_and_missing_workers_never_spawn() {
    use super::extract::try_worker_for_blank_pdf;

    let fixture = Fixture::new("readable-pdf-no-spawn");
    fixture.write("book.pdf", &pdf_fixture());
    let index = fixture.index();
    reconcile_manifest(&fixture.root, &index).expect("reconcile PDF");
    let source = current_source_for(&index, "book.pdf");
    let body = ConversionWorker::response_for(&source.content_sha256, &["X"]);
    let worker = ConversionWorker::new(&ConversionWorker::framed(&body), "sentinel");
    // Readable pages: the worker is never consulted.
    let native: Result<Vec<String>, ManifestError> = Ok(vec!["readable".to_owned()]);
    let outcome = try_worker_for_blank_pdf(
        &source,
        &native,
        &fixture.root.join("book.pdf"),
        Some(&worker.config()),
        None,
    )
    .expect("readable pages skip the worker");
    assert!(outcome.is_none());
    // No configured worker: blank pages still skip spawning.
    let blank: Result<Vec<String>, ManifestError> = Ok(vec!["".to_owned()]);
    let outcome =
        try_worker_for_blank_pdf(&source, &blank, &fixture.root.join("book.pdf"), None, None)
            .expect("missing worker keeps the native result");
    assert!(outcome.is_none());
    assert!(
        !worker.dir.join("spawned").exists(),
        "no worker process spawns without blank pages plus configuration"
    );
}

#[test]
#[cfg(unix)]
fn worker_text_passes_the_sensitivity_gate() {
    use super::extract::try_worker_for_blank_pdf;

    let fixture = Fixture::new("worker-sensitive-text");
    fixture.write("blank.pdf", &blank_pdf_fixture());
    let index = fixture.index();
    reconcile_manifest(&fixture.root, &index).expect("reconcile blank PDF");
    let source = current_source_for(&index, "blank.pdf");
    let body = ConversionWorker::response_for(&source.content_sha256, &["password=hunter2"]);
    let worker = ConversionWorker::new(&ConversionWorker::framed(&body), "success");
    let native: Result<Vec<String>, ManifestError> = Ok(vec!["".to_owned()]);
    let before = extraction_publication_snapshot(&index);
    let outcome = try_worker_for_blank_pdf(
        &source,
        &native,
        &fixture.root.join("blank.pdf"),
        Some(&worker.config()),
        None,
    )
    .expect("sensitive worker text is handled");
    assert!(
        matches!(
            &outcome,
            Some((super::extract::PdfOutcome::Sensitive(_), None))
        ),
        "sensitive converted text excludes the source like native secrets"
    );
    assert_eq!(
        extraction_publication_snapshot(&index),
        before,
        "worker preparation must only classify"
    );
    let Some((super::extract::PdfOutcome::Sensitive(sensitivity), None)) = outcome else {
        unreachable!()
    };
    let mut connection = Connection::open(&index).expect("reopen source index");
    super::publication::publish_extraction(
        &mut connection,
        &index,
        &source,
        &super::publication::PreparedOutcome::Sensitive(sensitivity),
        &crate::retrieval::OperationCancellation::none(),
    )
    .expect("publish current sensitive worker outcome");
    let status: String = connection
        .query_row(
            "SELECT index_status FROM source_manifest WHERE display_path = 'blank.pdf'",
            [],
            |row| row.get(0),
        )
        .expect("read excluded status");
    assert_eq!(
        status, "excluded",
        "guarded publication applies worker exclusion"
    );
}

#[test]
#[cfg(unix)]
fn worker_provenance_persists_in_the_derived_index_and_clears_on_native() {
    use super::convert::ConverterProvenance;
    use super::extract::{store_candidates, try_worker_for_blank_pdf};

    let fixture = Fixture::new("worker-provenance-store");
    fixture.write("blank.pdf", &blank_pdf_fixture());
    let index = fixture.index();
    reconcile_manifest(&fixture.root, &index).expect("reconcile blank PDF");
    let source = current_source_for(&index, "blank.pdf");
    let body = ConversionWorker::response_for(&source.content_sha256, &["Converted"]);
    let worker = ConversionWorker::new(&ConversionWorker::framed(&body), "success");
    let native: Result<Vec<String>, ManifestError> = Ok(vec!["".to_owned()]);
    let (outcome, provenance) = try_worker_for_blank_pdf(
        &source,
        &native,
        &fixture.root.join("blank.pdf"),
        Some(&worker.config()),
        None,
    )
    .expect("worker conversion")
    .expect("worker supplies drafts with provenance");
    let super::extract::PdfOutcome::Drafts(drafts) = outcome else {
        panic!("worker must supply drafts");
    };
    // Partial worker coverage with a missing range must survive the store.
    let provenance = ConverterProvenance {
        coverage: "partial".to_owned(),
        missing: vec!["remainder beyond worker bounds (1 blocks kept)".to_owned()],
        warnings: vec!["uncertain table structure".to_owned()],
        ..provenance.expect("provenance travels with drafts")
    };
    let mut connection = Connection::open(&index).expect("reopen source index");
    store_candidates(&mut connection, &index, &source, drafts, Some(&provenance))
        .expect("store worker drafts");
    type ProvenanceRow = (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let row: ProvenanceRow = connection
        .query_row(
            "SELECT extraction_method, extraction_method_version, extraction_coverage,
                        extraction_missing_json, extraction_warnings_json
                 FROM source_manifest WHERE display_path = 'blank.pdf'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .expect("read converter provenance");
    assert_eq!(row.0.as_deref(), Some("fake-worker"));
    assert_eq!(row.1.as_deref(), Some("hook-test"));
    assert_eq!(row.2.as_deref(), Some("partial"));
    assert!(
        row.3
            .as_deref()
            .is_some_and(|missing| missing.contains("remainder beyond worker bounds")),
        "{row:?}"
    );
    assert!(
        row.4
            .as_deref()
            .is_some_and(|warnings| warnings.contains("uncertain table structure")),
        "{row:?}"
    );
    // A later native generation supersedes the worker one: provenance
    // clears instead of misattributing native text to the converter.
    let source = current_source_for(&index, "blank.pdf");
    let native_pages = crate::retrieval::chunking::ChunkDraft {
        kind: crate::retrieval::chunking::ChunkKind::Page,
        line_start: 1,
        line_end: 1,
        body: String::new(),
        structural_path: Some("page 1".to_owned()),
        language: None,
        definitions: Vec::new(),
        imports: Vec::new(),
        references: Vec::new(),
    };
    store_candidates(&mut connection, &index, &source, vec![native_pages], None)
        .expect("store native drafts");
    let cleared: (Option<String>, Option<String>) = connection
        .query_row(
            "SELECT extraction_method, extraction_coverage
             FROM source_manifest WHERE display_path = 'blank.pdf'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read cleared provenance");
    assert_eq!(cleared, (None, None));
}

#[test]
fn fresh_source_cache_has_converter_provenance_columns_without_invented_values() {
    let fixture = Fixture::new("fresh-provenance");
    fixture.write("lesson.md", b"native provenance fixture\n");
    reconcile_manifest(&fixture.root, &fixture.index()).unwrap();
    extract_source(&fixture.root, &fixture.index(), Path::new("lesson.md")).unwrap();
    let connection = open_index(&fixture.index()).unwrap();
    let row: Vec<Option<String>> = connection
        .query_row(
            "SELECT extraction_method, extraction_method_version, extraction_coverage,
                    extraction_missing_json, extraction_warnings_json
             FROM source_manifest WHERE display_path = 'lesson.md'",
            [],
            |row| (0..5).map(|column| row.get(column)).collect(),
        )
        .unwrap();
    assert_eq!(row, vec![None; 5]);
}

#[test]
fn v1_late_pdf_failure_cannot_demote_newer_index() {
    actual_late_pdf_outcome_is_superseded("failed");
}

#[test]
fn v1_actual_late_pdf_ready_and_sensitive_outcomes_are_noops() {
    for outcome in ["ready", "sensitive"] {
        actual_late_pdf_outcome_is_superseded(outcome);
    }
}

fn actual_late_pdf_outcome_is_superseded(outcome: &'static str) {
    let fixture = Fixture::new("v1-late-pdf");
    fixture.write("book.pdf", &pdf_fixture());
    let index = fixture.index();
    reconcile_manifest(&fixture.root, &index).unwrap();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (published_tx, published_rx) = std::sync::mpsc::channel();
    let root_b = fixture.root.clone();
    let index_b = index.clone();
    let newer = std::thread::spawn(move || {
        started_rx.recv().unwrap();
        let mut newer_bytes = pdf_fixture();
        newer_bytes.extend_from_slice(b"\n% new revision\n");
        fs::write(root_b.join("book.pdf"), newer_bytes).unwrap();
        reconcile_manifest(&root_b, &index_b).unwrap();
        extract_source_with_pdf(&root_b, &index_b, Path::new("book.pdf"), |_| {
            Ok(vec!["newer current searchable body".to_owned()])
        })
        .unwrap();
        let snapshot = extraction_publication_snapshot(&index_b);
        published_tx.send(()).unwrap();
        snapshot
    });
    let result = extract_source_with_pdf(&fixture.root, &index, Path::new("book.pdf"), move |_| {
        started_tx.send(()).unwrap();
        published_rx.recv().unwrap();
        match outcome {
            "failed" => Err(ManifestError::Pdf("old parser failed".to_owned())),
            "sensitive" => Ok(vec!["password=old-sensitive-result".to_owned()]),
            "ready" => Ok(vec!["old prepared body must not be returned".to_owned()]),
            _ => unreachable!(),
        }
    });
    let before = newer.join().unwrap();
    assert!(
        matches!(result, Err(ManifestError::SourceSuperseded)),
        "late {outcome}: {result:?}"
    );
    assert_eq!(
        extraction_publication_snapshot(&index),
        before,
        "late {outcome} modified newer manifest, candidates, provenance or projections"
    );
    let current = extract_source(&fixture.root, &index, Path::new("book.pdf")).unwrap();
    assert_eq!(current[0].body, "newer current searchable body");
}

#[test]
fn v1_actual_late_changed_read_cannot_clear_newer_index_or_unchanged_scan() {
    let fixture = Fixture::new("v1-late-changed-read");
    fixture.write("lesson.md", b"original snapshot\n");
    let index = fixture.index();
    reconcile_manifest(&fixture.root, &index).unwrap();
    let (opened_tx, opened_rx) = std::sync::mpsc::channel();
    let (published_tx, published_rx) = std::sync::mpsc::channel();
    let root_b = fixture.root.clone();
    let index_b = index.clone();
    let newer = std::thread::spawn(move || {
        opened_rx.recv().unwrap();
        fs::write(root_b.join("lesson.md"), b"newer current source text\n").unwrap();
        reconcile_manifest(&root_b, &index_b).unwrap();
        extract_source(&root_b, &index_b, Path::new("lesson.md")).unwrap();
        let before = extraction_publication_snapshot(&index_b);
        published_tx.send(()).unwrap();
        before
    });
    let mut after_open = |_: &Path| {
        opened_tx.send(()).unwrap();
        published_rx.recv().unwrap();
        Ok(())
    };
    let result = extract_source_with_hooks(
        &fixture.root,
        &index,
        Path::new("lesson.md"),
        None,
        None,
        &mut after_open,
    );
    let before = newer.join().unwrap();
    assert!(
        matches!(result, Err(ManifestError::SourceSuperseded)),
        "{result:?}"
    );
    assert_eq!(extraction_publication_snapshot(&index), before);
    let report = reconcile_manifest(&fixture.root, &index).unwrap();
    assert!(report.unchanged >= 1);
    let connection = Connection::open(&index).unwrap();
    let status: String = connection
        .query_row(
            "SELECT index_status FROM source_manifest WHERE display_path = 'lesson.md'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "indexed");
    for table in ["source_search", "source_search_ranked"] {
        let body: String = connection
            .query_row(
                &format!("SELECT body FROM {table} WHERE path = 'lesson.md'"),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(body.contains("newer current source text"));
    }
}

fn extraction_publication_snapshot(index: &Path) -> Vec<Vec<Vec<String>>> {
    let connection = Connection::open(index).unwrap();
    [
        "SELECT * FROM source_manifest ORDER BY manifest_id",
        "SELECT * FROM source_candidates ORDER BY candidate_no",
        "SELECT rowid, * FROM source_search ORDER BY rowid",
        "SELECT rowid, * FROM source_search_ranked ORDER BY rowid",
    ]
    .iter()
    .map(|sql| {
        let mut statement = connection.prepare(sql).unwrap();
        let columns = statement.column_count();
        statement
            .query_map([], |row| {
                (0..columns)
                    .map(|column| Ok(format!("{:?}", row.get_ref(column)?)))
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    })
    .collect()
}

#[test]
fn v1_pdf_resource_limits_do_not_publish_source_failure() {
    for failure in ["timeout", "budget", "cancel"] {
        let fixture = Fixture::new("v1-pdf-resource-limit");
        fixture.write("book.pdf", &pdf_fixture());
        let index = fixture.index();
        reconcile_manifest(&fixture.root, &index).unwrap();
        let before = extraction_publication_snapshot(&index);
        let result =
            extract_source_with_pdf(&fixture.root, &index, Path::new("book.pdf"), move |_| {
                Err(match failure {
                    "timeout" => ManifestError::PdfTimeout,
                    "budget" => ManifestError::BudgetExhausted,
                    "cancel" => ManifestError::Cancelled,
                    _ => unreachable!(),
                })
            });
        assert!(matches!(
            result,
            Err(ManifestError::PdfTimeout
                | ManifestError::BudgetExhausted
                | ManifestError::Cancelled)
        ));
        assert_eq!(
            extraction_publication_snapshot(&index),
            before,
            "{failure} published failure state"
        );
    }
}

#[test]
fn v1_current_parser_failure_returns_original_error_and_only_bounded_diagnostic() {
    let fixture = Fixture::new("v1-current-parser-failure");
    fixture.write("book.pdf", &pdf_fixture());
    let index = fixture.index();
    reconcile_manifest(&fixture.root, &index).unwrap();
    let original = "original-parser-diagnostic-".repeat(30);
    let returned = original.clone();
    let result = extract_source_with_pdf(&fixture.root, &index, Path::new("book.pdf"), move |_| {
        Err(ManifestError::Pdf(returned))
    });
    assert!(matches!(result, Err(ManifestError::Pdf(ref diagnostic)) if *diagnostic == original));
    let connection = Connection::open(&index).unwrap();
    let (status, stored): (String, String) = connection
        .query_row(
            "SELECT index_status, last_error FROM source_manifest WHERE display_path = 'book.pdf'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(status, "failed");
    assert_eq!(stored, original.chars().take(240).collect::<String>());
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM source_candidates", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn v1_extraction_returns_committed_candidates_if_caller_cancels_after_commit() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let fixture = Fixture::new("v1-extraction-post-commit-cancel");
    fixture.write("book.pdf", &pdf_fixture());
    let index = fixture.index();
    reconcile_manifest(&fixture.root, &index).unwrap();
    let flag = Arc::new(AtomicBool::new(false));
    let trigger = flag.clone();
    let token = crate::request::OperationCancellation::with_flag(flag.clone()).with_test_hook(
        Arc::new(move |stage| {
            if stage == "source_publication_after_commit" {
                trigger.store(true, Ordering::Relaxed);
            }
        }),
    );
    let scope = crate::request::enter_request_cancellation(&token);
    let mut after_open = |_: &Path| Ok(());
    let result = extract_source_with_hooks(
        &fixture.root,
        &index,
        Path::new("book.pdf"),
        Some(Box::new(|_| {
            Ok(vec!["committed current candidate".to_owned()])
        })),
        Some(&token),
        &mut after_open,
    );
    drop(scope);
    assert!(flag.load(Ordering::Relaxed));
    let candidates = result.expect("committed extraction is complete even after caller cancels");
    assert_eq!(candidates[0].body, "committed current candidate");
    let actual = extract_source(&fixture.root, &index, Path::new("book.pdf")).unwrap();
    assert_eq!(actual, candidates);
}

#[test]
fn typed_pdf_content_failure_carries_only_successful_guarded_publication_identity() {
    let fixture = Fixture::new("typed-pdf-content-publication");
    fixture.write("book.pdf", &pdf_fixture());
    let index = fixture.index();
    reconcile_manifest(&fixture.root, &index).unwrap();
    let result = extract_source_with_pdf(&fixture.root, &index, Path::new("book.pdf"), |_| {
        Err(ManifestError::PdfContent("parser rejected content".into()))
    });
    let Err(ManifestError::PdfContentPublished(published)) = result else {
        panic!("content failure must carry a successful publication witness");
    };
    assert_eq!(published.diagnostic, "parser rejected content");
    let snapshot = read_selected_facts(
        &index,
        Path::new("book.pdf"),
        FactsInterest::IdentityOnly,
        PreparationDisposition::PdfContentFailure,
        Some(&published),
        &crate::retrieval::OperationCancellation::none(),
    )
    .unwrap();
    let row = snapshot.row.unwrap();
    assert_eq!(row.index_status, "failed");
    assert_eq!(row.manifest_id, published.source.manifest_id);
    assert_eq!(row.source_revision, published.source.source_revision);
    assert_eq!(
        row.content_sha256.as_deref(),
        Some(published.source.content_sha256.as_str())
    );
    Connection::open(&index)
        .unwrap()
        .execute(
            "UPDATE source_manifest SET index_status = 'pending' WHERE manifest_id = ?1",
            [row.manifest_id],
        )
        .unwrap();
    assert!(matches!(
        read_selected_facts(
            &index,
            Path::new("book.pdf"),
            FactsInterest::IdentityOnly,
            PreparationDisposition::PdfContentFailure,
            Some(&published),
            &crate::retrieval::OperationCancellation::none()
        ),
        Err(ManifestError::SourceSuperseded)
    ));
}

#[test]
fn failed_pdf_content_publication_propagates_storage_or_supersession_and_keeps_prior_state() {
    for failure in ["storage", "superseded"] {
        let fixture = Fixture::new("failed-content-publication");
        fixture.write("book.pdf", &pdf_fixture());
        let index = fixture.index();
        reconcile_manifest(&fixture.root, &index).unwrap();
        let before = extraction_publication_snapshot(&index);
        if failure == "storage" {
            Connection::open(&index).unwrap().execute_batch(
                "CREATE TRIGGER reject_content_failure BEFORE UPDATE OF index_status ON source_manifest
                 WHEN NEW.index_status = 'failed' BEGIN SELECT RAISE(ABORT, 'test publication rejection'); END;"
            ).unwrap();
            let result =
                extract_source_with_pdf(&fixture.root, &index, Path::new("book.pdf"), |_| {
                    Err(ManifestError::PdfContent("parser rejected content".into()))
                });
            assert!(matches!(result, Err(ManifestError::Sqlite { .. })));
            assert_eq!(extraction_publication_snapshot(&index), before);
        } else {
            let update_index = index.clone();
            let result = extract_source_with_pdf(
                &fixture.root,
                &index,
                Path::new("book.pdf"),
                move |_| {
                    Connection::open(update_index).unwrap().execute_batch("UPDATE source_manifest SET index_status = 'indexed', last_error = NULL;").unwrap();
                    Err(ManifestError::PdfContent("parser rejected content".into()))
                },
            );
            assert!(matches!(result, Err(ManifestError::SourceSuperseded)));
            let status: String = Connection::open(&index)
                .unwrap()
                .query_row("SELECT index_status FROM source_manifest", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(status, "indexed");
        }
    }
}

#[test]
fn exact_source_preparation_rejects_foreign_cache_identity_without_changing_it() {
    let fixture = Fixture::new("exact-cache-identity");
    fixture.write("source.md", b"original subject material\n");
    let index = fixture.index();
    Connection::open(&index)
        .unwrap()
        .execute_batch(
            "CREATE TABLE foreign_data(value TEXT); INSERT INTO foreign_data VALUES ('preserve');",
        )
        .unwrap();
    let before = fs::read(&index).unwrap();
    let result = crate::retrieval::prepare_explicit_source(
        &fixture.root,
        Path::new("source.md"),
        FactsInterest::IdentityOnly,
        &crate::retrieval::OperationCancellation::none(),
    );
    assert!(matches!(result, Err(ManifestError::Invalid(_))));
    assert_eq!(fs::read(&index).unwrap(), before);
    assert!(matches!(
        read_selected_facts(
            &fixture.root.join("arbitrary.sqlite"),
            Path::new("source.md"),
            FactsInterest::IdentityOnly,
            PreparationDisposition::Complete,
            None,
            &crate::retrieval::OperationCancellation::none()
        ),
        Err(ManifestError::Invalid(_))
    ));
}

#[test]
fn ordinary_parser_rejection_survives_successful_worker_framing_as_content_failure() {
    let result = run_pdf_command(pdf_helper("extract"), b"not a PDF", Duration::from_secs(5));
    assert!(
        matches!(result, Err(ManifestError::PdfContent(ref diagnostic)) if diagnostic.starts_with("PDF text extraction failed:"))
    );
}
