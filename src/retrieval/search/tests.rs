use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension};

use super::*;
use crate::retrieval::{RetrievalRequest, retrieve_workspace_sources};

static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

#[test]
fn projection_field_removal_preserves_effective_bm25_weights() {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(crate::retrieval::manifest::schema::INDEX_SCHEMA)
        .unwrap();
    connection
        .execute_batch(
            "CREATE VIRTUAL TABLE legacy_projection USING fts5(
                candidate_id UNINDEXED, manifest_id UNINDEXED,
                path, structural_path, body, symbols,
                tokenize = \"unicode61 tokenchars '_'\"
             );",
        )
        .unwrap();
    for (index, fields) in [
        ("weightneedle.md", "", "ordinary body", ""),
        (
            "ordinary.md",
            "weightneedle",
            "ordinary body with more words",
            "",
        ),
        (
            "ordinary.md",
            "",
            "weightneedle weightneedle ordinary body",
            "",
        ),
        ("ordinary.md", "", "ordinary body", "weightneedle"),
    ]
    .into_iter()
    .enumerate()
    {
        connection
            .execute(
                "INSERT INTO legacy_projection(rowid, candidate_id, manifest_id, path,
                      structural_path, body, symbols) VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6)",
                params![
                    index as i64 + 1,
                    format!("{index:064x}"),
                    fields.0,
                    fields.1,
                    fields.2,
                    fields.3
                ],
            )
            .unwrap();
        for table in ["source_search", "source_search_ranked"] {
            connection
                .execute(
                    &format!(
                        "INSERT INTO {table}(rowid, path, structural_path, body, symbols)
                     VALUES (?1, ?2, ?3, ?4, ?5)"
                    ),
                    params![index as i64 + 1, fields.0, fields.1, fields.2, fields.3],
                )
                .unwrap();
        }
    }
    let legacy = connection
        .prepare(
            "SELECT rowid, bm25(legacy_projection, 0.0, 0.0, 0.0, 8.0, 4.0, 1.0, 6.0)
             FROM legacy_projection WHERE legacy_projection MATCH 'weightneedle' ORDER BY rowid",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?.to_bits()))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    for table in ["source_search", "source_search_ranked"] {
        let current = connection
            .prepare(&format!(
                "SELECT rowid, bm25({table}, 0.0, 8.0, 4.0, 1.0)
                 FROM {table} WHERE {table} MATCH 'weightneedle' ORDER BY rowid"
            ))
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?.to_bits()))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(
            current, legacy,
            "effective BM25 field weights changed for {table}"
        );
    }
}

