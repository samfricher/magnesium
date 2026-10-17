//! The writer and a clean rebuild must expose the same active search corpus.
use super::{HostContext, LearnerStore, SCHEMA, UpdateResult, read, search, store};
use rusqlite::{Connection, params};
use serde_json::{Value, json};

struct Fixture {
    store: LearnerStore,
    host: HostContext,
    root: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(store::new_id("search-projection").unwrap());
        std::fs::create_dir(&root).unwrap();
        let store =
            LearnerStore::initialize(&root.join("state.sqlite"), "search-projection").unwrap();
        let host = HostContext {
            host_kind: "test".into(),
            host_thread_id: "search-projection".into(),
        };
        store
            .update(
                &host,
                json!({"schema":SCHEMA,"action":"update","request_id":"start",
            "session":{"operation":"start","client_ref":"session","goal":"Algebra"}}),
            )
            .unwrap();
        Self { store, host, root }
    }

    fn update(&self, id: &str, edits: Value) -> UpdateResult {
        self.store
            .update(
                &self.host,
                json!({"schema":SCHEMA,"action":"update",
            "request_id":id,"memory_edits":edits}),
            )
            .unwrap()
    }

    fn observation(&self) -> String {
        let captured = self.store.update(&self.host, json!({"schema":SCHEMA,"action":"update",
            "request_id":"capture","reported_events":[{"external_event_key":"work",
                "speaker":"learner","kind":"message","text":"The coefficient is fixed; x is the unknown."}]})).unwrap();
        let event = &captured.captured_event_ids[0];
        let observed = self.store.update(&self.host, json!({"schema":SCHEMA,"action":"update",
            "request_id":"observe","observations":[{"client_ref":"work","kind":"learner_work",
                "description":"Identifies coefficient roles on one supplied example.",
                "conditions_text":"After a worked example.","assistance_text":"Tutor provided the example.",
                "events":[{"event_id":event,"role":"learner_response"}]}],
            "event_dispositions":[{"event_id":event,"outcome":"processed"}]})).unwrap();
        observed.references["work"].clone()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let conn = std::mem::replace(&mut self.store.conn, Connection::open_in_memory().unwrap());
        drop(conn);
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn retire(id: &str, revision: i64, replacements: Value) -> Value {
    json!({"operation":"retire","memory_id":id,"expected_revision":revision,
        "reason":"Local consolidation of the same authored evidence.",
        "replacement_ids":replacements})
}

fn rows(conn: &Connection, table: &str) -> Vec<Value> {
    let mut stmt = conn
        .prepare(&format!("SELECT rowid,* FROM {table} ORDER BY rowid"))
        .unwrap();
    let columns = stmt.column_count();
    stmt.query_map([], |row| {
        let cells = (0..columns)
            .map(|column| {
                Ok(match row.get_ref(column)? {
                    rusqlite::types::ValueRef::Null => Value::Null,
                    rusqlite::types::ValueRef::Integer(n) => json!(n),
                    rusqlite::types::ValueRef::Real(n) => json!(n),
                    rusqlite::types::ValueRef::Text(text) => {
                        json!(std::str::from_utf8(text).unwrap())
                    }
                    rusqlite::types::ValueRef::Blob(bytes) => json!(bytes),
                })
            })
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(json!(cells))
    })
    .unwrap()
    .collect::<rusqlite::Result<_>>()
    .unwrap()
}

fn ranked(conn: &Connection) -> Vec<(String, f64)> {
    conn.prepare(
        "SELECT m.memory_id,bm25(memory_fts,0,5,2,2,4,1,1,1)
        FROM memory_fts JOIN learner_memories m ON m.memory_no=memory_fts.rowid
        WHERE memory_fts MATCH 'coefficient' AND m.lifecycle='active'
        ORDER BY bm25(memory_fts,0,5,2,2,4,1,1,1),m.memory_id",
    )
    .unwrap()
    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
    .unwrap()
    .collect::<rusqlite::Result<_>>()
    .unwrap()
}

fn returned(conn: &Connection) -> Value {
    let intents = [search::Intent {
        id: "roles".into(),
        purpose: "Coefficient evidence and terminology".into(),
        queries: vec![search::Query {
            any_of: vec!["coefficient".into(), "café".into(), "transfer".into()],
            all_of: vec![],
        }],
    }];
    let result = search::search(conn, &intents, "", &[], None).unwrap();
    json!({"intents":result.intents.into_iter().map(|(id, candidates)| {
        json!({"id":id,"candidates":candidates.into_iter().map(|candidate| {
            json!({"id":candidate.id,"score":candidate.score,"channels":candidate.channels})
        }).collect::<Vec<_>>()})
    }).collect::<Vec<_>>(),"truncated":result.truncated,"warnings":result.warnings})
}

fn canonical(conn: &Connection) -> Value {
    let mut snapshot = serde_json::Map::new();
    for table in [
        "workspace_meta",
        "learner_memories",
        "memory_topics",
        "memory_observations",
        "memory_changes",
        "observations",
        "observation_events",
        "learning_events",
        "operation_receipts",
        "learning_sessions",
        "host_bindings",
        "review_items",
    ] {
        snapshot.insert(table.into(), json!(rows(conn, table)));
    }
    Value::Object(snapshot)
}

fn assert_current_projection_survives_rebuild(conn: &Connection) {
    assert!(search::index_current(conn, false).unwrap());
    assert!(search::index_current(conn, true).unwrap());
    let retired: i64 = conn
        .query_row(
            "SELECT count(*) FROM memory_fts f
        LEFT JOIN learner_memories m ON m.memory_no=f.rowid
        WHERE m.lifecycle IS NULL OR m.lifecycle!='active'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        retired, 0,
        "the FTS corpus itself contains only active memories"
    );
    let before = (
        rows(conn, "memory_fts"),
        rows(conn, "observation_fts"),
        ranked(conn),
        returned(conn),
    );
    let before_canonical = canonical(conn);
    search::reindex(conn).unwrap();
    let after = (
        rows(conn, "memory_fts"),
        rows(conn, "observation_fts"),
        ranked(conn),
        returned(conn),
    );
    assert_eq!(
        before, after,
        "current rows, BM25 ranks, and returned candidates must match a clean rebuild"
    );
    assert_eq!(
        before_canonical,
        canonical(conn),
        "reindex must preserve canonical learner state"
    );
}

fn record(title: &str, observation: &str) -> Value {
    json!({"title":title,"scope":"Algebra","summary":"Coefficient roles are understood on one example.",
        "state_text":"No general transfer has been established.","topics":[" Zeta ","Café","cafe\u{301}","ALPHA"],
        "retrieval_cues":["fixed letter","Café","cafe\u{301}"," FIXED   LETTER "],
        "current_basis":[{"observation_id":observation}]})
}

fn create(client_ref: &str, observation: &str) -> Value {
    json!({"operation":"create","client_ref":client_ref,"record":record(client_ref, observation),
        "reason":"Preserve the authored local evidence.","change_basis":[{"observation_id":observation}]})
}

#[test]
fn whole_record_formulation_returns_current_writer_description_and_lexical_attribution() {
    let f = Fixture::new();
    let observation = f.observation();
    let mut description = record("Quadratic factorization", &observation);
    description["summary"] = json!("Can expand; reversing expansion needs support.");
    description["state_text"] = json!("Expansion is demonstrated; its reverse needs support.");
    description["topics"] = json!([]);
    description["retrieval_cues"] = json!(["undo FOIL"]);
    let created = f.update(
        "create-quadratic",
        json!([{"operation":"create","client_ref":"quadratic","record":description,
            "reason":"Keep description and retrieval vocabulary separate.",
            "change_basis":[{"observation_id":observation}]}]),
    );
    let memory = &created.references["quadratic"];
    let query = search::Query {
        any_of: vec!["undo FOIL".into()],
        all_of: vec!["quadratic".into()],
    };
    let intent = search::Intent {
        id: "reverse-expansion".into(),
        purpose: "Find the current factorization description".into(),
        queries: vec![query.clone(), query],
    };
    let before = canonical(&f.store.conn);
    let packet = read::test_support::execute_with_tracing(
        &f.store.conn,
        &f.host.host_kind,
        &f.host.host_thread_id,
        json!({"schema":SCHEMA,"action":"search","mode":"focused","intents":[intent]}),
        false,
    )
    .unwrap();
    assert_eq!(packet["selection_mode"], "focused");
    assert_eq!(packet["memories"].as_array().unwrap().len(), 1);
    let card = &packet["memories"][0];
    assert_eq!(card["memory_id"], memory.as_str());
    assert_eq!(card["matched_intents"], json!(["reverse-expansion"]));
    assert_eq!(card["match_channels"], json!(["lexical"]));
    assert_eq!(card["summary"], description["summary"]);
    assert_eq!(card["state_text"], description["state_text"]);
    assert_eq!(
        before,
        canonical(&f.store.conn),
        "retrieval does not alter learner facts"
    );
    let hits = search::search(&f.store.conn, std::slice::from_ref(&intent), "", &[], None).unwrap();
    assert_eq!(hits.intents[0].1[0].score, 1.0 / 61.0);

    description["retrieval_cues"] = json!(["reverse products"]);
    assert_eq!(
        f.update(
            "revise-quadratic",
            json!([{"operation":"revise","memory_id":memory,"expected_revision":1,
                "change_kind":"editorial","record":description,
                "reason":"Replace the retrieval alias without changing the description.",
                "change_basis":[{"observation_id":observation}]}]),
        )
        .index_status,
        "current"
    );
    assert!(
        search::search(&f.store.conn, &[intent], "", &[], None)
            .unwrap()
            .intents[0]
            .1
            .is_empty()
    );
    let snapshot: String = f
        .store
        .conn
        .query_row(
            "SELECT snapshot_json FROM memory_changes WHERE memory_id=?1 AND revision=1",
            [memory],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&snapshot).unwrap()["retrieval_cues"],
        json!(["undo foil"])
    );
    let current_intent = search::Intent {
        id: "reverse-expansion".into(),
        purpose: "Find only the current description".into(),
        queries: vec![search::Query {
            any_of: vec!["reverse products".into()],
            all_of: vec!["quadratic".into()],
        }],
    };
    assert_eq!(
        search::search(
            &f.store.conn,
            std::slice::from_ref(&current_intent),
            "",
            &[],
            None
        )
        .unwrap()
        .intents[0]
            .1[0]
            .id,
        *memory
    );
    assert_current_projection_survives_rebuild(&f.store.conn);
    f.update("retire-quadratic", json!([retire(memory, 2, json!([]))]));
    assert!(
        search::search(&f.store.conn, &[current_intent], "", &[], None)
            .unwrap()
            .intents[0]
            .1
            .is_empty()
    );
    assert_current_projection_survives_rebuild(&f.store.conn);
}

