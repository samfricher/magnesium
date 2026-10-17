use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, params, types::Value};

use super::schema::{
    INDEX_APPLICATION_ID, INDEX_DIAGNOSTIC_BUDGET, INDEX_INITIALIZATION_BUDGET, INDEX_SCHEMA,
    INDEX_SCHEMA_VERSION, initialize_index, open_index, open_index_for_write,
    validate_index_integrity,
};
use crate::retrieval::OperationCancellation;

const MATERIAL_COLUMNS: [&str; 14] = [
    "path_key",
    "display_path",
    "source_kind",
    "content_sha256",
    "index_status",
    "present",
    "sensitivity_class",
    "exclusion_reason",
    "last_error",
    "extraction_method",
    "extraction_method_version",
    "extraction_coverage",
    "extraction_missing_json",
    "extraction_warnings_json",
];

fn schema_connection(recursive: bool) -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .pragma_update(None, "recursive_triggers", recursive)
        .unwrap();
    connection.execute_batch(INDEX_SCHEMA).unwrap();
    connection
}

fn insert_source(connection: &Connection) {
    connection
        .execute_batch(
            "INSERT INTO source_manifest(
                manifest_id, path_key, display_path, source_kind,
                index_status, present, observed_generation
            ) VALUES (1, x'6c6573736f6e2e6d64', 'lesson.md', 'text', 'pending', 1, 1);",
        )
        .unwrap();
}