#[test]
fn canonical_source_keys_survive_incremental_refresh_rebuild_order_and_vacuum() {
    use crate::retrieval::manifest::{CurrentSource, extract, ranking};

    let fixture = Fixture::new("canonical-projection-keys");
    let text = chapter7_navigation_fixture();
    fixture.write("chapter7.md", text.as_bytes());
    fixture.write("ordinary.md", b"normal approximation ordinary body\n");
    let before = fixture.search(request("\"normal approximation\"")).hits;
    let keys = candidate_keys(&Connection::open(fixture.index()).unwrap());
    let projections = projection_snapshot(&Connection::open(fixture.index()).unwrap());

    // Changed extraction order must reuse existing canonical keys even
    // after both FTS rows have been deleted by incremental invalidation.
    let mut connection = Connection::open(fixture.index()).unwrap();
    let source = connection
            .query_row(
                "SELECT manifest_id, display_path, source_kind, content_sha256, index_status, source_revision
             FROM source_manifest WHERE display_path = 'chapter7.md'",
                [],
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
            .unwrap();
    let mut chunks = super::super::chunking::chunk_text(Path::new("chapter7.md"), &text);
    chunks.reverse();
    extract::store_candidates(&mut connection, &fixture.index(), &source, chunks, None).unwrap();
    assert_eq!(candidate_keys(&connection), keys);
    assert_eq!(projection_snapshot(&connection), projections);
    drop(connection);

    // A real source edit allocates only the changed public identity;
    // restoring the source revives the prior number rather than replacing it.
    fixture.write(
        "chapter7.md",
        text.replace("Cover notes", "Changed cover notes")
            .as_bytes(),
    );
    fixture.search(request("\"normal approximation\""));
    let changed_keys = candidate_keys(&Connection::open(fixture.index()).unwrap());
    for (id, number) in &keys {
        assert_eq!(changed_keys.get(id), Some(number));
    }
    assert!(changed_keys.len() > keys.len());
    fixture.write("chapter7.md", text.as_bytes());
    assert_eq!(
        fixture.search(request("\"normal approximation\"")).hits,
        before
    );
    // Deleting a source invalidates only that source's canonical numbers.
    // Reintroducing the same source must never attach another source's text.
    fs::remove_file(fixture.root.join("chapter7.md")).unwrap();
    let remaining = fixture.search(request("\"normal approximation\"")).hits;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].candidate.path, "ordinary.md");
    projection_snapshot(&Connection::open(fixture.index()).unwrap());
    fixture.write("chapter7.md", text.as_bytes());
    assert_eq!(
        fixture.search(request("\"normal approximation\"")).hits,
        before
    );
    let mut connection = Connection::open(fixture.index()).unwrap();
    let restored_keys = candidate_keys(&connection);
    assert_eq!(restored_keys, changed_keys);
    let restored_projections = projection_snapshot(&connection);
    assert_eq!(restored_projections, projections);

    struct Projection {
        candidate_no: i64,
        manifest_id: i64,
        path: String,
        structural_path: Option<String>,
        body: String,
        symbols: String,
    }
    let rows = connection
        .prepare(
            "SELECT candidate_no, candidate.manifest_id, display_path, structural_path, body,
                    definitions_json || ' ' || imports_json || ' ' || references_json
             FROM source_candidates candidate JOIN source_manifest manifest
               ON candidate.manifest_id = manifest.manifest_id
             WHERE candidate.status = 'current' ORDER BY candidate_no",
        )
        .unwrap()
        .query_map([], |row| {
            Ok(Projection {
                candidate_no: row.get(0)?,
                manifest_id: row.get(1)?,
                path: row.get(2)?,
                structural_path: row.get(3)?,
                body: row.get(4)?,
                symbols: row.get(5)?,
            })
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    let transaction = connection.transaction().unwrap();
    transaction
        .execute_batch(
            "DELETE FROM source_search; DELETE FROM source_search_ranked;
             INSERT INTO source_search(rowid, path, structural_path, body, symbols)
             SELECT candidate_no, display_path, structural_path, body,
                    definitions_json || ' ' || imports_json || ' ' || references_json
             FROM source_candidates candidate JOIN source_manifest manifest
               ON candidate.manifest_id = manifest.manifest_id
             WHERE candidate.status = 'current' ORDER BY candidate_no DESC;",
        )
        .unwrap();
    // Rebuild ranking in the opposite order from original matching,
    // deriving its navigation-stripped body from canonical candidates.
    for row in &rows {
        let bodies = rows
            .iter()
            .filter(|candidate| candidate.manifest_id == row.manifest_id)
            .map(|candidate| candidate.body.as_str())
            .collect::<Vec<_>>();
        let navigation = ranking::navigation_lines(&bodies);
        ranking::insert_ranked_candidate(
            &transaction,
            &fixture.index(),
            row.candidate_no,
            &row.path,
            row.structural_path.as_deref(),
            &ranking::ranking_body(&row.body, &navigation),
            &row.symbols,
        )
        .unwrap();
    }
    transaction.commit().unwrap();
    assert_eq!(candidate_keys(&connection), restored_keys);
    assert_eq!(projection_snapshot(&connection), restored_projections);
    connection.execute_batch("VACUUM").unwrap();
    assert_eq!(candidate_keys(&connection), restored_keys);
    assert_eq!(projection_snapshot(&connection), restored_projections);
    drop(connection);
    let after = query_index_with_budget(
        &fixture.root,
        &fixture.index(),
        Path::new(""),
        &request("\"normal approximation\""),
        None,
        &OperationCancellation::none(),
        Duration::from_secs(5),
    )
    .unwrap();
    assert_eq!(after, before);
}

#[test]
fn missing_ranking_projection_falls_back_to_original_identity_and_signals() {
    // NULL IN (an empty set) is false in SQLite, so a COALESCE membership
    // predicate cannot reliably distinguish an absent ranking projection.
    for other_ranked_match in [false, true] {
        let fixture = Fixture::new("missing-ranking-key");
        fixture.write("ordinary.md", b"normal approximation ordinary body\n");
        if other_ranked_match {
            fixture.write("z.md", b"normal approximation ordinary body\n");
        }
        let before = fixture.search(request("\"normal approximation\"")).hits;
        assert_eq!(before.len(), if other_ranked_match { 2 } else { 1 });
        assert!(before[0].signals.phrase_match && before[0].signals.full_token_match);
        let connection = Connection::open(fixture.index()).unwrap();
        connection
            .execute(
                "DELETE FROM source_search_ranked WHERE rowid IN (
                SELECT candidate_no FROM source_candidates candidate JOIN source_manifest manifest
                  ON candidate.manifest_id = manifest.manifest_id
                WHERE display_path = 'ordinary.md'
            )",
                [],
            )
            .unwrap();
        drop(connection);
        let after = query_index_with_budget(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &request("\"normal approximation\""),
            None,
            &OperationCancellation::none(),
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(after, before);
    }
}

fn candidate_keys(connection: &Connection) -> std::collections::BTreeMap<String, i64> {
    connection
        .prepare("SELECT candidate_id, candidate_no FROM source_candidates")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn projection_snapshot(connection: &Connection) -> Vec<(i64, String, String, String, String)> {
    let mut rows = connection
        .prepare(
            "SELECT candidate_no, candidate_id, source_search.path, source_search.body,
                    source_search_ranked.body
             FROM source_candidates candidate
             JOIN source_search ON source_search.rowid = candidate_no
             JOIN source_search_ranked ON source_search_ranked.rowid = candidate_no
             WHERE status = 'current' ORDER BY candidate_no",
        )
        .unwrap();
    let snapshot = rows
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    for table in ["source_search", "source_search_ranked"] {
        let orphans: i64 = connection
            .query_row(
                &format!(
                    "SELECT count(*) FROM {table} projection LEFT JOIN source_candidates candidate
                   ON candidate_no = projection.rowid
                 WHERE candidate_no IS NULL OR status != 'current'"
                ),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(orphans, 0);
        let count: i64 = connection
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count as usize, snapshot.len());
    }
    snapshot
}

#[test]
fn corpus_query_uses_keyed_ranking_projection_joins() {
    let mut measurements = Vec::new();
    for documents in [100_i64, 1_000, 3_000] {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(crate::retrieval::manifest::schema::INDEX_SCHEMA)
            .unwrap();
        let transaction = connection.transaction().unwrap();
        for index in 0..documents {
            let candidate_id = format!("{index:064x}");
            let content_hash = "a".repeat(64);
            let path = format!("source-{index:05}.md");
            let body = if index % 2 == 0 {
                "corpusneedle normal body"
            } else {
                "other body"
            };
            transaction
                .execute(
                    "INSERT INTO source_manifest(manifest_id, path_key, display_path,
                         source_kind, content_sha256, index_status, present, observed_generation)
                     VALUES (?1, ?2, ?3, 'text', ?4, 'indexed', 1, 1)",
                    params![index + 1, path.as_bytes(), path, content_hash],
                )
                .unwrap();
            transaction.execute(
                    "INSERT INTO source_candidates(candidate_no, candidate_id, manifest_id, content_sha256,
                         anchor_kind, anchor_start, anchor_end, body, status)
                     VALUES (?1, ?2, ?1, ?3, 'line', 1, 1, ?4, 'current')",
                    params![index + 1, candidate_id, content_hash, body],
                ).unwrap();
            for table in ["source_search", "source_search_ranked"] {
                transaction
                    .execute(
                        &format!(
                            "INSERT INTO {table}(rowid, path, structural_path, body, symbols)
                                  VALUES (?1, ?2, NULL, ?3, '')"
                        ),
                        params![index + 1, path, body],
                    )
                    .unwrap();
            }
        }
        transaction.commit().unwrap();
        let arguments = || {
            params![
                "\"corpusneedle\"*",
                "corpusneedle",
                "",
                "",
                "",
                "",
                false,
                "[]",
                "\"corpusneedle\"",
                "\"corpusneedle\"",
            ]
        };
        let plan = connection
            .prepare(&format!("EXPLAIN QUERY PLAN {SOURCE_QUERY_SQL}"))
            .unwrap()
            .query_map(arguments(), |row| row.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        let callbacks = Arc::new(AtomicU64::new(0));
        let count = callbacks.clone();
        connection
            .progress_handler(
                1_000,
                Some(move || {
                    count.fetch_add(1, Ordering::Relaxed);
                    false
                }),
            )
            .unwrap();
        let ids = connection
            .prepare(SOURCE_QUERY_SQL)
            .unwrap()
            .query_map(arguments(), |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        connection
            .progress_handler(0, None::<fn() -> bool>)
            .unwrap();
        assert_eq!(ids.len() as i64, documents / 2);
        assert_eq!(
            ids,
            (0..documents)
                .step_by(2)
                .map(|index| format!("{index:064x}"))
                .collect::<Vec<_>>()
        );
        let count = callbacks.load(Ordering::Relaxed);
        eprintln!("source corpus documents={documents}, callbacks/1000={count}, plan={plan:#?}");
        measurements.push((documents, count, plan));
    }
    for (documents, callbacks, plan) in measurements {
        assert!(
            callbacks < u64::try_from(documents).unwrap(),
            "full production query must remain within a linear VM instruction bound: documents={documents}, callbacks={callbacks}"
        );
        assert!(
            plan.iter()
                .filter(|step| step.contains("source_search_ranked") && step.contains("LEFT-JOIN"))
                .all(|step| step.contains(":=")),
            "ranking projection must use rowid equality, not scan per match: {plan:#?}"
        );
    }
}

#[test]
fn excluded_sources_execute_ranking_once_and_preserve_allowed_order() {
    let fixture = Fixture::new("one-ranked-cursor");
    for index in 0..48 {
        fixture.write(
            &format!("aaa-export/private-{index:03}.md"),
            b"cursorfixture cursorfixture cursorfixture private history\n",
        );
    }
    for index in 0..8 {
        fixture.write(
            &format!("ordinary/source-{index:03}.md"),
            b"cursorfixture ordinary source material\n",
        );
    }
    prepare_source_query_with_allowlist_and_cancellation(
        &fixture.root,
        &fixture.index(),
        Path::new(""),
        &[],
        CoordinatorBudget {
            max_metadata_entries: 128,
            max_on_demand: 128,
            max_duration: Duration::from_secs(30),
        },
        None,
        &OperationCancellation::none(),
    )
    .unwrap();
    let mut selected = request("cursorfixture");
    selected.limit = 4;
    let all = query_index_with_budget(
        &fixture.root,
        &fixture.index(),
        Path::new(""),
        &SourceSearchRequest {
            limit: 50,
            ..selected.clone()
        },
        None,
        &OperationCancellation::none(),
        Duration::from_secs(5),
    )
    .unwrap();
    assert!(
        all[..48]
            .iter()
            .all(|hit| hit.candidate.path.starts_with("aaa-export/"))
    );
    fixture.write("aaa-export/.magnesium-export", b"magnesium-export-v2\n");
    let (result, statements) = crate::retrieval::manifest::schema::sql_trace::record(|| {
        query_index_with_budget(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &selected,
            None,
            &OperationCancellation::none(),
            Duration::from_secs(5),
        )
    });
    let hits = result.unwrap();
    assert_eq!(
        hits.iter()
            .map(|hit| hit.candidate.path.clone())
            .collect::<Vec<_>>(),
        (0..4)
            .map(|index| format!("ordinary/source-{index:03}.md"))
            .collect::<Vec<_>>()
    );
    for hit in &hits {
        let original = fs::read(fixture.root.join(&hit.candidate.path)).unwrap();
        assert_eq!(
            hit.candidate.content_sha256,
            crate::retrieval::manifest::sha256_hex(&original)
        );
        assert_eq!(
            (hit.candidate.anchor_start, hit.candidate.anchor_end),
            (1, 1)
        );
    }
    let ranking_count = statements
        .iter()
        .filter(|sql| sql.starts_with("WITH ranked_matches AS MATERIALIZED"))
        .count();
    eprintln!("ranked statement executions across 48 excluded rows: {ranking_count}");
    assert_eq!(
        ranking_count, 1,
        "ranking must execute once, then stream past excluded rows"
    );
}

#[test]
fn ranked_cursor_checks_markers_changed_while_rows_are_streamed() {
    let fixture = Fixture::new("live-cursor-marker");
    fixture.write(
        "aaa-export/private.md",
        b"streammarkerfixture streammarkerfixture private history\n",
    );
    fixture.write("ordinary.md", b"streammarkerfixture ordinary source\n");
    assert_eq!(fixture.search(request("streammarkerfixture")).hits.len(), 2);
    let marker = fixture.root.join("aaa-export/.magnesium-export");
    let (result, statements) = crate::retrieval::manifest::schema::sql_trace::record_with_hook(
        move |event| {
            if event == "ranked row" {
                fs::write(&marker, b"magnesium-export-v2\n").unwrap();
            }
        },
        || {
            query_index_with_budget(
                &fixture.root,
                &fixture.index(),
                Path::new(""),
                &request("streammarkerfixture"),
                None,
                &OperationCancellation::none(),
                Duration::from_secs(5),
            )
        },
    );
    let hits = result.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].candidate.path, "ordinary.md");
    assert_eq!(
        statements
            .iter()
            .filter(|sql| sql.starts_with("WITH ranked_matches AS MATERIALIZED"))
            .count(),
        1
    );
}