#[test]
fn whole_record_formulations_preserve_per_intent_packet_opportunities() {
    let f = Fixture::new();
    let observation = f.observation();
    for batch in 0..3 {
        let edits = (batch * 6..batch * 6 + 6)
            .map(|index| {
                let mut description = record(&format!("Coefficient {index}"), &observation);
                description["summary"] = json!("Recognizes fixed labels in an example.");
                description["topics"] = json!([]);
                description["retrieval_cues"] = json!([]);
                json!({"operation":"create","client_ref":format!("coefficient-{index}"),
                "record":description,"reason":"Keep each local example description.",
                "change_basis":[{"observation_id":observation}]})
            })
            .collect::<Vec<_>>();
        f.update(&format!("create-coefficients-{batch}"), json!(edits));
    }
    let mut description = record("Geometry example", &observation);
    description["summary"] = json!("Works from a diagram in the supplied example.");
    description["state_text"] = json!("The diagram supports the explanation.");
    description["topics"] = json!([]);
    description["retrieval_cues"] = json!(["visual proof"]);
    let created = f.update(
        "create-geometry",
        json!([{"operation":"create","client_ref":"geometry","record":description,
            "reason":"Retain a different useful strength.",
            "change_basis":[{"observation_id":observation}]}]),
    );
    let packet = read::test_support::execute_with_tracing(
        &f.store.conn,
        &f.host.host_kind,
        &f.host.host_thread_id,
        json!({"schema":SCHEMA,"action":"search","mode":"focused","max_bytes":65536,
            "intents":[
                {"id":"coefficient-roles","purpose":"One possible explanation","queries":[
                    {"any_of":["coefficient"]},{"any_of":["fixed labels"]}]},
                {"id":"diagram-strength","purpose":"Another relevant strength","queries":[
                    {"any_of":["visual proof"],"all_of":["geometry"]}]}]}),
        false,
    )
    .unwrap();
    assert_eq!(packet["memories"].as_array().unwrap().len(), 16);
    assert_eq!(
        packet["memories"][1]["memory_id"],
        created.references["geometry"]
    );
    assert_eq!(
        packet["memories"][1]["matched_intents"],
        json!(["diagram-strength"])
    );
    assert_eq!(packet["memories"][1]["match_channels"], json!(["lexical"]));
    assert_eq!(packet["coverage"]["omitted_card_count"], 3);
}