fn revision(connection: &Connection) -> i64 {
    connection
        .query_row(
            "SELECT source_revision FROM source_manifest WHERE manifest_id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn fresh_source_manifest_has_revision_counter() {
    let connection = schema_connection(false);
    let columns: i64 = connection
        .query_row(
            "SELECT count(*) FROM pragma_table_info('source_manifest') WHERE name = 'source_revision' AND type = 'INTEGER' AND \"notnull\" = 1 AND dflt_value = '1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(columns, 1, "fresh source manifests need a revision counter");
    assert_eq!(INDEX_SCHEMA_VERSION, 10);
    insert_source(&connection);
    assert_eq!(revision(&connection), 1);
    for invalid in [Value::Integer(0), Value::Integer(-1), Value::Null] {
        assert!(
            connection
                .execute(
                    "UPDATE source_manifest SET source_revision = ?1 WHERE manifest_id = 1",
                    [invalid],
                )
                .is_err()
        );
        assert_eq!(revision(&connection), 1);
    }
}

#[test]
fn revision_trigger_names_only_existing_material_columns() {
    let connection = schema_connection(false);
    let trigger: String = connection
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = 'source_manifest_revision'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    // SQLite silently accepts nonexistent UPDATE OF names. Inspect the actual
    // installed trigger, then prove behavior independently in the next test.
    let listed: Vec<&str> = trigger
        .split_once("AFTER UPDATE OF")
        .unwrap()
        .1
        .split_once("ON source_manifest")
        .unwrap()
        .0
        .split(',')
        .map(str::trim)
        .collect();
    assert_eq!(listed, MATERIAL_COLUMNS);
    let existing = connection
        .prepare("SELECT name FROM pragma_table_info('source_manifest')")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<BTreeSet<_>>>()
        .unwrap();
    for column in listed {
        assert!(
            existing.contains(column),
            "trigger column {column} is missing"
        );
    }
    let triggers = connection
        .prepare("SELECT name FROM sqlite_schema WHERE type = 'trigger' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(triggers, ["source_manifest_revision"]);
}

#[test]
fn each_material_field_advances_revision_only_when_changed_with_recursive_triggers_on_or_off() {
    let changed = [
        Value::Blob(b"renamed.md".to_vec()),
        Value::Text("renamed.md".into()),
        Value::Text("pdf".into()),
        Value::Text("a".repeat(64)),
        Value::Text("indexed".into()),
        Value::Integer(0),
        Value::Text("credential".into()),
        Value::Text("sensitive content".into()),
        Value::Text("parser failed".into()),
        Value::Text("fixture_converter".into()),
        Value::Text("2.0".into()),
        Value::Text("partial".into()),
        Value::Text("[2]".into()),
        Value::Text("[\"missing page\"]".into()),
    ];
    for recursive in [false, true] {
        for (column, value) in MATERIAL_COLUMNS.iter().zip(&changed) {
            let connection = schema_connection(recursive);
            insert_source(&connection);
            let select = format!("SELECT {column} FROM source_manifest WHERE manifest_id = 1");
            let original: Value = connection.query_row(&select, [], |row| row.get(0)).unwrap();
            let update = format!("UPDATE source_manifest SET {column} = ?1 WHERE manifest_id = 1");
            connection.execute(&update, [&original]).unwrap();
            assert_eq!(
                revision(&connection),
                1,
                "identical {column}, recursive={recursive}"
            );
            connection.execute(&update, [value]).unwrap();
            assert_eq!(
                revision(&connection),
                2,
                "changed {column}, recursive={recursive}"
            );
            connection.execute(&update, [value]).unwrap();
            assert_eq!(
                revision(&connection),
                2,
                "repeated {column}, recursive={recursive}"
            );
            connection.execute(&update, [&original]).unwrap();
            assert_eq!(
                revision(&connection),
                3,
                "restored {column}, recursive={recursive}"
            );
            connection.execute(&update, [&original]).unwrap();
            assert_eq!(
                revision(&connection),
                3,
                "identical restored {column}, recursive={recursive}"
            );
            if original != Value::Null {
                assert!(
                    connection.execute(&update, [Value::Null]).is_err(),
                    "{column} must not accept NULL"
                );
                assert_eq!(revision(&connection), 3);
            }
            // Nullable fields start NULL, so the loop explicitly covers
            // NULL->value, identical value, value->NULL, and identical NULL.
        }
    }
}

#[test]
fn heartbeat_and_verified_content_metadata_refreshes_do_not_advance_revision() {
    for recursive in [false, true] {
        let connection = schema_connection(recursive);
        insert_source(&connection);
        for generation in 2..=5 {
            connection
                .execute(
                    "UPDATE source_manifest SET observed_generation = ?1 WHERE manifest_id = 1",
                    [generation],
                )
                .unwrap();
            assert_eq!(revision(&connection), 1);
        }
        for column in ["file_size", "modified_unix_ns"] {
            let update = format!("UPDATE source_manifest SET {column} = ?1 WHERE manifest_id = 1");
            for value in [
                Value::Integer(7),
                Value::Integer(7),
                Value::Null,
                Value::Null,
            ] {
                connection.execute(&update, [value]).unwrap();
                assert_eq!(revision(&connection), 1, "metadata-only {column}");
            }
        }
        connection
            .execute_batch("UPDATE source_manifest SET observed_generation = 6, file_size = 42, modified_unix_ns = 123 WHERE manifest_id = 1;")
            .unwrap();
        assert_eq!(revision(&connection), 1);
    }
}

#[test]
fn revision_trigger_has_no_projection_side_effects_and_advances_once_per_row_update() {
    for recursive in [false, true] {
        let connection = schema_connection(recursive);
        insert_source(&connection);
        connection
            .execute(
                "INSERT INTO source_candidates(candidate_no, candidate_id, manifest_id, content_sha256, anchor_kind, anchor_start, anchor_end, body, status) VALUES (7, ?1, 1, ?2, 'line', 1, 1, 'current candidate body', 'current')",
                params!["c".repeat(64), "a".repeat(64)],
            )
            .unwrap();
        for table in ["source_search", "source_search_ranked"] {
            connection
                .execute(
                    &format!("INSERT INTO {table}(rowid, path, body) VALUES (7, 'lesson.md', 'current candidate body')"),
                    [],
                )
                .unwrap();
        }
        connection
            .execute_batch("UPDATE source_manifest SET display_path = 'renamed.md', index_status = 'excluded', present = 0, last_error = 'new outcome' WHERE manifest_id = 1;")
            .unwrap();
        assert_eq!(revision(&connection), 2);
        let candidate: (i64, String, String) = connection
            .query_row(
                "SELECT candidate_no, body, status FROM source_candidates",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            candidate,
            (7, "current candidate body".into(), "current".into())
        );
        for table in ["source_search", "source_search_ranked"] {
            let projection: (i64, String, String) = connection
                .query_row(
                    &format!("SELECT rowid, path, body FROM {table}"),
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
            assert_eq!(
                projection,
                (7, "lesson.md".into(), "current candidate body".into())
            );
        }
    }
}

#[test]
fn revision_rolls_back_with_material_state_and_is_row_local() {
    for recursive in [false, true] {
        let mut connection = schema_connection(recursive);
        insert_source(&connection);
        connection
            .execute_batch("INSERT INTO source_manifest(manifest_id, path_key, display_path, source_kind, index_status, present, observed_generation) VALUES (2, x'7365636f6e642e6d64', 'second.md', 'text', 'pending', 1, 1);")
            .unwrap();
        let transaction = connection.transaction().unwrap();
        transaction
            .execute_batch("UPDATE source_manifest SET display_path = 'renamed.md', index_status = 'indexed' WHERE manifest_id = 1;")
            .unwrap();
        assert_eq!(revision(&transaction), 2);
        let other_revision: i64 = transaction
            .query_row(
                "SELECT source_revision FROM source_manifest WHERE manifest_id = 2",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(other_revision, 1);
        transaction.rollback().unwrap();
        assert_eq!(revision(&connection), 1);
        let restored: (String, String) = connection
            .query_row(
                "SELECT display_path, index_status FROM source_manifest WHERE manifest_id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(restored, ("lesson.md".into(), "pending".into()));
    }
}

#[test]
fn returning_does_not_include_after_trigger_revision_but_followup_select_does() {
    for recursive in [false, true] {
        let connection = schema_connection(recursive);
        insert_source(&connection);
        let returned: i64 = connection
            .query_row(
                "UPDATE source_manifest SET index_status = 'indexed' WHERE manifest_id = 1 RETURNING source_revision",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            returned, 1,
            "SQLite RETURNING precedes AFTER-trigger updates"
        );
        assert_eq!(revision(&connection), 2);
        let unchanged: i64 = connection
            .query_row(
                "UPDATE source_manifest SET index_status = 'indexed' WHERE manifest_id = 1 RETURNING source_revision",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(unchanged, 2);
        assert_eq!(revision(&connection), 2);
    }
}

#[test]
fn content_aba_and_same_hash_terminal_outcome_have_distinct_revisions() {
    for recursive in [false, true] {
        let connection = schema_connection(recursive);
        insert_source(&connection);
        connection
            .execute(
                "UPDATE source_manifest SET content_sha256 = ?1 WHERE manifest_id = 1",
                ["a".repeat(64)],
            )
            .unwrap();
        let captured = revision(&connection);
        for digest in ["b".repeat(64), "a".repeat(64)] {
            connection
                .execute(
                    "UPDATE source_manifest SET content_sha256 = ?1 WHERE manifest_id = 1",
                    [digest],
                )
                .unwrap();
        }
        assert_eq!(revision(&connection), captured + 2);
        connection
            .execute_batch(
                "UPDATE source_manifest SET index_status = 'indexed' WHERE manifest_id = 1;",
            )
            .unwrap();
        assert_eq!(revision(&connection), captured + 3);
    }
}

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct IndexFixture(PathBuf);

impl IndexFixture {
    fn new() -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "magnesium-revision-schema-{}-{unique}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(root.join(crate::workspace::CONTROL_DIR).join("index")).unwrap();
        Self(root)
    }

    fn index(&self) -> PathBuf {
        self.0
            .join(crate::workspace::CONTROL_DIR)
            .join("index/source-search.sqlite")
    }
}

impl Drop for IndexFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn cache_artifacts(path: &Path) -> Vec<Option<Vec<u8>>> {
    ["", "-wal", "-shm", "-journal"]
        .into_iter()
        .map(|suffix| {
            let mut name = path.as_os_str().to_os_string();
            name.push(suffix);
            let artifact = PathBuf::from(name);
            if artifact.exists() {
                Some(fs::read(artifact).unwrap())
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn fresh_source_cache_is_schema_ten_and_schema_nine_stays_byte_preserved() {
    let fresh = IndexFixture::new();
    let connection = initialize_index(
        &fresh.index(),
        INDEX_INITIALIZATION_BUDGET,
        &OperationCancellation::none(),
    )
    .unwrap();
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 10);
    insert_source(&connection);
    assert_eq!(revision(&connection), 1);
    drop(connection);

    for wal in [false, true] {
        let legacy = IndexFixture::new();
        let index = legacy.index();
        let canonical = legacy
            .0
            .join(crate::workspace::CONTROL_DIR)
            .join("state.sqlite");
        fs::write(
            &canonical,
            b"canonical learner-store bytes are not a source cache",
        )
        .unwrap();
        let canonical_before = fs::read(&canonical).unwrap();
        let owner = Connection::open(&index).unwrap();
        owner
            .pragma_update(None, "application_id", INDEX_APPLICATION_ID)
            .unwrap();
        owner.pragma_update(None, "user_version", 9).unwrap();
        if wal {
            owner.pragma_update(None, "journal_mode", "WAL").unwrap();
        }
        // Reconstruct the previous cache schema by omitting only this repair's
        // counter and trigger. Identity rejection must preserve even live WAL
        // artifacts without attempting a migration or opening their contents.
        let (before_trigger, trigger_and_rest) = INDEX_SCHEMA
            .split_once("CREATE TRIGGER source_manifest_revision")
            .unwrap();
        let (_, after_trigger) = trigger_and_rest.split_once("END;").unwrap();
        let legacy_schema = format!("{before_trigger}{after_trigger}").replace(
            "    source_revision INTEGER NOT NULL DEFAULT 1 CHECK (source_revision >= 1),\n",
            "",
        );
        owner.execute_batch(&legacy_schema).unwrap();
        insert_source(&owner);
        owner
            .execute_batch(
                "UPDATE source_index_metadata SET generation = 12; UPDATE source_manifest SET display_path = 'legacy lesson.md';",
            )
            .unwrap();
        let before = cache_artifacts(&index);
        if wal {
            assert!(before[1].is_some());
            assert!(before[2].is_some());
        }
        for error in [
            open_index(&index).unwrap_err(),
            open_index_for_write(&index).unwrap_err(),
            initialize_index(
                &index,
                INDEX_INITIALIZATION_BUDGET,
                &OperationCancellation::none(),
            )
            .unwrap_err(),
            validate_index_integrity(
                &index,
                INDEX_DIAGNOSTIC_BUDGET,
                &OperationCancellation::none(),
            )
            .unwrap_err(),
        ] {
            let message = error.to_string();
            for required in [
                "schema version 9",
                "and 10",
                "The file was not rebuilt",
                "stop Magnesium processes",
                "-wal/-shm/-journal sidecars",
                "then run magnesium init",
                &format!(
                    "Do not move or delete {}/state.sqlite",
                    crate::workspace::CONTROL_DIR
                ),
            ] {
                assert!(
                    message.contains(required),
                    "missing {required:?} in {message}"
                );
            }
        }
        assert_eq!(cache_artifacts(&index), before);
        assert_eq!(fs::read(&canonical).unwrap(), canonical_before);
        let original: String = owner
            .query_row("SELECT display_path FROM source_manifest", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(original, "legacy lesson.md");
    }
}