#[test]
fn running_ranked_cursor_observes_sql_cancellation() {
    let fixture = Fixture::new("cancel-running-ranked-cursor");
    for index in 0..48 {
        fixture.write(
            &format!("source-{index:03}.md"),
            b"cancelcursorfixture ordinary source material\n",
        );
    }
    prepare_source_query_with_allowlist_and_cancellation(
        &fixture.root,
        &fixture.index(),
        Path::new(""),
        &[],
        CoordinatorBudget {
            max_metadata_entries: 128,
            max_on_demand: 128,
            max_duration: Duration::from_secs(30),
        },
        None,
        &OperationCancellation::none(),
    )
    .unwrap();
    let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let trigger = flag.clone();
    let (result, statements) = crate::retrieval::manifest::schema::sql_trace::record_with_hook(
        move |sql| {
            if sql.starts_with("WITH ranked_matches AS MATERIALIZED") {
                trigger.store(true, Ordering::Relaxed);
            }
        },
        || {
            query_index_with_budget(
                &fixture.root,
                &fixture.index(),
                Path::new(""),
                &request("cancelcursorfixture"),
                None,
                &OperationCancellation::with_flag(flag),
                Duration::from_secs(5),
            )
        },
    );
    assert!(
        matches!(result, Err(ManifestError::Cancelled)),
        "{result:?}"
    );
    assert_eq!(
        statements
            .iter()
            .filter(|sql| sql.starts_with("WITH ranked_matches AS MATERIALIZED"))
            .count(),
        1
    );
}

#[test]
fn export_marker_at_workspace_root_revokes_cached_and_direct_extraction() {
    let fixture = Fixture::new("export-root-marker-cached");
    let private = b"exportrootsecretfixture private history\n";
    fixture.write("learner.json", private);
    let before = fixture.search(request("exportrootsecretfixture"));
    assert_eq!(before.hits.len(), 1);
    fixture.write(".magnesium-export", b"magnesium-export-v2\n");
    let after = search_workspace_sources(
        &fixture.root,
        &fixture.index(),
        Path::new(""),
        &request("exportrootsecretfixture"),
        CoordinatorBudget {
            max_duration: Duration::ZERO,
            ..budget()
        },
    )
    .unwrap();
    assert!(after.hits.is_empty());
    assert!(
        crate::retrieval::discover_root(&fixture.root)
            .unwrap()
            .entries
            .iter()
            .all(|entry| matches!(
                entry.disposition,
                crate::retrieval::DiscoveryDisposition::Excluded(_)
            ))
    );
    assert!(
        crate::retrieval::manifest::extract::extract_source(
            &fixture.root,
            &fixture.index(),
            Path::new("learner.json")
        )
        .unwrap_err()
        .to_string()
        .contains("excluded")
    );
    assert_eq!(
        fs::read(fixture.root.join("learner.json")).unwrap(),
        private
    );
}

