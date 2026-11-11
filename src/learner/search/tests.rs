use super::*;
pub fn fixture() -> Connection {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch(crate::learner::store::SCHEMA_SQL).unwrap();
    c.execute("INSERT INTO workspace_meta(singleton,workspace_id,storage_family,schema_version,created_at_ms) VALUES(1,'w-test','learner-model/v2',?1,0)",[super::super::store::SCHEMA_VERSION]).unwrap();
    c
}
pub fn memory(c: &Connection, id: &str, title: &str, state: &str, cues: &str) {
    c.execute("INSERT INTO learner_memories(memory_id,title,scope,summary,state_text,retrieval_cues_json,current_basis_json,lifecycle,revision,created_at_ms,updated_at_ms) VALUES(?1,?2,'algebra',?3,?3,?4,'[]','active',1,0,0)",params![id,title,state,cues]).unwrap();
}
#[test]
fn whole_record_formulation_matches_alias_with_required_content() {
    let c = fixture();
    memory(
        &c,
        "quadratic",
        "Quadratic factorization",
        "Can expand; reversing expansion needs support.",
        r#"["undo FOIL"]"#,
    );
    rebuild_indexes(&c).unwrap();
    let intent = Intent {
        id: "reverse-expansion".into(),
        purpose: "Find the relevant factorization description".into(),
        queries: vec![Query {
            any_of: vec!["undo FOIL".into()],
            all_of: vec!["quadratic".into()],
        }],
    };
    let result = search(&c, &[intent], "", &[], None).unwrap();
    assert_eq!(
        result.intents[0]
            .1
            .iter()
            .map(|candidate| candidate.id.as_str())
            .collect::<Vec<_>>(),
        ["quadratic"],
        "the cue phrase and required title term belong to one current record"
    );
    assert_eq!(
        result.intents[0].1[0].channels,
        BTreeSet::from(["lexical".into()])
    );
}
#[test]
fn whole_record_formulation_matches_description_with_required_cue() {
    let c = fixture();
    memory(
        &c,
        "quadratic",
        "Factorization",
        "Can expand; reversing expansion needs support.",
        r#"["quadratic"]"#,
    );
    rebuild_indexes(&c).unwrap();
    let mut intent = Intent {
        id: "reverse-expansion".into(),
        purpose: "Find the relevant factorization description".into(),
        queries: vec![Query {
            any_of: vec!["reversing expansion".into()],
            all_of: vec!["quadratic".into()],
        }],
    };
    let result = search(&c, std::slice::from_ref(&intent), "", &[], None).unwrap();
    assert_eq!(result.intents[0].1[0].id, "quadratic");
    intent.queries[0].all_of.push("unavailable".into());
    let result = search(&c, &[intent], "", &[], None).unwrap();
    assert!(
        result.intents[0].1.is_empty(),
        "every required phrase must match somewhere in the same record"
    );
}
#[test]
fn whole_record_formulation_does_not_form_phrases_across_fields() {
    let c = fixture();
    memory(&c, "split", "undo", "FOIL", r#"["quadratic"]"#);
    memory(
        &c,
        "phrase",
        "Factorization",
        "reversing expansion",
        r#"["undo FOIL", "quadratic"]"#,
    );
    rebuild_indexes(&c).unwrap();
    let intent = Intent {
        id: "reverse-expansion".into(),
        purpose: "Keep literal phrase semantics".into(),
        queries: vec![Query {
            any_of: vec!["undo FOIL".into()],
            all_of: vec!["quadratic".into()],
        }],
    };
    let result = search(&c, &[intent], "", &[], None).unwrap();
    assert_eq!(result.intents[0].1.len(), 1);
    assert_eq!(result.intents[0].1[0].id, "phrase");
}
#[test]
fn whole_record_formulation_preserves_unicode_and_quoted_operators() {
    let c = fixture();
    memory(
        &c,
        "unicode",
        "Coefficient notation",
        "Uses x² − b in a café example.",
        r#"["cafe\u0301"]"#,
    );
    memory(&c, "quoted", "a OR b", "coefficient", "[]");
    memory(&c, "operator-decoy", "b", "coefficient", "[]");
    rebuild_indexes(&c).unwrap();
    let intents = [
        Intent {
            id: "unicode".into(),
            purpose: "NFC normalization across fields".into(),
            queries: vec![Query {
                any_of: vec!["cafe\u{301}".into()],
                all_of: vec!["coefficient".into()],
            }],
        },
        Intent {
            id: "quoted".into(),
            purpose: "Quoted input cannot broaden a formulation".into(),
            queries: vec![Query {
                any_of: vec!["a\" OR b".into()],
                all_of: vec!["coefficient".into()],
            }],
        },
    ];
    let result = search(&c, &intents, "", &["x² − b".into()], Some("unicode")).unwrap();
    assert_eq!(result.intents[0].1.len(), 1);
    assert_eq!(result.intents[0].1[0].id, "unicode");
    assert_eq!(
        result.intents[0].1[0].channels,
        BTreeSet::from(["lexical".into(), "literal".into()])
    );
    assert_eq!(result.intents[1].1.len(), 1);
    assert_eq!(result.intents[1].1[0].id, "quoted");
}
#[test]
fn whole_record_formulation_keeps_bounds_and_active_row_filter() {
    let c = fixture();
    for n in 0..26 {
        memory(&c, &format!("m-{n:02}"), "coefficient", "evidence", "[]");
    }
    memory(&c, "retired", "coefficient", "evidence", "[]");
    rebuild_indexes(&c).unwrap();
    // Even a stale document cannot bypass current canonical lifecycle.
    c.execute(
        "UPDATE learner_memories SET lifecycle='retired' WHERE memory_id='retired'",
        [],
    )
    .unwrap();
    let intent = Intent {
        id: "roles".into(),
        purpose: "Bounded active lexical results".into(),
        queries: vec![Query {
            any_of: vec!["coefficient".into()],
            all_of: vec![],
        }],
    };
    let result = search(&c, &[intent], "", &[], None).unwrap();
    assert_eq!(result.intents[0].1.len(), 24);
    assert_eq!(result.truncated, BTreeSet::from(["roles".into()]));
    assert!(result.intents[0].1.iter().all(|c| c.id != "retired"));
}
#[test]
fn literal_fts_operators_are_quoted_and_or_is_explicit() {
    let c = fixture();
    memory(&c, "a", "factoring", "does not confuse coefficients", "[]");
    memory(&c, "b", "factorization", "coefficient labels unclear", "[]");
    memory(&c, "c", "undo FOIL", "multiply binomials", "[]");
    rebuild_indexes(&c).unwrap();
    let i = Intent {
        id: "test".into(),
        purpose: "alternatives".into(),
        queries: vec![Query {
            any_of: vec![
                "factoring".into(),
                "factorization".into(),
                "undo FOIL".into(),
            ],
            all_of: vec![],
        }],
    };
    let s = search(&c, std::slice::from_ref(&i), "", &[], None).unwrap();
    assert_eq!(s.intents[0].1.len(), 3);
    let mut restricted = i;
    restricted.queries[0].all_of.push("coefficient".into());
    assert_eq!(
        search(&c, &[restricted], "", &[], None).unwrap().intents[0]
            .1
            .len(),
        1
    );
    assert_eq!(
        compile(&Query {
            any_of: vec!["a\" OR b".into()],
            all_of: vec![]
        })
        .unwrap(),
        "(\"a\"\" OR b\")"
    );
    assert_eq!(
        compile(&Query {
            any_of: vec!["!!!".into()],
            all_of: vec![]
        })
        .unwrap_err(),
        "no_search_terms"
    );
}
#[test]
fn cues_current_rows_literal_and_dirty_fallback() {
    let c = fixture();
    memory(
        &c,
        "a",
        "symbol roles",
        "x² − b is not a negative sign error",
        "[\"coefficient\"]",
    );
    memory(&c, "b", "coefficient", "old error", "[]");
    c.execute(
        "UPDATE learner_memories SET lifecycle='retired' WHERE memory_id='b'",
        [],
    )
    .unwrap();
    rebuild_indexes(&c).unwrap();
    let i = Intent {
        id: "roles".into(),
        purpose: "retrieval cue".into(),
        queries: vec![Query {
            any_of: vec!["coefficient".into()],
            all_of: vec![],
        }],
    };
    let s = search(&c, std::slice::from_ref(&i), "", &[], None).unwrap();
    assert_eq!(s.intents[0].1.len(), 1);
    assert_eq!(
        s.intents[0].1[0].channels,
        BTreeSet::from(["lexical".to_string()])
    );
    let s = search(&c, &[], "", &["x² − b".into()], None).unwrap();
    assert_eq!(s.intents[0].1[0].id, "a");
    c.execute("UPDATE workspace_meta SET memory_generation=1", [])
        .unwrap();
    assert!(!index_current(&c, false).unwrap());
    let s = search(&c, &[i], "", &[], None).unwrap();
    assert!(s.intents[0].1.is_empty());
    assert!(!s.warnings.is_empty());
    reindex(&c).unwrap();
    assert!(index_current(&c, false).unwrap());
}
#[test]
fn duplicate_queries_do_not_inflate_ranks() {
    let c = fixture();
    memory(&c, "a", "coefficient", "fixed labels", r#"["coefficient"]"#);
    rebuild_indexes(&c).unwrap();
    let q = Query {
        any_of: vec!["coefficient".into()],
        all_of: vec![],
    };
    let mut i = Intent {
        id: "roles".into(),
        purpose: "roles".into(),
        queries: vec![q.clone()],
    };
    let one = search(&c, std::slice::from_ref(&i), "", &[], None)
        .unwrap()
        .intents[0]
        .1[0]
        .score;
    assert_eq!(one, 1.0 / 61.0, "one formulation supplies one vote");
    i.queries.push(q);
    assert_eq!(
        search(&c, &[i], "", &[], None).unwrap().intents[0].1[0].score,
        one
    );
}
#[test]
fn distinct_formulations_fuse_within_each_intent() {
    let c = fixture();
    memory(&c, "a", "coefficient", "fixed labels", r#"["coefficient"]"#);
    rebuild_indexes(&c).unwrap();
    let query = Query {
        any_of: vec!["coefficient".into()],
        all_of: vec![],
    };
    let result = search(
        &c,
        &[
            Intent {
                id: "two-formulations".into(),
                purpose: "Two distinct matching opportunities".into(),
                queries: vec![
                    query.clone(),
                    Query {
                        any_of: vec!["fixed labels".into()],
                        all_of: vec![],
                    },
                ],
            },
            Intent {
                id: "one-formulation".into(),
                purpose: "The shared cache does not combine intents".into(),
                queries: vec![query],
            },
        ],
        "",
        &[],
        None,
    )
    .unwrap();
    assert_eq!(result.intents[0].1[0].score, 2.0 / 61.0);
    assert_eq!(result.intents[1].1[0].score, 1.0 / 61.0);
}

#[test]
fn selected_sync_work_does_not_scale_with_fts_corpus_size() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    let count_update = |size: usize| {
        let c = fixture();
        for index in 0..size {
            memory(
                &c,
                &format!("m-{index:05}"),
                "coefficient",
                "evidence",
                "[]",
            );
        }
        rebuild_indexes(&c).unwrap();
        c.execute(
            "UPDATE learner_memories SET title='revised coefficient' WHERE memory_id='m-00050'",
            [],
        )
        .unwrap();
        let steps = Arc::new(AtomicUsize::new(0));
        let counter = steps.clone();
        c.progress_handler(
            1,
            Some(move || {
                counter.fetch_add(1, Ordering::Relaxed);
                false
            }),
        )
        .unwrap();
        sync_memories(&c, &BTreeSet::from(["m-00050".into()])).unwrap();
        c.progress_handler(0, None::<fn() -> bool>).unwrap();
        assert_eq!(
            c.query_row("SELECT count(*) FROM memory_fts", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            size as i64
        );
        assert_eq!(c.query_row("SELECT title FROM memory_fts WHERE rowid=(SELECT memory_no FROM learner_memories WHERE memory_id='m-00050')", [], |row| row.get::<_, String>(0)).unwrap(), "revised coefficient");
        steps.load(Ordering::Relaxed)
    };
    let small = count_update(100);
    let large = count_update(10_000);
    println!("selected sync VM steps: 100 memories={small}, 10,000 memories={large}");
    assert!(
        large <= small * 3,
        "one selected update scanned the growing FTS corpus: 100 rows={small} VM steps, 10,000 rows={large} VM steps"
    );
}

#[test]
fn missing_canonical_memory_requires_rebuild_instead_of_unindexed_cleanup() {
    let c = fixture();
    memory(&c, "a", "coefficient", "current evidence", "[]");
    rebuild_indexes(&c).unwrap();
    c.execute(
        "INSERT INTO memory_fts(rowid,memory_id,title) VALUES(99,'missing','stale coefficient')",
        [],
    )
    .unwrap();
    assert!(matches!(
        sync_memories(&c, &BTreeSet::from(["missing".into()])),
        Err(rusqlite::Error::QueryReturnedNoRows)
    ));
    rebuild_indexes(&c).unwrap();
    assert_eq!(
        c.query_row(
            "SELECT group_concat(memory_id) FROM memory_fts",
            [],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "a"
    );
}

#[test]
fn selected_sync_uses_rebuild_normalization_and_removes_retired_documents() {
    let c = fixture();
    let raw_cues = r#"["  ZOË\n","zoë","cafe\u0301","Café"]"#;
    memory(&c, "a", "Original CASE", "coefficient evidence", raw_cues);
    memory(&c, "b", "coefficient", "old evidence", "[]");
    for topic in ["  BETA ", "beta", "Cafe\u{301}", "CAFÉ"] {
        c.execute("INSERT INTO memory_topics VALUES('a',?1)", [topic])
            .unwrap();
    }
    rebuild_indexes(&c).unwrap();
    c.execute(
        "UPDATE learner_memories SET lifecycle='retired' WHERE memory_id='b'",
        [],
    )
    .unwrap();
    sync_memories(&c, &BTreeSet::from(["a".into(), "b".into()])).unwrap();
    let projection = || {
        c.query_row("SELECT json_group_array(json_array(rowid,memory_id,title,scope,topics,summary,state_text,review_note,retrieval_cues)) FROM (SELECT rowid,* FROM memory_fts ORDER BY rowid)", [], |row| row.get::<_, String>(0)).unwrap()
    };
    let before = projection();
    assert_eq!(
        c.query_row(
            "SELECT title,topics,retrieval_cues,review_note FROM memory_fts",
            [],
            |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?
            ))
        )
        .unwrap(),
        (
            "Original CASE".into(),
            "beta\ncafé".into(),
            "café\nzoë".into(),
            None
        )
    );
    rebuild_indexes(&c).unwrap();
    assert_eq!(before, projection());
    assert_eq!(
        c.query_row("SELECT count(*) FROM memory_fts", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        c.query_row(
            "SELECT retrieval_cues_json FROM learner_memories WHERE memory_id='a'",
            [],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        raw_cues
    );
}