#[test]
fn create_revise_flag_and_retire_share_exact_projection_and_search_results() {
    let f = Fixture::new();
    let observation = f.observation();
    let created = f.update(
        "create",
        json!([
            create("coefficient", &observation),
            create("peer", &observation)
        ]),
    );
    assert_eq!(created.index_status, "current");
    let memory = &created.references["coefficient"];
    let conn = &f.store.conn;
    assert_current_projection_survives_rebuild(conn);
    let (topics, cues, note): (String, String, Option<String>) = conn
        .query_row(
            "SELECT topics,retrieval_cues,review_note FROM memory_fts WHERE memory_id=?1",
            [memory],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(topics, "alpha\ncafé\nzeta");
    assert_eq!(cues, "café\nfixed letter");
    assert_eq!(note, None);

    let mut revised = record("Revised coefficient scope", &observation);
    revised["summary"] = json!("The coefficient is fixed; transfer still untested.");
    revised.as_object_mut().unwrap().remove("topics");
    revised.as_object_mut().unwrap().remove("retrieval_cues");
    assert_eq!(f.update("revise", json!([{"operation":"revise","memory_id":memory,
        "expected_revision":1,"change_kind":"editorial","record":revised,
        "reason":"Clarify the existing description.","change_basis":[{"observation_id":observation}]}])).index_status, "current");
    assert_current_projection_survives_rebuild(conn);

    assert_eq!(f.update("flag", json!([{"operation":"flag","memory_id":memory,
        "expected_revision":2,"review_note":"Check coefficient transfer without inferring inability.",
        "reason":"Keep the interpretation limited.","change_basis":[{"observation_id":observation}]}])).index_status, "current");
    assert_current_projection_survives_rebuild(conn);
    assert_eq!(
        conn.query_row(
            "SELECT review_note FROM memory_fts WHERE memory_id=?1",
            [memory],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "Check coefficient transfer without inferring inability."
    );

    assert_eq!(
        f.update(
            "retire",
            json!([retire(memory, 3, json!([created.references["peer"]]))])
        )
        .index_status,
        "current"
    );
    assert_current_projection_survives_rebuild(conn);
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM memory_fts WHERE memory_id=?1",
            [memory],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}

#[test]
fn merge_and_split_local_consolidations_preserve_projection() {
    let f = Fixture::new();
    let observation = f.observation();
    let created = f.update(
        "create",
        json!([
            create("coefficient", &observation),
            create("scope", &observation)
        ]),
    );
    let a = &created.references["coefficient"];
    let b = &created.references["scope"];
    assert_current_projection_survives_rebuild(&f.store.conn);
    let merged = f.update(
        "merge",
        json!([
            retire(a, 1, json!(["combined"])),
            create("combined", &observation),
            retire(b, 1, json!(["combined"]))
        ]),
    );
    assert_eq!(merged.index_status, "current");
    assert_current_projection_survives_rebuild(&f.store.conn);
    let combined = &merged.references["combined"];
    let split = f.update(
        "split",
        json!([
            create("coefficient", &observation),
            retire(combined, 1, json!(["coefficient", "transfer"])),
            create("transfer", &observation)
        ]),
    );
    assert_eq!(split.index_status, "current");
    assert_current_projection_survives_rebuild(&f.store.conn);
    assert_eq!(rows(&f.store.conn, "memory_fts").len(), 2);
}

#[test]
fn index_only_failure_rolls_back_projection_but_commits_canonical_state_and_dirty_receipt() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    let f = Fixture::new();
    let observation = f.observation();
    let created = f.update("create", json!([create("coefficient", &observation)]));
    let memory = &created.references["coefficient"];
    let conn = &f.store.conn;
    let before_index = rows(conn, "memory_fts");
    conn.authorizer(Some(|context: AuthContext<'_>| match context.action {
        AuthAction::Insert {
            table_name: "memory_fts",
        } => Authorization::Deny,
        _ => Authorization::Allow,
    }))
    .unwrap();
    let result = f.update(
        "flag-failed-index",
        json!([{"operation":"flag","memory_id":memory,
        "expected_revision":1,"review_note":"Reconsider coefficient transfer.",
        "reason":"An index error must not discard the learner description.",
        "change_basis":[{"observation_id":observation}]}]),
    );
    conn.authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .unwrap();
    assert_eq!(result.update_status, "committed");
    assert_eq!(result.index_status, "dirty");
    assert_eq!(
        rows(conn, "memory_fts"),
        before_index,
        "savepoint restores the prior document after insert failure"
    );
    assert_eq!(
        conn.query_row(
            "SELECT revision,review_note FROM learner_memories WHERE memory_id=?1",
            [memory],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        )
        .unwrap(),
        (2, "Reconsider coefficient transfer.".into())
    );
    let receipt: String = conn
        .query_row(
            "SELECT result_json FROM operation_receipts WHERE request_id='flag-failed-index'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&receipt).unwrap()["index_status"],
        "dirty"
    );
    assert!(!search::index_current(conn, false).unwrap());
    assert!(
        returned(conn)["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning.as_str().unwrap().starts_with("index_unavailable:"))
    );
    search::reindex(conn).unwrap();
    assert_current_projection_survives_rebuild(conn);
}

#[test]
fn canonical_write_failure_remains_an_error_and_rolls_back_the_whole_batch() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    let f = Fixture::new();
    let observation = f.observation();
    let created = f.update("create", json!([create("coefficient", &observation)]));
    let conn = &f.store.conn;
    search::reindex(conn).unwrap();
    let before = canonical(conn);
    let before_index = rows(conn, "memory_fts");
    conn.authorizer(Some(|context: AuthContext<'_>| match context.action {
        AuthAction::Insert {
            table_name: "memory_changes",
        } => Authorization::Deny,
        _ => Authorization::Allow,
    }))
    .unwrap();
    let error = f
        .store
        .update(
            &f.host,
            json!({"schema":SCHEMA,"action":"update","request_id":"canonical-failure",
        "memory_edits":[{"operation":"flag","memory_id":created.references["coefficient"],
            "expected_revision":1,"review_note":"Reconsider coefficient transfer.",
            "reason":"A canonical storage error is not an indexing warning.",
            "change_basis":[{"observation_id":observation}]}]}),
        )
        .unwrap_err();
    conn.authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .unwrap();
    assert_eq!(error.code, "storage_error");
    assert_eq!(canonical(conn), before);
    assert_eq!(rows(conn, "memory_fts"), before_index);
    assert_current_projection_survives_rebuild(conn);
}