#[test]
fn export_marker_revokes_cached_sources_without_reconciliation() {
    let fixture = Fixture::new("export-marker-cached");
    let private = b"isolationfixture isolationfixture isolationfixture private historical answer\n";
    let course = b"isolationfixture ordinary course material\n";
    fixture.write("aaa-review/learner.json", private);
    fixture.write("course.md", course);
    let before = fixture.search(request("isolationfixture"));
    assert_eq!(before.hits.len(), 2);
    assert_eq!(before.hits[0].candidate.path, "aaa-review/learner.json");

    fixture.write("aaa-review/.magnesium-export", b"magnesium-export-v2\n");
    let after = search_workspace_sources(
        &fixture.root,
        &fixture.index(),
        Path::new(""),
        &request("isolationfixture"),
        CoordinatorBudget {
            max_duration: Duration::ZERO,
            ..budget()
        },
    )
    .unwrap();
    assert_eq!(
        after
            .hits
            .iter()
            .map(|hit| hit.candidate.path.as_str())
            .collect::<Vec<_>>(),
        ["course.md"]
    );
    assert_eq!(
        fs::read(fixture.root.join("aaa-review/learner.json")).unwrap(),
        private
    );
    assert_eq!(fs::read(fixture.root.join("course.md")).unwrap(), course);

    // A revoked top hit must not consume the caller's result limit.
    let mut one = request("isolationfixture");
    one.limit = 1;
    let after = search_workspace_sources(
        &fixture.root,
        &fixture.index(),
        Path::new(""),
        &one,
        CoordinatorBudget {
            max_duration: Duration::ZERO,
            ..budget()
        },
    )
    .unwrap();
    assert_eq!(after.hits.len(), 1);
    assert_eq!(after.hits[0].candidate.path, "course.md");
}

#[test]
fn hand_authored_rankings_cover_identifiers_paths_phrases_prefixes_and_terms() {
    let fixture = Fixture::new("rankings");
    fixture.write(
        "src/parser.py",
        b"def parse_course_code(value):\n    # Parse CS-101 course notation.\n    return value\n",
    );
    fixture.write(
        "docs/retrieval.md",
        b"# Retrieval Practice\n\nSpaced retrieval practice improves durable recall.\n",
    );
    fixture.write(
        "docs/reference.md",
        b"The parse_course_code helper is listed in the API reference.\n",
    );

    // Code files are split as plain text now. The identifier tier still
    // ranks the function pieces an index built by an earlier version holds.
    fixture.search(request("parse_course_code"));
    fixture.store_earlier_code_pieces(
        "src/parser.py",
        vec![EarlierCodePiece {
            name: "parse_course_code",
            lines: (1, 3),
            body: "def parse_course_code(value):\n    # Parse CS-101 course notation.\n    return value\n",
            imports: &[],
            references: &["value"],
        }],
    );
    let identifier = fixture.search(request("parse_course_code"));
    assert_eq!(identifier.hits[0].candidate.path, "src/parser.py");
    assert!(identifier.hits[0].signals.exact_identifier);
    assert_eq!(identifier.hits[0].signals.lexical_rank, 1);
    assert_eq!(identifier.hits[0].candidate.anchor_start, 1);
    assert_eq!(
        identifier.hits,
        fixture.search(request("parse_course_code")).hits
    );

    let path = fixture.search(request("src/parser.py"));
    assert_eq!(path.hits[0].candidate.path, "src/parser.py");
    assert!(path.hits[0].signals.exact_path);

    let phrase = fixture.search(request("\"retrieval practice\""));
    assert_eq!(phrase.hits[0].candidate.path, "docs/retrieval.md");
    assert!(
        phrase.hits[0]
            .candidate
            .body
            .to_ascii_lowercase()
            .contains("retrieval practice")
    );

    let prefix = fixture.search(request("retriev*"));
    assert_eq!(prefix.hits[0].candidate.path, "docs/retrieval.md");
    let course = fixture.search(request("CS-101"));
    assert_eq!(course.hits[0].candidate.path, "src/parser.py");
    let natural = fixture.search(request("spaced durable"));
    assert_eq!(natural.hits[0].candidate.path, "docs/retrieval.md");

    let mut docs_only = request("parse_course_code");
    docs_only.path_prefix = Some(PathBuf::from("docs"));
    assert_eq!(
        fixture.search(docs_only).hits[0].candidate.path,
        "docs/reference.md"
    );
}

#[test]
fn exact_identifier_tier_matches_names_and_not_definition_kinds() {
    let fixture = Fixture::new("identifier-kinds");
    fixture.write(
            "src/area.py",
            b"from geometry import square\n\ndef compute_area(width, height):\n    return width * height\n",
        );
    fixture.write(
        "notes/functions.md",
        b"# Functions\n\nA function maps each input to one output.\n",
    );

    fixture.search(request("function"));
    fixture.store_earlier_code_pieces(
        "src/area.py",
        vec![EarlierCodePiece {
            name: "compute_area",
            lines: (3, 4),
            body: "def compute_area(width, height):\n    return width * height\n",
            imports: &[("geometry", "square")],
            references: &["width", "height"],
        }],
    );
    let kind = fixture.search(request("function"));
    assert_eq!(kind.hits[0].candidate.path, "notes/functions.md");
    assert!(
        kind.hits.iter().all(|hit| !hit.signals.exact_identifier),
        "a definition kind matched as an identifier: {:?}",
        kind.hits
    );
    for name in ["compute_area", "width", "geometry", "square"] {
        let identifier = fixture.search(request(name));
        assert_eq!(identifier.hits[0].candidate.path, "src/area.py", "{name}");
        assert!(identifier.hits[0].signals.exact_identifier, "{name}");
    }
}

#[test]
fn document_phrase_and_full_tokens_outrank_incidental_page_numbers() {
    let fixture = Fixture::new("chapter-intent");
    fixture.write("chapter6.md", b"Chapter 6. Transformations. Page 7.");
    fixture.write("syllabus.md", b"Course syllabus. Page 7.");
    fixture.write("chapter7.md", b"Chapter 7. Sampling distributions.");
    fixture.write("chapter17.md", b"Chapter 17. Other distributions.");
    let hits = fixture.search(request("Chapter 7")).hits;
    assert_eq!(hits[0].candidate.path, "chapter7.md", "{hits:?}");
    let strict = fixture.search(request("\"Chapter 7\"")).hits;
    assert!(!strict.is_empty());
    assert!(strict.iter().all(|hit| hit.candidate.path == "chapter7.md"));
    fixture.write("chapter7.md", b"Chapter\n7\nSampling distributions");
    assert_eq!(
        fixture.search(request("chapter 7")).hits[0].candidate.path,
        "chapter7.md"
    );
    let fallback = fixture.search(request("sampling unknownterm"));
    assert!(!fallback.hits.is_empty());
    assert!(
        fallback
            .hits
            .iter()
            .all(|hit| !hit.signals.full_token_match)
    );
    let mut one = request("Chapter 7");
    one.limit = 1;
    assert_eq!(fixture.search(one).hits[0].candidate.path, "chapter7.md");
}

