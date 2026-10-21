use super::*;
use serde_json::json;
use std::collections::BTreeSet;
fn fixture() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(crate::learner::store::SCHEMA_SQL)
        .unwrap();
    conn.execute("INSERT INTO workspace_meta(singleton,workspace_id,storage_family,schema_version,created_at_ms) VALUES(1,'read-tests','learner-model/v2',2,0)",[]).unwrap();
    conn
}
fn run(conn: &Connection, mut request: Value) -> Result<Value, String> {
    request["schema"] = json!(crate::learner::SCHEMA);
    crate::learner::read::test_support::execute(conn, "h", "fresh", request)
}
fn event(conn: &Connection, id: &str, host: &str, thread: &str, session: Option<&str>) {
    conn.execute("INSERT INTO learning_events(event_id,session_id,host_kind,host_thread_id,external_event_key,speaker,kind,origin,text,metadata_json,captured_at_ms,processing) VALUES(?1,?2,?3,?4,?1,'learner','message','host_observed','private raw text unknown to this tutor','{}',41,'pending')",params![id,session,host,thread]).unwrap();
}
#[test]
fn pending_work_is_discoverable_metadata_only_explicitly_filtered_and_read_only() {
    let conn = fixture();
    conn.execute(
        "INSERT INTO learning_sessions VALUES('s','goal','active','{}',1,0,0)",
        [],
    )
    .unwrap();
    event(&conn, "old-unbound", "host-a", "thread-a", None);
    event(&conn, "other-unbound", "host-b", "thread-b", None);
    event(&conn, "bound", "host-a", "thread-a", Some("s"));
    let notice = run(&conn, json!({"action":"prepare"})).unwrap();
    assert_eq!(
        notice["pending_evidence"]["pending_work_ref"]["arguments"]["action"],
        "pending_work"
    );
    assert_eq!(
        notice["pending_evidence"]["pending_work_ref"]["arguments"]["unbound_only"],
        true
    );
    let page = strict_next_call(&conn, &notice["pending_evidence"]["pending_work_ref"]).unwrap();
    assert_eq!(page["page_consistency"], "live");
    assert!(page.get("memory_generation").is_none());
    assert_eq!(page["pending_work"].as_array().unwrap().len(), 2);
    assert!(!page.to_string().contains("private raw text"));
    let row = &page["pending_work"][0];
    assert_eq!(row["event_id"], "old-unbound");
    assert_eq!(row["host_thread_id"], "thread-a");
    assert_eq!(row["captured_at_ms"], 41);
    assert_eq!(
        row["capture_time_meaning"],
        "capture_time_not_occurrence_time"
    );
    let read = run(&conn, json!({"action":"read","refs":[row["read_ref"]]})).unwrap();
    assert_eq!(
        read["records"][0]["text"],
        "private raw text unknown to this tutor"
    );
    let filtered=run(&conn,json!({"action":"pending_work","host_kind":"host-a","host_thread_id":"thread-a","unbound_only":true})).unwrap();
    assert_eq!(filtered["pending_work"].as_array().unwrap().len(), 1);
    let bound = run(&conn, json!({"action":"pending_work","session_id":"s"})).unwrap();
    assert_eq!(bound["pending_work"][0]["event_id"], "bound");
    assert!(
        run(
            &conn,
            json!({"action":"pending_work","session_id":"s","unbound_only":true})
        )
        .is_err()
    );
    let other = fixture();
    assert!(
        run(&other, json!({"action":"pending_work"})).unwrap()["pending_work"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(conn.query_row("SELECT count(*) FROM learning_events WHERE processing='pending' AND session_id IS NULL",[],|r|r.get::<_,i64>(0)).unwrap(),2);
    assert_eq!(
        conn.query_row("SELECT count(*) FROM host_bindings", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn every_page_reserves_cursor_and_keeps_first_rejected_item_reachable() {
    let conn = fixture();
    for n in 0..4 {
        let id = format!("m{n}");
        conn.execute("INSERT INTO learner_memories(memory_id,title,scope,summary,state_text,current_basis_json,lifecycle,revision,created_at_ms,updated_at_ms) VALUES(?1,'title','scope',?2,?3,'[]','active',1,0,0)",params![id,"whole summary ".repeat(30),"optional body ".repeat(3000)]).unwrap();
        conn.execute(
            "INSERT INTO memory_topics VALUES(?1,?2)",
            params![id, format!("topic{n}")],
        )
        .unwrap();
        conn.execute("INSERT INTO observations(observation_id,kind,description,conditions_text,assistance_text,source_refs_json,created_at_ms) VALUES(?1,'question',?2,'limited conditions','help unknown','[]',0)",params![format!("o{n}"),"concrete observed work ".repeat(20)]).unwrap();
        conn.execute("INSERT INTO memory_observations VALUES(?1,'o0')", [&id])
            .unwrap();
        conn.execute("INSERT INTO review_items(review_id,memory_id,memory_revision,purpose,reason,status,revision,updated_at_ms) VALUES(?1,?2,1,?3,'authored reason','suggested',1,0)",params![format!("r{n}"),id,"authored review purpose ".repeat(20)]).unwrap();
        let mut snapshot = crate::learner::records::memory_snapshot(&conn, "m0").unwrap();
        snapshot.revision = n + 1;
        snapshot.state_text = "historical state ".repeat(30);
        conn.execute("INSERT INTO memory_changes(memory_id,revision,change_kind,snapshot_json,reason,change_basis_json,created_at_ms) VALUES('m0',?1,'new_observation',?2,'historical reason','[]',0)",params![n+1,serde_json::to_string(&snapshot).unwrap()]).unwrap();
        conn.execute(
            "INSERT INTO learning_sessions VALUES(?1,?2,'active','{}',1,0,0)",
            params![format!("s{n}"), "bounded session goal ".repeat(20)],
        )
        .unwrap();
        event(&conn, &format!("e{n}"), "h", "fresh", None);
        conn.execute(
            "INSERT INTO observation_events VALUES('o0',?1,'learner_response','[]')",
            [format!("e{n}")],
        )
        .unwrap();
    }
    let actions = [
        (json!({"action":"browse"}), "memories", "memory_id"),
        (json!({"action":"topics"}), "topics", "topic"),
        (
            json!({"action":"history","memory_id":"m0"}),
            "history",
            "revision",
        ),
        (json!({"action":"reviews"}), "reviews", "review_id"),
        (
            json!({"action":"observations"}),
            "observations",
            "observation_id",
        ),
        (
            json!({"action":"observations","kind":"memory_refs","observation_id":"o0"}),
            "support",
            "memory_id",
        ),
        (
            json!({"action":"observations","kind":"event_refs","observation_id":"o0"}),
            "support",
            "event_id",
        ),
        (
            json!({"action":"observations","kind":"event_memory_refs","event_id":"e0"}),
            "support",
            "memory_id",
        ),
        (
            json!({"action":"observations","kind":"event","literal_terms":["private"]}),
            "observations",
            "event_id",
        ),
        (
            json!({"action":"session_choices"}),
            "session_choices",
            "session_id",
        ),
        (
            json!({"action":"pending_events"}),
            "pending_event_refs",
            "id",
        ),
        (json!({"action":"pending_work"}), "pending_work", "event_id"),
    ];
    for (mut request, field, id) in actions {
        request["limit"] = json!(1);
        request["max_bytes"] = json!(65536);
        let first = run(&conn, request.clone()).unwrap();
        let budget = pages::size(&first);
        request["limit"] = json!(2);
        request["max_bytes"] = json!(budget);
        let mut next_call = Value::Null;
        let mut seen = BTreeSet::new();
        loop {
            let page = if next_call.is_null() {
                run(&conn, request.clone())
            } else {
                strict_next_call(&conn, &next_call)
            }
            .unwrap_or_else(|e| panic!("{field} budget {budget}: {e}"));
            assert!(pages::size(&page) <= budget, "{field}");
            for row in page[field].as_array().unwrap() {
                assert!(seen.insert(row[id].to_string()), "duplicate {field}: {row}");
            }
            next_call = page["next_call"].clone();
            assert_eq!(
                next_call.is_null(),
                page["next_cursor"].is_null(),
                "{field} must emit an executable call for every cursor"
            );
            if next_call.is_null() {
                break;
            }
            assert!(
                seen.len() < 4,
                "continuation advanced past rejected item in {field}"
            );
        }
        assert_eq!(seen.len(), 4, "{field} skipped first rejected item");
    }
}
#[test]
fn action_contract_rejects_fields_from_other_actions() {
    let conn = fixture();
    for request in [
        json!({"action":"history","memory_id":"missing","intents":[]}),
        json!({"action":"browse","refs":[]}),
        json!({"action":"read","refs":[{"kind":"observation","id":"o","start_byte":0}]}),
        json!({"action":"status","mode":"auto"}),
    ] {
        assert!(
            run(&conn, request)
                .unwrap_err()
                .starts_with("invalid_variant")
        );
    }
    assert!(
        run(&conn, json!({"action":"prepare","mode":"unsupported"}))
            .unwrap_err()
            .contains("unsupported")
    );
}
#[test]
fn filtered_event_search_and_review_cursors_track_relational_generations() {
    let conn = fixture();
    conn.execute("INSERT INTO learner_memories(memory_id,title,scope,summary,state_text,current_basis_json,lifecycle,revision,created_at_ms,updated_at_ms) VALUES('m','title','scope','summary','state','[]','active',1,0,0)",[]).unwrap();
    for n in 0..2 {
        let observation = format!("o{n}");
        event(&conn, &format!("e{n}"), "h", "fresh", None);
        conn.execute("INSERT INTO observations(observation_id,kind,description,conditions_text,assistance_text,source_refs_json,created_at_ms) VALUES(?1,'question','work','conditions','help','[]',0)",[&observation]).unwrap();
        conn.execute(
            "INSERT INTO observation_events VALUES(?1,?2,'learner_response','[]')",
            params![observation, format!("e{n}")],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO memory_observations VALUES('m',?1)",
            [&observation],
        )
        .unwrap();
        conn.execute("INSERT INTO review_items(review_id,memory_id,memory_revision,purpose,reason,status,revision,updated_at_ms) VALUES(?1,'m',1,'review','reason','suggested',1,0)",[format!("r{n}")]).unwrap();
    }
    let events=run(&conn,json!({"action":"observations","kind":"event","memory_id":"m","literal_terms":["private"],"limit":1})).unwrap();
    let review = run(&conn, json!({"action":"reviews","limit":1})).unwrap();
    conn.execute(
        "UPDATE workspace_meta SET memory_generation=memory_generation+1",
        [],
    )
    .unwrap();
    assert!(run(&conn,json!({"action":"observations","kind":"event","memory_id":"m","literal_terms":["private"],"cursor":events["next_cursor"]})).unwrap_err().starts_with("cursor_stale"));
    let review = run(&conn, json!({"action":"reviews","limit":1})).unwrap_or(review);
    conn.execute(
        "UPDATE review_items SET status='dismissed',revision=2 WHERE review_id='r1'",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE workspace_meta SET review_generation=review_generation+1",
        [],
    )
    .unwrap();
    assert!(
        run(
            &conn,
            json!({"action":"reviews","cursor":review["next_cursor"]})
        )
        .unwrap_err()
        .starts_with("cursor_stale")
    );
    assert!(review.get("review_fingerprint").is_none());
}
#[test]
fn trace_off_after_snapshot_discards_only_receipt_and_changes_no_generation() {
    let conn = fixture();
    conn.execute("INSERT INTO host_bindings VALUES('h','fresh',NULL,1,1)", [])
        .unwrap();
    let prepared = prepare_snapshot(
        &conn,
        "h",
        "fresh",
        json!({"schema":crate::learner::SCHEMA,"action":"browse"}),
    )
    .unwrap();
    conn.execute("UPDATE host_bindings SET recording_enabled=0", [])
        .unwrap();
    let packet = finish_trace(&conn, prepared, true).unwrap();
    assert_eq!(packet["trace_status"], "disabled");
    assert_eq!(
        conn.query_row("SELECT count(*) FROM context_receipts", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    let (m, o, r): (i64, i64, i64) = conn
        .query_row(
            "SELECT memory_generation,observation_generation,review_generation FROM workspace_meta",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((m, o, r), (0, 0, 0));
}

#[test]
fn raw_event_session_pages_reject_older_unbound_attachment() {
    let conn = fixture();
    conn.execute(
        "INSERT INTO learning_sessions VALUES('s','goal','active','{}',1,0,0)",
        [],
    )
    .unwrap();
    event(&conn, "older-unbound", "old-host", "old-thread", None);
    event(&conn, "current-one", "h", "fresh", Some("s"));
    event(&conn, "current-two", "h", "fresh", Some("s"));
    let first=run(&conn,json!({"action":"observations","kind":"event","session_id":"s","literal_terms":["private"],"limit":1})).unwrap();
    conn.execute(
        "UPDATE learning_events SET session_id='s' WHERE event_id='older-unbound'",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE workspace_meta SET event_generation=event_generation+1",
        [],
    )
    .unwrap();
    assert!(run(&conn,json!({"action":"observations","kind":"event","session_id":"s","literal_terms":["private"],"cursor":first["next_cursor"]})).unwrap_err().starts_with("cursor_stale"));
    let fresh=run(&conn,json!({"action":"observations","kind":"event","session_id":"s","literal_terms":["private"]})).unwrap();
    assert_eq!(fresh["observations"][0]["event_id"], "older-unbound");
}
#[test]
fn historical_snapshot_preserves_complete_review_resolution() {
    let conn = fixture();
    conn.execute("INSERT INTO learner_memories(memory_id,title,scope,summary,state_text,current_basis_json,lifecycle,revision,created_at_ms,updated_at_ms) VALUES('m','title','scope','No longer disputed','state','[]','active',2,0,0)",[]).unwrap();
    let resolution =
        "Learner clarified the apparent contradiction; this is not a persistent difficulty. "
            .repeat(8);
    let snapshot = json!({"memory_id":"m","title":"title","scope":"scope","summary":"No longer disputed","state_text":"full state","retrieval_cues":[],"review_note":null,"review_resolution":resolution,"current_basis":[],"replacement_ids":[],"lifecycle":"active","revision":2,"last_observed_at_ms":null,"created_at_ms":0,"updated_at_ms":1,"topics":[]});
    conn.execute("INSERT INTO memory_changes(memory_id,revision,change_kind,snapshot_json,reason,change_basis_json,created_at_ms) VALUES('m',2,'new_observation',?1,'review flag resolved','[]',0)",[snapshot.to_string()]).unwrap();
    let history = run(&conn, json!({"action":"history","memory_id":"m"})).unwrap();
    assert_eq!(history["history"][0]["snapshot"], snapshot);
    assert_eq!(
        history["history"][0]["snapshot"]["review_resolution"],
        resolution
    );
}

#[test]
fn read_action_payload_decoder_reports_nested_struct_field_paths() {
    let conn = fixture();
    let error=run(&conn,json!({"action":"prepare","intents":[{"id":"i","purpose":"matching","queries":[{"all_of":[9]}]}]})).unwrap_err();
    assert!(error.contains("intents[0].queries[0].all_of[0]"), "{error}");
    let error = run(
        &conn,
        json!({"action":"prepare","known_revisions":[{"memory_id":"m","revision":"one"}]}),
    )
    .unwrap_err();
    assert!(error.contains("known_revisions[0].revision"), "{error}");
}

fn strict_next_call(conn: &Connection, call: &Value) -> Result<Value, String> {
    let tool = match call["tool"].as_str() {
        Some("memory") => Tool::Memory,
        Some("context") => Tool::Context,
        _ => return Err(format!("missing executable next call: {call}")),
    };
    let prepared = prepare_snapshot_for_tool(conn, "h", "fresh", call["arguments"].clone(), tool)?;
    finish_trace(conn, prepared, false)
}
fn insert_read_memory(conn: &Connection, id: &str) {
    conn.execute("INSERT INTO learner_memories(memory_id,title,scope,summary,state_text,current_basis_json,lifecycle,revision,created_at_ms,updated_at_ms) VALUES(?1,'coefficient','algebra','whole summary','whole state','[]','active',1,0,0)", [id]).unwrap();
}
fn insert_anchor_memory(conn: &Connection, id: &str, title: &str, anchor: &str) {
    conn.execute("INSERT INTO learner_memories(memory_id,title,scope,summary,state_text,current_basis_json,lifecycle,revision,created_at_ms,updated_at_ms) VALUES(?1,?2,'calculus','summary',?3,'[]','active',1,0,0)", params![id, title, format!("anchor: {anchor}")]).unwrap();
}
#[test]
fn anchor_lookup_scoped_to_the_topic_finds_an_anchor_that_an_unscoped_one_misses() {
    let conn = fixture();
    for n in 0..30 {
        insert_anchor_memory(
            &conn,
            &format!("a-{n:02}"),
            &format!("Unrelated skill {n}"),
            &format!("solves exercise {n} (Week {n} tutorial)"),
        );
    }
    insert_anchor_memory(
        &conn,
        "z-chain-rule",
        "Chain rule",
        "applies the chain rule to single-variable functions (Diagnostic, Q3)",
    );
    search::rebuild_indexes(&conn).unwrap();
    let ids = |packet: &Value| -> Vec<String> {
        packet["memories"]
            .as_array()
            .unwrap()
            .iter()
            .map(|card| card["memory_id"].as_str().unwrap().to_owned())
            .collect()
    };

    let unscoped = run(
        &conn,
        json!({"action":"search","literal_terms":["anchor:"]}),
    )
    .unwrap();
    assert_eq!(unscoped["coverage"]["candidate_lists_truncated"], true);
    assert!(
        !ids(&unscoped).contains(&"z-chain-rule".to_owned()),
        "30 other anchors fill the 24 literal results: {unscoped}"
    );

    let scoped = run(
        &conn,
        json!({"action":"search","intents":[{"id":"anchors","purpose":"anchors for the topic","queries":[{"any_of":["chain rule","derivative"],"all_of":["anchor"]}]}]}),
    )
    .unwrap();
    assert_eq!(ids(&scoped), ["z-chain-rule"], "{scoped}");
    assert_eq!(scoped["coverage"]["candidate_lists_truncated"], false);
    assert!(
        scoped["memories"][0]["state_text"]
            .as_str()
            .unwrap()
            .starts_with("anchor: applies the chain rule"),
        "{scoped}"
    );
}
#[test]
fn no_hit_fallback_emits_unchanged_executable_browse_calls() {
    let conn = fixture();
    for n in 0..24 {
        insert_read_memory(&conn, &format!("m{n}"));
        conn.execute(
            "INSERT INTO memory_topics VALUES(?1,'algebra')",
            [format!("m{n}")],
        )
        .unwrap();
    }
    conn.execute(
        "UPDATE learner_memories SET summary=?1",
        ["coherent summary ".repeat(40)],
    )
    .unwrap();
    insert_read_memory(&conn, "outside-topic");
    search::rebuild_indexes(&conn).unwrap();
    for (action, mode) in [
        ("search", "focused"),
        ("prepare", "focused"),
        ("prepare", "auto"),
        ("prepare", "overview"),
    ] {
        let mut page = run(&conn, json!({"action":action,"mode":mode,"topic":"algebra","limit":1,"intents":[{"id":"no-hit","purpose":"absent query","queries":[{"all_of":["zznotfoundzz"]}]}]})).unwrap();
        let mut seen = BTreeSet::new();
        loop {
            assert_eq!(page["selection_mode"], "browse");
            for card in page["memories"].as_array().unwrap() {
                assert!(seen.insert(card["memory_id"].as_str().unwrap().to_owned()));
            }
            if page["next_cursor"].is_null() {
                break;
            }
            assert_eq!(page["next_call"]["tool"], "memory");
            assert_eq!(page["next_call"]["arguments"]["action"], "browse");
            assert_eq!(page["next_call"]["arguments"]["topic"], "algebra");
            page = strict_next_call(&conn, &page["next_call"]).unwrap();
        }
        assert_eq!(seen, (0..24).map(|n| format!("m{n}")).collect());
    }
}
#[test]
fn complete_directory_does_not_execute_ranked_fts_queries() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let conn = fixture();
    insert_read_memory(&conn, "m");
    search::rebuild_indexes(&conn).unwrap();
    let reads = Arc::new(AtomicUsize::new(0));
    let counter = reads.clone();
    conn.authorizer(Some(move |context: AuthContext<'_>| {
        if matches!(
            context.action,
            AuthAction::Read {
                table_name: "memory_fts",
                ..
            }
        ) {
            counter.fetch_add(1, Ordering::Relaxed);
        }
        Authorization::Allow
    }))
    .unwrap();
    prepare::RANKED_CALLS.with(|calls| calls.set(0));
    let page = run(&conn, json!({"action":"prepare","mode":"auto","intents":[{"id":"i","purpose":"find coefficient","queries":[{"all_of":["coefficient"]}]}]})).unwrap();
    prepare::RANKED_CALLS.with(|calls| assert_eq!(calls.get(), 0));
    assert_eq!(
        reads.load(Ordering::Relaxed),
        0,
        "complete directory must bypass ranked retrieval"
    );
    assert_eq!(page["selection_mode"], "complete_directory");
    assert_eq!(page["coverage"]["ranked_retrieval_used"], false);
    assert!(page["coverage"]["query_execution_complete"].is_null());
    assert!(
        page["memories"][0]["matched_intents"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
#[test]
fn complete_directory_survives_isolated_ranked_index_query_failure() {
    let conn = fixture();
    insert_read_memory(&conn, "m");
    conn.execute_batch("DROP TABLE memory_fts; CREATE TABLE memory_fts(unexpected TEXT);")
        .unwrap();
    assert!(search::index_current(&conn, false).unwrap());
    let page = run(&conn, json!({"action":"prepare","mode":"auto","intents":[{"id":"i","purpose":"find coefficient","queries":[{"all_of":["coefficient"]}]}]})).unwrap();
    assert_eq!(page["selection_mode"], "complete_directory");
    assert_eq!(page["coverage"]["ranked_retrieval_used"], false);
}

#[test]
fn ranked_fallback_annotates_priorities_and_separates_advice_from_calls() {
    let conn = fixture();
    for n in 0..40 {
        insert_read_memory(&conn, &format!("m{n}"));
    }
    conn.execute(
        "UPDATE learner_memories SET summary=?1",
        ["coefficient roles with a qualified description ".repeat(20)],
    )
    .unwrap();
    search::rebuild_indexes(&conn).unwrap();
    prepare::RANKED_CALLS.with(|calls| calls.set(0));
    let page = run(&conn, json!({"action":"prepare","mode":"auto","memory_ids":["m0"],"intents":[{"id":"i","purpose":"find coefficient","queries":[{"all_of":["coefficient"]}]}]})).unwrap();
    prepare::RANKED_CALLS.with(|calls| assert_eq!(calls.get(), 1));
    assert_eq!(page["selection_mode"], "focused");
    assert_eq!(page["coverage"]["ranked_retrieval_used"], true);
    assert_eq!(page["coverage"]["query_execution_complete"], true);
    assert_eq!(page["coverage"]["catalog_complete"], false);
    assert_eq!(page["coverage"]["candidate_lists_truncated"], true);
    assert!(page["coverage"]["omitted_card_count"].as_u64().unwrap() > 0);
    assert_eq!(page["memories"][0]["memory_id"], "m0");
    assert_eq!(page["memories"][0]["matched_intents"], json!(["i"]));
    assert!(
        page["hints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|hint| hint.as_str().unwrap().contains("Intent i:"))
    );
    for call in page["continuations"].as_array().unwrap() {
        assert!(call["arguments"].get("intent_id").is_none());
        assert!(call["arguments"].get("instruction").is_none());
        strict_next_call(&conn, call).unwrap();
    }
}

#[test]
fn emitted_call_tracks_relevant_write_and_preserves_pending_filters() {
    let conn = fixture();
    for n in 0..3 {
        insert_read_memory(&conn, &format!("m{n}"));
        event(
            &conn,
            &format!("e{n}"),
            "selected-host",
            "selected-thread",
            None,
        );
        event(
            &conn,
            &format!("noise{n}"),
            "other-host",
            "other-thread",
            None,
        );
    }
    let first = run(
        &conn,
        json!({"action":"search","limit":1,"original_query":"absent-vocabulary"}),
    )
    .unwrap();
    strict_next_call(&conn, &first["next_call"]).unwrap();
    conn.execute(
        "UPDATE workspace_meta SET memory_generation=memory_generation+1",
        [],
    )
    .unwrap();
    assert!(
        strict_next_call(&conn, &first["next_call"])
            .unwrap_err()
            .starts_with("cursor_stale")
    );
    let mut page = run(&conn, json!({"action":"pending_work","host_kind":"selected-host","host_thread_id":"selected-thread","unbound_only":true,"limit":1})).unwrap();
    let mut ids = BTreeSet::new();
    loop {
        for record in page["pending_work"].as_array().unwrap() {
            assert!(ids.insert(record["event_id"].as_str().unwrap().to_owned()));
            assert_eq!(record["host_kind"], "selected-host");
        }
        if page["next_call"].is_null() {
            break;
        }
        assert_eq!(page["next_call"]["arguments"]["unbound_only"], true);
        assert_eq!(
            page["next_call"]["arguments"]["host_thread_id"],
            "selected-thread"
        );
        page = strict_next_call(&conn, &page["next_call"]).unwrap();
    }
    assert_eq!(ids, (0..3).map(|n| format!("e{n}")).collect());
}

#[test]
fn support_preview_calls_use_real_requests_and_page_all_eligible_records() {
    let conn = fixture();
    conn.execute("INSERT INTO observations(observation_id,kind,description,conditions_text,assistance_text,source_refs_json,created_at_ms) VALUES('o','question','work','conditions','help','[]',0)",[]).unwrap();
    for n in 0..35 {
        let id = format!("m{n}");
        insert_read_memory(&conn, &id);
        conn.execute("INSERT INTO memory_observations VALUES(?1,'o')", [&id])
            .unwrap();
        event(&conn, &format!("e{n}"), "h", "fresh", None);
        conn.execute(
            "INSERT INTO observation_events VALUES('o',?1,'learner_response','[]')",
            [format!("e{n}")],
        )
        .unwrap();
    }
    let read = run(&conn,json!({"action":"read","max_bytes":65536,"refs":[{"kind":"observation","id":"o"},{"kind":"event","id":"e0"}]})).unwrap();
    for (call, id_key) in [
        (
            &read["records"][0]["current_memory_refs_continuation"],
            "memory_id",
        ),
        (&read["records"][0]["event_refs_continuation"], "event_id"),
        (
            &read["records"][1]["current_memory_refs_continuation"],
            "memory_id",
        ),
    ] {
        let mut page = strict_next_call(&conn, call).unwrap();
        let mut ids = BTreeSet::new();
        loop {
            for record in page["support"].as_array().unwrap() {
                assert!(ids.insert(record[id_key].as_str().unwrap().to_owned()));
            }
            if page["next_call"].is_null() {
                break;
            }
            page = strict_next_call(&conn, &page["next_call"]).unwrap();
        }
        assert_eq!(ids.len(), 35);
    }
}

#[test]
fn directory_short_circuit_preserves_request_validation() {
    let conn = fixture();
    for request in [
        json!({"action":"prepare","mode":"auto","literal_intent":"unknown"}),
        json!({"action":"prepare","mode":"overview","intents":[{"id":"i","purpose":"bounded","queries":[{"all_of":["x".repeat(257)]}]}]}),
        json!({"action":"prepare","mode":"auto","intents":[{"id":"i","purpose":"duplicate"},{"id":"i","purpose":"duplicate"}]}),
    ] {
        assert!(run(&conn, request).is_err());
    }
}

#[test]
fn optional_ranked_cards_skip_large_candidates_and_keep_whole_qualifications() {
    let conn = fixture();
    insert_read_memory(&conn, "large");
    insert_read_memory(&conn, "small");
    let summary = format!("Not established: {}", "a".repeat(1000));
    let review = format!("Only with help: {}", "b".repeat(1000));
    conn.execute("UPDATE learner_memories SET title='oversized',scope=?1,summary=?2,review_note=?3 WHERE memory_id='large'",params!["s".repeat(512),summary,review]).unwrap();
    conn.execute("UPDATE learner_memories SET title='smaller',summary='Not independent yet.',review_note='Only with a worked example.' WHERE memory_id='small'",[]).unwrap();
    search::rebuild_indexes(&conn).unwrap();
    let packet = run(&conn, json!({"action":"search","mode":"focused","max_bytes":3000,"intents":[{"id":"first","purpose":"large","queries":[{"all_of":["oversized"]}]},{"id":"second","purpose":"small","queries":[{"all_of":["smaller"]}]}]})).unwrap();
    assert!(pages::size(&packet) <= 3000);
    assert_eq!(packet["memories"].as_array().unwrap().len(), 1);
    assert_eq!(packet["memories"][0]["memory_id"], "small");
    assert_eq!(packet["memories"][0]["summary"], "Not independent yet.");
    assert_eq!(
        packet["memories"][0]["review_note"],
        "Only with a worked example."
    );
    assert_eq!(packet["coverage"]["omitted_card_count"], 1);
    assert_eq!(
        packet["omitted_memory_refs"],
        json!([{"kind":"current_memory","id":"large","expected_revision":1}])
    );
    let read = run(
        &conn,
        json!({"action":"read","max_bytes":65536,"refs":[{"kind":"current_memory","id":"large"}]}),
    )
    .unwrap();
    assert_eq!(read["records"][0]["summary"], summary);
    assert_eq!(read["records"][0]["review_note"], review);
}

#[test]
fn compact_directory_batches_projection_without_loading_body_cues_or_topics() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{Arc, Mutex};
    let conn = fixture();
    for n in 0..12 {
        insert_read_memory(&conn, &format!("m{n}"));
    }
    let columns = Arc::new(Mutex::new(Vec::new()));
    let seen = columns.clone();
    conn.authorizer(Some(move |context: AuthContext<'_>| {
        if let AuthAction::Read {
            table_name,
            column_name,
        } = context.action
        {
            seen.lock()
                .unwrap()
                .push((table_name.to_owned(), column_name.to_owned()));
        }
        Authorization::Allow
    }))
    .unwrap();
    let packet = run(&conn, json!({"action":"browse","max_bytes":65536})).unwrap();
    assert_eq!(packet["memories"].as_array().unwrap().len(), 12);
    let reads = columns.lock().unwrap();
    for column in ["state_text", "retrieval_cues_json"] {
        assert!(
            !reads
                .iter()
                .any(|(table, name)| table == "learner_memories" && name == column),
            "compact read loaded {column}: {reads:?}"
        );
    }
    // A topic predicate may read the topic relation; it must not materialize a
    // per-memory topic list. The summary projection itself is one batch.
    assert_eq!(
        reads
            .iter()
            .filter(|(table, name)| table == "learner_memories" && name == "summary")
            .count(),
        1
    );
}

#[test]
fn explicit_full_read_loads_body_once() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let conn = fixture();
    insert_read_memory(&conn, "m");
    let reads = Arc::new(AtomicUsize::new(0));
    let seen = reads.clone();
    conn.authorizer(Some(move |context: AuthContext<'_>| {
        if matches!(
            context.action,
            AuthAction::Read {
                table_name: "learner_memories",
                column_name: "state_text"
            }
        ) {
            seen.fetch_add(1, Ordering::Relaxed);
        }
        Authorization::Allow
    }))
    .unwrap();
    let packet = run(
        &conn,
        json!({"action":"read","refs":[{"kind":"current_memory","id":"m"}]}),
    )
    .unwrap();
    assert_eq!(packet["records"][0]["state_text"], "whole state");
    assert_eq!(reads.load(Ordering::Relaxed), 1);
}

#[test]
fn selected_reads_emit_valid_remaining_refs_with_expected_revisions() {
    let conn = fixture();
    let refs: Vec<_> = (0..16)
        .map(|n| {
            let id = format!("selected-{n}");
            insert_read_memory(&conn, &id);
            conn.execute(
                "UPDATE learner_memories SET state_text=?1 WHERE memory_id=?2",
                params!["complete body ".repeat(80), id],
            )
            .unwrap();
            json!({"kind":"current_memory","id":id,"expected_revision":1})
        })
        .collect();
    let mut packet = run(&conn, json!({"action":"read","refs":refs,"max_bytes":4000})).unwrap();
    let mut seen = Vec::new();
    loop {
        assert!(pages::size(&packet) <= 4000);
        let records = packet["records"].as_array().unwrap();
        assert!(!records.is_empty());
        for record in records {
            seen.push(record["memory_id"].as_str().unwrap().to_owned());
            assert_eq!(record["state_text"], "complete body ".repeat(80));
        }
        let call = packet["next_call"].clone();
        if call.is_null() {
            break;
        }
        let remaining = call["arguments"]["refs"].as_array().unwrap();
        assert_eq!(remaining, &refs[seen.len()..]);
        assert_eq!(call["arguments"]["max_bytes"], 4000);
        packet = strict_next_call(&conn, &call).unwrap();
    }
    assert_eq!(
        seen,
        (0..16).map(|n| format!("selected-{n}")).collect::<Vec<_>>()
    );
}

#[test]
fn optional_ranked_cards_skip_after_required_context_consumes_default_budget() {
    let conn = fixture();
    let priorities = (0..8).map(|n| format!("priority-{n}")).collect::<Vec<_>>();
    for id in &priorities {
        insert_read_memory(&conn, id);
        conn.execute("UPDATE learner_memories SET title=?1,scope=?2,summary=?3,review_note=?4 WHERE memory_id=?5",params!["t".repeat(256),"s".repeat(64),"q".repeat(1024),"r".repeat(1024),id]).unwrap();
    }
    insert_read_memory(&conn, "large");
    insert_read_memory(&conn, "small");
    conn.execute("UPDATE learner_memories SET title='oversized',scope=?1,summary=?2,review_note=?3 WHERE memory_id='large'",params!["s".repeat(512),"x".repeat(1024),"y".repeat(1024)]).unwrap();
    conn.execute("UPDATE learner_memories SET title='smaller',summary='Not independent yet.',review_note='Only with a worked example.' WHERE memory_id='small'",[]).unwrap();
    search::rebuild_indexes(&conn).unwrap();
    let packet = run(&conn,json!({"action":"prepare","mode":"focused","memory_ids":priorities,"intents":[{"id":"first","purpose":"large","queries":[{"all_of":["oversized"]}]},{"id":"second","purpose":"small","queries":[{"all_of":["smaller"]}]}]})).unwrap();
    assert!(pages::size(&packet) <= crate::learner::limits::DEFAULT_PACKET_BYTES);
    let memories = packet["memories"].as_array().unwrap();
    assert_eq!(
        memories.len(),
        9,
        "required context must leave the later small candidate reachable: {packet}"
    );
    assert_eq!(memories[8]["memory_id"], "small");
    assert_eq!(memories[8]["summary"], "Not independent yet.");
    assert_eq!(memories[8]["review_note"], "Only with a worked example.");
    assert_eq!(packet["coverage"]["omitted_card_count"], 1);
}

#[test]
fn compact_and_complete_views_share_strict_canonical_field_decoding() {
    let conn = fixture();
    insert_read_memory(&conn, "m");
    conn.execute("UPDATE learner_memories SET current_basis_json='[1]'", [])
        .unwrap();
    for request in [
        json!({"action":"browse"}),
        json!({"action":"read","refs":[{"kind":"current_memory","id":"m"}]}),
    ] {
        let error = run(&conn, request).unwrap_err();
        assert!(error.contains("stored_data_invalid"), "{error}");
        assert!(error.contains("current_basis_json"), "{error}");
    }
    conn.execute(
        "UPDATE learner_memories SET current_basis_json='[]',retrieval_cues_json='[1]'",
        [],
    )
    .unwrap();
    // A compact projection does not invent an interpretation for omitted fields.
    assert!(run(&conn, json!({"action":"browse"})).is_ok());
    assert!(
        run(
            &conn,
            json!({"action":"read","refs":[{"kind":"current_memory","id":"m"}]})
        )
        .unwrap_err()
        .contains("retrieval_cues_json")
    );
}

thread_local! {
    static READ_SQL: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}
fn traced_read(conn: &Connection, request: Value) -> (Value, Vec<String>) {
    use rusqlite::trace::{TraceEvent, TraceEventCodes};
    READ_SQL.with(|sql| sql.borrow_mut().clear());
    conn.trace_v2(
        TraceEventCodes::SQLITE_TRACE_STMT,
        Some(|event| {
            if let TraceEvent::Stmt(statement, _) = event {
                READ_SQL.with(|sql| sql.borrow_mut().push(statement.sql().into_owned()));
            }
        }),
    );
    let packet = run(conn, request).unwrap();
    conn.trace_v2(TraceEventCodes::empty(), None);
    (
        packet,
        READ_SQL.with(|sql| std::mem::take(&mut *sql.borrow_mut())),
    )
}

#[test]
fn selected_body_batch_and_projection_statement_counts_are_bounded() {
    let conn = fixture();
    for n in 0..12 {
        insert_read_memory(&conn, &format!("m{n}"));
    }
    let (_, directory_sql) = traced_read(&conn, json!({"action":"browse","max_bytes":65536}));
    assert_eq!(
        directory_sql
            .iter()
            .filter(|sql| sql.contains("m.summary"))
            .count(),
        1,
        "{directory_sql:?}"
    );
    assert!(
        !directory_sql
            .iter()
            .any(|sql| sql.contains("state_text") || sql.contains("retrieval_cues_json")),
        "{directory_sql:?}"
    );
    assert!(
        !directory_sql
            .iter()
            .any(|sql| sql.contains("SELECT topic FROM memory_topics")),
        "{directory_sql:?}"
    );
    let (_, prepare_sql) = traced_read(&conn, json!({"action":"prepare","mode":"auto"}));
    assert_eq!(
        prepare_sql
            .iter()
            .filter(|sql| sql.contains("state_text"))
            .count(),
        1,
        "{prepare_sql:?}"
    );
    let (_, full_sql) = traced_read(
        &conn,
        json!({"action":"read","refs":[{"kind":"current_memory","id":"m0"}]}),
    );
    assert_eq!(
        full_sql
            .iter()
            .filter(|sql| sql.contains("state_text"))
            .count(),
        1,
        "{full_sql:?}"
    );
}

#[test]
fn selected_reads_preserve_exact_final_envelope_and_hard_input_bound() {
    let conn = fixture();
    insert_read_memory(&conn, "m");
    let refs = vec![json!({"kind":"current_memory","id":"m","expected_revision":1}); 32];
    let packet = run(
        &conn,
        json!({"action":"read","refs":refs,"max_bytes":65536}),
    )
    .unwrap();
    assert_eq!(packet["records"].as_array().unwrap().len(), 32);
    let exact = pages::size(&packet);
    let bounded = run(
        &conn,
        json!({"action":"read","refs":refs,"max_bytes":exact}),
    )
    .unwrap();
    assert_eq!(pages::size(&bounded), exact);
    assert!(bounded["next_call"].is_null());
    let too_many = vec![json!({"kind":"current_memory","id":"m"}); 33];
    assert!(
        run(&conn, json!({"action":"read","refs":too_many}))
            .unwrap_err()
            .contains("1..32")
    );
    let error=run(&conn,json!({"action":"read","refs":[{"kind":"current_memory","id":"m","expected_revision":1}],"max_bytes":1})).unwrap_err();
    assert!(error.starts_with("budget_too_small"));
    let call: Value = serde_json::from_str(error.split_once("next_call=").unwrap().1).unwrap();
    let recovery = strict_next_call(&conn, &call).unwrap();
    assert_eq!(recovery["records"][0]["memory_id"], "m");
}

#[test]
fn selected_read_continuation_returns_current_revision_without_stale_registry() {
    let conn = fixture();
    for id in ["first", "changed", "retired"] {
        insert_read_memory(&conn, id);
        conn.execute(
            "UPDATE learner_memories SET state_text=?1 WHERE memory_id=?2",
            params!["whole body ".repeat(160), id],
        )
        .unwrap();
    }
    let refs = vec![
        json!({"kind":"current_memory","id":"first","expected_revision":1}),
        json!({"kind":"current_memory","id":"changed","expected_revision":1}),
        json!({"kind":"current_memory","id":"retired","expected_revision":1}),
    ];
    let packet = run(&conn, json!({"action":"read","refs":refs,"max_bytes":3600})).unwrap();
    assert_eq!(packet["records"].as_array().unwrap().len(), 1);
    assert_eq!(packet["next_call"]["arguments"]["refs"], json!(&refs[1..]));
    conn.execute(
        "UPDATE learner_memories SET revision=2 WHERE memory_id='changed'",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE learner_memories SET revision=2,lifecycle='retired' WHERE memory_id='retired'",
        [],
    )
    .unwrap();
    let next = strict_next_call(&conn, &packet["next_call"]).unwrap();
    assert_eq!(next["records"][0]["memory_id"], "changed");
    assert_eq!(next["records"][0]["revision"], 2);
    assert_eq!(next["records"][0]["revision_changed"], true);
    let last = if next["next_call"].is_null() {
        next
    } else {
        strict_next_call(&conn, &next["next_call"]).unwrap()
    };
    let retired = last["records"].as_array().unwrap().last().unwrap();
    assert_eq!(retired["memory_id"], "retired");
    assert_eq!(retired["detail_status"], "retired");
    assert_eq!(retired["revision_changed"], true);
    assert!(retired["state_text"].is_null());
}

#[test]
fn optional_ranked_omissions_count_skips_between_admitted_intents() {
    let conn = fixture();
    for n in 0..2 {
        insert_read_memory(&conn, &format!("large-{n}"));
        insert_read_memory(&conn, &format!("small-{n}"));
    }
    conn.execute("UPDATE learner_memories SET title='oversized',scope=?1,summary=?2,review_note=?3 WHERE memory_id LIKE 'large-%'",params!["s".repeat(512),"x".repeat(1024),"y".repeat(1024)]).unwrap();
    conn.execute("UPDATE learner_memories SET title='smaller',summary='Not independent.',review_note='Only with help.' WHERE memory_id LIKE 'small-%'",[]).unwrap();
    search::rebuild_indexes(&conn).unwrap();
    let packet = run(&conn,json!({"action":"search","mode":"focused","max_bytes":4000,"intents":[{"id":"first","purpose":"large","queries":[{"all_of":["oversized"]}]},{"id":"second","purpose":"small","queries":[{"all_of":["smaller"]}]}]})).unwrap();
    assert_eq!(
        packet["memories"]
            .as_array()
            .unwrap()
            .iter()
            .map(|card| card["memory_id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["small-0", "small-1"]
    );
    assert_eq!(packet["coverage"]["omitted_card_count"], 2);
    assert_eq!(packet["omitted_memory_refs"].as_array().unwrap().len(), 2);
}

#[test]
fn selected_read_continuations_preserve_mixed_references_and_event_ranges() {
    let conn = fixture();
    for id in ["first", "last"] {
        insert_read_memory(&conn, id);
        conn.execute(
            "UPDATE learner_memories SET state_text=?1 WHERE memory_id=?2",
            params!["complete memory ".repeat(120), id],
        )
        .unwrap();
    }
    event(&conn, "e", "h", "fresh", None);
    conn.execute(
        "UPDATE learning_events SET text='évidence entière' WHERE event_id='e'",
        [],
    )
    .unwrap();
    conn.execute("INSERT INTO observations(observation_id,kind,description,conditions_text,assistance_text,source_refs_json,created_at_ms) VALUES('o','question','Not independent','limited conditions','with help','[]',0)",[]).unwrap();
    let refs = vec![
        json!({"kind":"current_memory","id":"first","expected_revision":1}),
        json!({"kind":"event","id":"e","start_byte":0,"end_byte":2}),
        json!({"kind":"observation","id":"o"}),
        json!({"kind":"current_memory","id":"last","expected_revision":1}),
    ];
    let mut packet = run(&conn, json!({"action":"read","refs":refs,"max_bytes":3600})).unwrap();
    let mut received = Vec::new();
    loop {
        assert!(pages::size(&packet) <= 3600);
        let records = packet["records"].as_array().unwrap();
        assert!(!records.is_empty());
        received.extend(records.iter().cloned());
        if packet["next_call"].is_null() {
            break;
        }
        assert_eq!(
            packet["next_call"]["arguments"]["refs"],
            json!(&refs[received.len()..])
        );
        packet = strict_next_call(&conn, &packet["next_call"]).unwrap();
    }
    assert_eq!(received.len(), 4);
    assert_eq!(received[0]["memory_id"], "first");
    assert_eq!(received[1]["event_id"], "e");
    assert_eq!(received[1]["text"], "é");
    assert_eq!(received[1]["partial"], true);
    assert_eq!(received[2]["observation_id"], "o");
    assert_eq!(received[2]["description"], "Not independent");
    assert_eq!(received[3]["memory_id"], "last");
}

#[test]
fn selected_read_large_remaining_ref_envelope_does_not_break_single_record_readability() {
    let conn = fixture();
    insert_read_memory(&conn, "first");
    conn.execute(
        "UPDATE learner_memories SET state_text=?1 WHERE memory_id='first'",
        ["\0".repeat(10000)],
    )
    .unwrap();
    let long_id = "\0".repeat(256);
    insert_read_memory(&conn, &long_id);
    let mut refs = vec![json!({"kind":"current_memory","id":"first","expected_revision":1})];
    refs.extend(vec![
        json!({"kind":"current_memory","id":long_id,"expected_revision":1});
        31
    ]);
    let single = run(
        &conn,
        json!({"action":"read","refs":[refs[0]],"max_bytes":65536}),
    )
    .unwrap();
    assert!(pages::size(&single) > 60000);
    let error = run(
        &conn,
        json!({"action":"read","refs":refs,"max_bytes":65536}),
    )
    .unwrap_err();
    assert!(error.starts_with("budget_too_small"), "{error}");
    let call: Value = serde_json::from_str(error.split_once("next_call=").unwrap().1).unwrap();
    assert_eq!(call["arguments"]["refs"], json!([refs[0]]));
    let recovered = strict_next_call(&conn, &call).unwrap();
    assert_eq!(recovered["records"][0]["state_text"], "\0".repeat(10000));
    assert!(pages::size(&recovered) <= 65536);
}

#[test]
fn native_trace_suppression_leaves_no_receipt_or_ability_changes() {
    let conn = fixture();
    conn.execute("INSERT INTO learner_memories(memory_id,title,scope,summary,state_text,current_basis_json,lifecycle,revision,created_at_ms,updated_at_ms) VALUES('m','title','scope','summary','state','[]','active',1,0,0)", []).unwrap();
    conn.execute("INSERT INTO host_bindings(host_kind,host_thread_id,learning_active,recording_enabled) VALUES('h','fresh',1,1)", []).unwrap();
    for n in 0..2 {
        conn.execute("INSERT INTO review_items(review_id,memory_id,memory_revision,purpose,reason,status,revision,updated_at_ms) VALUES(?1,'m',1,'review','reason','suggested',1,0)", [format!("r{n}")]).unwrap();
    }
    let before = conn.total_changes();
    let suppressed = test_support::execute_with_tracing(
        &conn,
        "h",
        "fresh",
        json!({"schema":crate::learner::SCHEMA,"action":"reviews"}),
        false,
    )
    .unwrap();
    assert_eq!(suppressed["trace_status"], "disabled");
    assert!(suppressed["trace_id"].is_null());
    assert_eq!(suppressed["coverage"]["review_queue_count"], 2);
    assert_eq!(
        conn.query_row("SELECT count(*) FROM context_receipts", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(conn.total_changes(), before);
}