#[test]
fn retirement_preserves_active_corpus_and_ranking_across_rebuild() {
    let f = Fixture::new();
    let conn = &f.store.conn;
    // A short content match and a longer title match exchange rank if retired
    // filler documents remain in the corpus and inflate its average length.
    for (id, title, summary, state) in [
        (
            "A".to_owned(),
            "a".to_owned(),
            "coefficient".to_owned(),
            String::new(),
        ),
        (
            "B".to_owned(),
            "coefficient coefficient coefficient".to_owned(),
            String::new(),
            "filler ".repeat(100),
        ),
    ]
    .into_iter()
    .chain((0..32).map(|n| {
        (
            format!("retire-{n}"),
            String::new(),
            String::new(),
            "filler ".repeat(200),
        )
    })) {
        conn.execute("INSERT INTO learner_memories(memory_id,title,scope,summary,state_text,current_basis_json,lifecycle,revision,created_at_ms,updated_at_ms)
            VALUES(?1,?2,'',?3,?4,'[]','active',1,0,0)", params![id,title,summary,state]).unwrap();
    }
    search::rebuild_indexes(conn).unwrap();
    for batch in 0..4 {
        let edits = (batch * 8..batch * 8 + 8)
            .map(|n| retire(&format!("retire-{n}"), 1, json!([])))
            .collect::<Vec<_>>();
        assert_eq!(
            f.update(&format!("retire-batch-{batch}"), json!(edits))
                .index_status,
            "current"
        );
    }
    let incremental_rows = rows(conn, "memory_fts");
    let incremental_ranks = ranked(conn);
    let incremental_results = returned(conn);
    search::rebuild_indexes(conn).unwrap();
    assert_eq!(
        incremental_ranks,
        ranked(conn),
        "rebuilding current derived state must not change active ranks"
    );
    assert_eq!(incremental_results, returned(conn));
    assert_eq!(incremental_rows, rows(conn, "memory_fts"));
    assert_eq!(
        incremental_rows.len(),
        2,
        "retired text must be absent before ranking"
    );
}