#[test]
fn relevance_orders_identical_match_flags_even_with_later_filename() {
    let fixture = Fixture::new("relevance-tiebreak");
    // Both documents satisfy the phrase and full-token tiers; a.md is
    // alphabetically first but its match is diluted by irrelevant text.
    let diluted = format!("normal approximation {}", "irrelevant ".repeat(200));
    fixture.write("a.md", diluted.as_bytes());
    fixture.write(
        "b.md",
        b"normal approximation normal approximation normal approximation",
    );

    let mut one = request("\"normal approximation\"");
    one.limit = 1;
    let top = fixture.search(one);
    assert_eq!(
        top.hits[0].candidate.path,
        "b.md",
        "the stronger match must win the single result slot: {:?}",
        top.hits
            .iter()
            .map(|hit| (&hit.candidate.path, hit.signals))
            .collect::<Vec<_>>()
    );

    let ranked = fixture.search(request("\"normal approximation\""));
    assert_eq!(ranked.hits[0].candidate.path, "b.md");
    assert_eq!(ranked.hits[1].candidate.path, "a.md");
    for hit in &ranked.hits {
        assert!(
            hit.signals.phrase_match && hit.signals.full_token_match,
            "both documents must share the match flags: {hit:?}"
        );
    }
}

#[test]
fn repeated_navigation_lines_do_not_outrank_body_supported_matches() {
    let fixture = Fixture::new("navigation-ranking");
    fixture.write("chapter7.md", chapter7_navigation_fixture().as_bytes());

    // Every chunk carries the same "chapter map" navigation line, so the
    // original FTS index matches all of them for this phrase.
    let phrase = fixture.search(request("\"normal approximation\""));
    assert!(
        phrase.hits.len() > 1,
        "navigation-only matches must remain findable: {phrase:?}"
    );
    assert!(
        phrase.hits[0]
            .candidate
            .body
            .contains("Use the normal approximation"),
        "the body-supported definition must rank first: {:?}",
        phrase
            .hits
            .iter()
            .map(|hit| (&hit.candidate.body, &hit.signals))
            .collect::<Vec<_>>()
    );
    assert!(phrase.hits[0].signals.full_token_match);
    assert!(phrase.hits[0].signals.phrase_match);

    let navigation_only = phrase
        .hits
        .iter()
        .find(|hit| hit.candidate.body.contains("central limit theorem"))
        .expect("the navigation-only page must still be returned");
    assert!(
        !navigation_only.signals.full_token_match && !navigation_only.signals.phrase_match,
        "a navigation-only match must not claim body support: {:?}",
        navigation_only.signals
    );
    assert!(
        navigation_only
            .candidate
            .body
            .contains("Chapter 7 map: CLT"),
        "derived ranking data must never delete stored source content"
    );

    let outline = fixture.search(request("Chapter 7 Sampling Distributions"));
    assert!(
        outline.hits.iter().any(|hit| {
            hit.candidate.body.contains("Cover notes")
                && hit.signals.full_token_match
                && hit.signals.phrase_match
        }),
        "the outline/cover must stay body-supported: {:?}",
        outline
            .hits
            .iter()
            .map(|hit| (&hit.candidate.body, &hit.signals))
            .collect::<Vec<_>>()
    );
}

#[test]
fn incompatible_ranked_view_is_rejected_without_modifying_sources_or_cache() {
    let fixture = Fixture::new("navigation-incompatible");
    let original = chapter7_navigation_fixture();
    fixture.write("chapter7.md", original.as_bytes());
    let before = fixture.search(request("\"normal approximation\""));
    assert!(before.hits[0].signals.phrase_match);
    Connection::open(fixture.index())
        .unwrap()
        .execute_batch("DROP TABLE source_search_ranked; PRAGMA user_version = 7;")
        .unwrap();
    let cache = fs::read(fixture.index()).unwrap();
    let error = search_workspace_sources(
        &fixture.root,
        &fixture.index(),
        Path::new(""),
        &request("\"normal approximation\""),
        budget(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("run magnesium init"), "{error}");
    assert_eq!(fs::read(fixture.index()).unwrap(), cache);
    assert_eq!(
        fs::read(fixture.root.join("chapter7.md")).unwrap(),
        original.as_bytes()
    );
}

fn chapter7_navigation_fixture() -> String {
    let navigation =
        "Chapter 7 map: CLT · normal approximation · t distribution · confidence intervals";
    let mut document = String::from(
        "# Chapter 7: Sampling Distributions\n\nCover notes for the sampling distributions chapter.\n",
    );
    document.push_str(navigation);
    document.push('\n');
    for (heading, body) in [
        (
            "Central Limit Theorem",
            "The central limit theorem says the sample mean distribution approaches a normal distribution as the sample size grows.",
        ),
        (
            "Normal Approximation",
            "Use the normal approximation to the binomial when np and n(1-p) are both large enough.",
        ),
        (
            "t Distribution",
            "The t distribution has heavier tails than the normal distribution for small samples.",
        ),
        (
            "Confidence Intervals",
            "A confidence interval combines a point estimate with a margin of error.",
        ),
        (
            "Standard Error",
            "The standard error measures the variability of a sample statistic across samples.",
        ),
        (
            "Sampling Variability",
            "Sampling variability describes how estimates differ from sample to sample.",
        ),
    ] {
        document.push_str(&format!("\n## {heading}\n\n{body}\n"));
        document.push_str(navigation);
        document.push('\n');
    }
    document
}

#[test]
fn active_workspace_and_query_scope_are_hard_filters() {
    let first = Fixture::new("workspace-first");
    let second = Fixture::new("workspace-second");
    first.write("docs/allowed.md", b"workspace sentinel allowed\n");
    first.write("private/hidden.md", b"workspace sentinel hidden\n");
    second.write("docs/other.md", b"workspace sentinel other\n");

    let error = search_workspace_sources(
        &first.root,
        &second.index(),
        Path::new(""),
        &request("workspace"),
        budget(),
    )
    .expect_err("a workspace cannot query another workspace index");
    assert!(error.to_string().contains("not the active workspace index"));

    let scoped = search_workspace_sources(
        &first.root,
        &first.index(),
        Path::new("docs"),
        &request("workspace"),
        budget(),
    )
    .expect("search one workspace scope");
    assert_eq!(scoped.hits.len(), 1);
    assert_eq!(scoped.hits[0].candidate.path, "docs/allowed.md");

    let error = retrieve_workspace_sources(
        &first.root,
        &first.index(),
        Path::new(""),
        &RetrievalRequest {
            active_workspace_id: "wrong-workspace".into(),
            lexical: request("workspace"),
        },
        budget(),
    )
    .expect_err("retrieval authority cannot relabel an active workspace");
    assert!(
        error
            .to_string()
            .contains("does not match active workspace")
    );
}

#[test]
fn exact_allowlist_does_not_reconcile_or_search_unapproved_paths() {
    let cold = Fixture::new("allowlist-cold");
    cold.write("approved.md", b"allowlist sentinel approved\n");
    cold.write("unapproved.md", b"allowlist sentinel unapproved\n");
    let allowed = BTreeSet::from([PathBuf::from("approved.md")]);
    let restricted = search_workspace_sources_with_allowlist(
        &cold.root,
        &cold.index(),
        Path::new(""),
        &request("allowlist"),
        budget(),
        Some(&allowed),
    )
    .expect("search the exact allowlist");
    assert_eq!(
        restricted
            .hits
            .iter()
            .map(|hit| hit.candidate.path.as_str())
            .collect::<Vec<_>>(),
        ["approved.md"]
    );
    assert_eq!(manifest_paths(&cold.index()), ["approved.md"]);

    let warm = Fixture::new("allowlist-warm");
    warm.write("approved.md", b"allowlist sentinel approved\n");
    warm.write("unapproved.md", b"allowlist sentinel unapproved\n");
    warm.search(request("allowlist"));
    warm.write("new-unapproved.md", b"allowlist sentinel new\n");
    search_workspace_sources(
        &warm.root,
        &warm.index(),
        Path::new(""),
        &request("allowlist"),
        CoordinatorBudget {
            max_duration: Duration::ZERO,
            ..budget()
        },
    )
    .expect("preserve an in-progress unrestricted cursor");
    let before = index_snapshot(&warm.index());
    let restricted = search_workspace_sources_with_allowlist(
        &warm.root,
        &warm.index(),
        Path::new(""),
        &request("allowlist"),
        budget(),
        Some(&allowed),
    )
    .expect("reuse the exact allowlist on a warm index");
    assert!(
        restricted
            .hits
            .iter()
            .all(|hit| hit.candidate.path == "approved.md")
    );
    let after = index_snapshot(&warm.index());
    assert_eq!(
        before
            .manifest
            .iter()
            .find(|(path, ..)| path == "unapproved.md"),
        after
            .manifest
            .iter()
            .find(|(path, ..)| path == "unapproved.md")
    );
    assert_eq!(before.generation, after.generation);
    let before_cursor: serde_json::Value =
        serde_json::from_str(before.cursor.as_deref().expect("warm cursor"))
            .expect("parse warm cursor");
    let after_cursor: serde_json::Value =
        serde_json::from_str(after.cursor.as_deref().expect("restricted cursor"))
            .expect("parse restricted cursor");
    assert_eq!(before_cursor["directories"], after_cursor["directories"]);
    assert!(
        !after
            .manifest
            .iter()
            .any(|(path, ..)| path == "new-unapproved.md")
    );

    let spanning = Fixture::new("allowlist-budget");
    spanning.write("approved-a.md", b"allowlist sentinel approved a\n");
    spanning.write("approved-b.md", b"allowlist sentinel approved b\n");
    spanning.write("unapproved.md", b"allowlist sentinel unapproved\n");
    let spanning_allowed = BTreeSet::from([
        PathBuf::from("approved-a.md"),
        PathBuf::from("approved-b.md"),
    ]);
    let one_at_a_time = CoordinatorBudget {
        max_on_demand: 1,
        ..budget()
    };
    search_workspace_sources_with_allowlist(
        &spanning.root,
        &spanning.index(),
        Path::new(""),
        &request("allowlist"),
        one_at_a_time,
        Some(&spanning_allowed),
    )
    .expect("index the first approved pending source");
    assert_eq!(
        manifest_statuses(&spanning.index()),
        vec![
            ("approved-a.md".to_owned(), "indexed".to_owned()),
            ("approved-b.md".to_owned(), "pending".to_owned())
        ]
    );
    search_workspace_sources_with_allowlist(
        &spanning.root,
        &spanning.index(),
        Path::new(""),
        &request("allowlist"),
        one_at_a_time,
        Some(&spanning_allowed),
    )
    .expect("index the next approved pending source");
    assert_eq!(
        manifest_statuses(&spanning.index()),
        vec![
            ("approved-a.md".to_owned(), "indexed".to_owned()),
            ("approved-b.md".to_owned(), "indexed".to_owned())
        ]
    );
    assert!(!manifest_paths(&spanning.index()).contains(&"unapproved.md".to_owned()));
}

#[test]
fn retrieval_returns_exact_content_only_for_the_active_workspace() {
    let fixture = Fixture::new("boundary-workspace");
    fixture.write(
        "source.md",
        b"# Original source\n\nboundarysignal exact returned content\n",
    );

    let report = fixture.retrieve(retrieval_request(request("boundarysignal")));
    assert_eq!(report.hits.len(), 1);
    let hit = &report.hits[0];
    assert_eq!(hit.candidate.path, "source.md");
    assert!(hit.candidate.body.contains("exact returned content"));

    let foreign = retrieve_workspace_sources(
        &fixture.root,
        &fixture.index(),
        Path::new(""),
        &RetrievalRequest {
            active_workspace_id: "another-workspace".into(),
            lexical: request("boundarysignal"),
        },
        budget(),
    )
    .expect_err("a foreign workspace identity must not search this index");
    assert!(
        foreign
            .to_string()
            .contains("does not match active workspace"),
        "{foreign}"
    );
}

#[test]
fn mutation_and_rename_update_search_without_stale_rows() {
    let fixture = Fixture::new("incremental");
    fixture.write("lesson.md", b"# Lesson\n\noldmarker retained concept\n");
    assert_eq!(
        fixture.search(request("oldmarker")).hits[0].candidate.path,
        "lesson.md"
    );

    fixture.write(
        "lesson.md",
        b"# Lesson\n\nnewmarker replaces the earlier source content completely\n",
    );
    assert!(fixture.search(request("oldmarker")).hits.is_empty());
    assert_eq!(
        fixture.search(request("newmarker")).hits[0].candidate.path,
        "lesson.md"
    );

    fs::rename(
        fixture.root.join("lesson.md"),
        fixture.root.join("renamed.md"),
    )
    .expect("rename indexed source");
    let renamed = fixture.search(request("renamed.md"));
    assert_eq!(renamed.hits[0].candidate.path, "renamed.md");
    assert!(renamed.hits[0].signals.exact_path);
}

#[test]
fn manually_rebuilding_search_is_semantically_equivalent_and_canonical_neutral() {
    let fixture = Fixture::new("rebuild");
    fixture.write(
        "guide.md",
        b"# Rebuildable Search\n\nrebuild sentinel with exact provenance\n",
    );
    fixture.seed_canonical();
    let before = fixture.semantic_results("rebuild");
    let canonical_before = fixture.canonical_value();

    // Follow the explicit recovery instruction: preserve the complete
    // cache artifact set together, rather than adopting orphan sidecars.
    let preserved = fixture
        .root
        .join(crate::workspace::CONTROL_DIR)
        .join("backups/source-cache");
    fs::create_dir_all(&preserved).unwrap();
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let name = format!("source-search.sqlite{suffix}");
        let path = fixture.index().parent().unwrap().join(&name);
        if path.exists() {
            fs::rename(path, preserved.join(name)).unwrap();
        }
    }
    let after = fixture.semantic_results("rebuild");

    assert_eq!(after, before);
    assert_eq!(fixture.canonical_value(), canonical_before);
}

#[test]
fn malformed_match_syntax_is_rejected_before_index_creation() {
    let fixture = Fixture::new("invalid-query");
    let error = search_workspace_sources(
        &fixture.root,
        &fixture.index(),
        Path::new(""),
        &request("unterminated \"phrase"),
        budget(),
    )
    .expect_err("reject malformed phrase");
    assert!(error.to_string().contains("unterminated phrase"));
    assert!(!fixture.index().exists());
}

#[test]
fn uncancelled_search_over_a_matching_corpus_is_not_reported_as_cancelled() {
    // Regression: the SQLite progress handler inverted its polarity, so
    // every noncancelled FTS scan that reached the 1,000-VM interval was
    // aborted and relabeled `Cancelled`. A matching corpus of this size
    // reaches the interval; the ordinary path must return hits.
    let fixture = Fixture::new("uncancelled-corpus");
    for index in 0..40 {
        fixture.write(
            &format!("notes/retrieval-{index}.md"),
            format!("# Retrieval notes {index}\n\nretrieval practice entry {index}\n").as_bytes(),
        );
    }
    // Corpus preparation has its own bounded setup budget. Under a loaded
    // aggregate, the ordinary one-second cold-index budget can legitimately
    // return a partial corpus (e.g. nine hits), which does not exercise the
    // SQL progress-handler polarity regression this test is meant to guard.
    prepare_source_query_with_allowlist_and_cancellation(
        &fixture.root,
        &fixture.index(),
        Path::new(""),
        &[],
        CoordinatorBudget {
            max_on_demand: 64,
            max_duration: Duration::from_secs(30),
            ..budget()
        },
        None,
        &OperationCancellation::none(),
    )
    .expect("prepare the complete matching fixture");
    // IndexingOnDemand describes work performed, not incomplete contents.
    // Check the persisted fixture itself before exercising the SQL handler.
    let coverage = crate::retrieval::manifest::manifest_coverage(&fixture.index()).unwrap();
    assert!(!coverage.scan_active);
    assert_eq!((coverage.pending, coverage.stale), (0, 0));
    let mut expected = (0..40)
        .map(|n| (format!("notes/retrieval-{n}.md"), "indexed".to_owned()))
        .collect::<Vec<_>>();
    expected.sort();
    let actual = manifest_statuses(&fixture.index())
        .into_iter()
        .filter(|(path, _)| path.starts_with("notes/retrieval-"))
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
    let hits = query_index_with_budget(
        &fixture.root,
        &fixture.index(),
        Path::new(""),
        &request("retrieval"),
        None,
        &OperationCancellation::none(),
        Duration::from_secs(5),
    )
    .expect("an uncancelled corpus query must return results, not Cancelled");
    assert_eq!(hits.len(), 10);
    assert!(
        hits.iter()
            .all(|hit| hit.candidate.path.starts_with("notes/retrieval-")),
        "unexpected corpus hits: {:?}",
        hits.iter()
            .map(|hit| hit.candidate.path.as_str())
            .collect::<Vec<_>>()
    );
}

fn fixture_with_unextractable_sources(name: &str) -> Fixture {
    let fixture = Fixture::new(name);
    fixture.write(
        "broken.pdf",
        b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>\nendobj\ntrailer\n<< /Root 1 0 R >>\n%%EOF\n",
    );
    fixture.write("garbled.md", b"\xff\xfe not utf-8 \x80\x81\n");
    fixture.write("panics.pdf", &pdf_that_panics_the_parser());
    for index in 0..3 {
        fixture.write(
            &format!("note{index}.md"),
            format!("# Note {index}\n\nrecursion base case {index}\n").as_bytes(),
        );
    }
    fixture
}

/// A page without a media box makes the parser panic. The extractor
/// reports that as a per-document failure, like a PDF over the page limit.
fn pdf_that_panics_the_parser() -> Vec<u8> {
    use pdf_extract::{Document, Object, dictionary};
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let page_id = document.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id });
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1,
        }),
    );
    let catalog_id = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    document.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).unwrap();
    bytes
}

#[test]
fn sources_that_fail_extraction_do_not_fail_a_search_over_the_others() {
    let fixture = fixture_with_unextractable_sources("unextractable-fallback");
    for round in 0..2 {
        let report = search_workspace_sources(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &request("recursion base case"),
            budget(),
        )
        .unwrap_or_else(|error| panic!("round {round}: {error}"));
        assert_eq!(report.hits.len(), 3, "round {round}");
    }
    let statuses = manifest_statuses(&fixture.index());
    for broken in ["broken.pdf", "garbled.md", "panics.pdf"] {
        assert!(
            statuses.contains(&(broken.to_owned(), "failed".to_owned())),
            "{broken} stays recorded as failed: {statuses:?}"
        );
    }
}

#[test]
fn source_named_by_the_query_that_fails_extraction_does_not_fail_the_search() {
    let fixture = fixture_with_unextractable_sources("unextractable-on-demand");
    for named in ["broken.pdf", "garbled.md", "panics.pdf"] {
        let result = search_workspace_sources(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &request(named),
            budget(),
        );
        assert!(result.is_ok(), "{named}: {:?}", result.err());
    }
}

#[test]
fn sql_budget_exhaustion_and_explicit_cancellation_have_distinct_recovery() {
    let fixture = Fixture::new("deadline-vs-cancel");
    for index in 0..40 {
        fixture.write(
            &format!("lesson-{index}.md"),
            b"# Shared concept\n\nA shared matching concept.\n",
        );
    }
    search_workspace_sources(
        &fixture.root,
        &fixture.index(),
        Path::new(""),
        &request("shared concept"),
        budget(),
    )
    .unwrap();
    let exhausted = query_index_with_budget(
        &fixture.root,
        &fixture.index(),
        Path::new(""),
        &request("shared concept"),
        None,
        &OperationCancellation::none(),
        std::time::Duration::ZERO,
    )
    .unwrap_err();
    assert!(
        matches!(exhausted, ManifestError::BudgetExhausted),
        "{exhausted}"
    );
    assert!(exhausted.to_string().contains("narrow the query"));
    assert!(exhausted.to_string().contains("incomplete"));
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let cancelled = query_index_with_budget(
        &fixture.root,
        &fixture.index(),
        Path::new(""),
        &request("shared concept"),
        None,
        &OperationCancellation::with_flag(flag),
        std::time::Duration::from_secs(1),
    )
    .unwrap_err();
    assert!(matches!(cancelled, ManifestError::Cancelled), "{cancelled}");
    assert!(
        cancelled
            .to_string()
            .contains("retry only if retrieval is still wanted")
    );
}

#[test]
fn cancelled_search_aborts_without_building_the_index() {
    let fixture = Fixture::new("cancelled-search");
    fixture.write("lesson.md", b"cancelled sentinel\n");
    let flag = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let cancellation = OperationCancellation::with_flag(flag);
    let error = search_workspace_sources_with_allowlist_and_cancellation(
        &fixture.root,
        &fixture.index(),
        Path::new(""),
        &request("cancelled"),
        budget(),
        None,
        &cancellation,
    )
    .expect_err("pre-cancelled search must abort");
    assert!(
        matches!(error, ManifestError::Cancelled),
        "unexpected cancelled-search error: {error}"
    );
    assert!(
        !fixture.index().exists(),
        "a cancelled search must not build the source index"
    );
}

fn request(query: &str) -> SourceSearchRequest {
    SourceSearchRequest {
        query: query.into(),
        path_prefix: None,
        limit: 10,
    }
}

fn budget() -> CoordinatorBudget {
    CoordinatorBudget {
        max_metadata_entries: 128,
        max_duration: Duration::from_secs(1),
        max_on_demand: 16,
    }
}

fn retrieval_request(lexical: SourceSearchRequest) -> RetrievalRequest {
    RetrievalRequest {
        active_workspace_id: "workspace".into(),
        lexical,
    }
}

fn manifest_paths(index: &Path) -> Vec<String> {
    let connection = Connection::open(index).expect("open source manifest");
    let mut statement = connection
        .prepare(
            "SELECT display_path FROM source_manifest
                 WHERE present = 1 ORDER BY display_path COLLATE BINARY",
        )
        .expect("prepare source manifest paths");
    statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query source manifest paths")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("read source manifest paths")
}

fn manifest_statuses(index: &Path) -> Vec<(String, String)> {
    let connection = Connection::open(index).expect("open source manifest statuses");
    let mut statement = connection
        .prepare(
            "SELECT display_path, index_status FROM source_manifest
                 WHERE present = 1 ORDER BY display_path COLLATE BINARY",
        )
        .expect("prepare source manifest statuses");
    statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .expect("query source manifest statuses")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("read source manifest statuses")
}

type ManifestSnapshotRow = (String, String, Option<String>, i64, Option<String>);

#[derive(Debug, Eq, PartialEq)]
struct IndexSnapshot {
    manifest: Vec<ManifestSnapshotRow>,
    generation: i64,
    cursor: Option<String>,
}

fn index_snapshot(index: &Path) -> IndexSnapshot {
    let connection = Connection::open(index).expect("open source index snapshot");
    let mut statement = connection
        .prepare(
            "SELECT display_path, index_status, content_sha256,
                        observed_generation, last_error
                 FROM source_manifest
                 WHERE present = 1 ORDER BY display_path COLLATE BINARY",
        )
        .expect("prepare source index snapshot");
    let manifest = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })
        .expect("query source index snapshot")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("read source index snapshot");
    let generation = connection
        .query_row(
            "SELECT generation FROM source_index_metadata WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .expect("read source index generation");
    let cursor = connection
        .query_row(
            "SELECT cursor_json FROM source_reconciliation_state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .expect("read source index cursor");
    IndexSnapshot {
        manifest,
        generation,
        cursor,
    }
}

struct Fixture {
    root: PathBuf,
}

/// One function piece of a Python file, as an earlier index stored it.
struct EarlierCodePiece {
    name: &'static str,
    lines: (u32, u32),
    body: &'static str,
    imports: &'static [(&'static str, &'static str)],
    references: &'static [&'static str],
}

impl Fixture {
    fn new(name: &str) -> Self {
        let sequence = NEXT_ROOT.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test clock after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "magnesium-search-{}-{name}-{nanos}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(root.join(crate::workspace::CONTROL_DIR).join("index"))
            .expect("create search fixture");
        fs::write(
            root.join(crate::workspace::CONTROL_DIR).join("config.toml"),
            "workspace_id = \"workspace\"\n",
        )
        .expect("write search workspace identity");
        Self { root }
    }

    fn index(&self) -> PathBuf {
        self.root
            .join(crate::workspace::CONTROL_DIR)
            .join("index/source-search.sqlite")
    }

    fn canonical(&self) -> PathBuf {
        self.root
            .join(crate::workspace::CONTROL_DIR)
            .join("canonical.sqlite")
    }

    fn write(&self, relative: &str, contents: &[u8]) {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().expect("fixture parent")).expect("create fixture parent");
        fs::write(path, contents).expect("write search fixture");
    }

    fn search(&self, request: SourceSearchRequest) -> SourceSearchReport {
        search_workspace_sources(&self.root, &self.index(), Path::new(""), &request, budget())
            .expect("search fixture")
    }

    /// Replace the indexed pieces of `path` with code pieces of the shape
    /// earlier versions cut from code files, as an index built then holds.
    fn store_earlier_code_pieces(&self, path: &str, pieces: Vec<EarlierCodePiece>) {
        use crate::retrieval::chunking::{
            ChunkDraft, ChunkKind, SourceImport, SymbolDefinition, SymbolReference,
        };
        use crate::retrieval::manifest::{CurrentSource, extract};

        let mut connection = Connection::open(self.index()).unwrap();
        let source = connection
            .query_row(
                "SELECT manifest_id, display_path, source_kind, content_sha256, index_status, source_revision
                 FROM source_manifest WHERE display_path = ?1",
                [path],
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
            .unwrap();
        let drafts = pieces
            .into_iter()
            .map(|piece| ChunkDraft {
                kind: ChunkKind::Symbol,
                line_start: piece.lines.0,
                line_end: piece.lines.1,
                body: piece.body.to_owned(),
                structural_path: Some(piece.name.to_owned()),
                language: Some("python"),
                definitions: vec![SymbolDefinition {
                    name: piece.name.to_owned(),
                    kind: "function".into(),
                    line_start: piece.lines.0,
                    line_end: piece.lines.1,
                }],
                imports: piece
                    .imports
                    .iter()
                    .map(|(module, name)| SourceImport {
                        module: (*module).to_owned(),
                        names: vec![(*name).to_owned()],
                        line_start: 1,
                        line_end: 1,
                    })
                    .collect(),
                references: piece
                    .references
                    .iter()
                    .map(|name| SymbolReference {
                        name: (*name).to_owned(),
                        line: piece.lines.0,
                    })
                    .collect(),
            })
            .collect();
        extract::store_candidates(&mut connection, &self.index(), &source, drafts, None).unwrap();
    }

    fn retrieve(&self, request: RetrievalRequest) -> SourceSearchReport {
        retrieve_workspace_sources(&self.root, &self.index(), Path::new(""), &request, budget())
            .expect("retrieve fixture")
    }

    fn seed_canonical(&self) {
        let connection = Connection::open(self.canonical()).expect("open canonical sentinel");
        connection
            .execute_batch(
                "CREATE TABLE canonical_sentinel(value TEXT NOT NULL) STRICT;
                     INSERT INTO canonical_sentinel(value) VALUES ('unchanged');",
            )
            .expect("seed canonical sentinel");
    }

    fn canonical_value(&self) -> String {
        Connection::open(self.canonical())
            .expect("open canonical sentinel")
            .query_row("SELECT value FROM canonical_sentinel", [], |row| row.get(0))
            .expect("read canonical sentinel")
    }

    fn semantic_results(&self, query: &str) -> Vec<(String, u32, u32, String, String)> {
        self.search(request(query))
            .hits
            .into_iter()
            .map(|hit| {
                (
                    hit.candidate.path,
                    hit.candidate.anchor_start,
                    hit.candidate.anchor_end,
                    hit.candidate.content_sha256,
                    hit.candidate.body,
                )
            })
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
