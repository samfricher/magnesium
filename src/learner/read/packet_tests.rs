//! Packet selection, continuation and snapshot regressions for learner reads.
use super::{SESSION_CHOICES_SQL, session_choice_rows, test_support::execute};
use crate::learner::search;
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::collections::BTreeSet;

const SCHEMA: &str = crate::learner::SCHEMA;
fn size(value: &Value) -> usize {
    serde_json::to_vec(value).unwrap().len()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(crate::learner::store::SCHEMA_SQL).unwrap();
        c.execute("INSERT INTO workspace_meta(singleton,workspace_id,storage_family,schema_version,created_at_ms) VALUES(1,'w-test','learner-model/v2',2,0)",[]).unwrap();
        c
    }
    fn memory(c: &Connection, id: &str, title: &str, summary: &str, state: &str) {
        c.execute("INSERT INTO learner_memories(memory_id,title,scope,summary,state_text,current_basis_json,lifecycle,revision,created_at_ms,updated_at_ms) VALUES(?1,?2,'algebra',?3,?4,'[]','active',1,0,0)",params![id,title,summary,state]).unwrap();
    }
    fn run(c: &Connection, mut r: Value) -> Result<Value, String> {
        r["schema"] = json!(SCHEMA);
        execute(c, "test-host", "fresh-thread", r)
    }
    #[test]
    fn priority_search_annotations_are_reserved_before_optional_cards() {
        let c = fixture();
        memory(
            &c,
            "priority",
            "coefficient roles",
            "Not a current factoring difficulty",
            "body".repeat(3000).as_str(),
        );
        memory(
            &c,
            "optional",
            "coefficient roles",
            "Other whole summary",
            "body".repeat(3000).as_str(),
        );
        search::rebuild_indexes(&c).unwrap();
        let intent = "matching-intent-".repeat(8);
        let request = json!({"action":"prepare","mode":"focused","memory_ids":["priority"],"intents":[{"id":intent,"purpose":"symbol roles","queries":[{"any_of":["coefficient"]}]}]});
        let mut full = run(&c, request.clone()).unwrap();
        // Reproduce the old admission boundary before priority annotations.
        full["memories"][0]["matched_intents"] = json!([]);
        full["memories"][0]["match_channels"] = json!([]);
        for card in full["memories"].as_array_mut().unwrap() {
            card["state_text"] = Value::Null;
            card["detail_status"] = json!("read_required");
        }
        let budget = size(&full);
        let mut bounded = request;
        bounded["max_bytes"] = json!(budget);
        let packet =
            run(&c, bounded).expect("priority metadata fits; optional card must yield to it");
        assert!(size(&packet) <= budget);
        assert_eq!(packet["memories"][0]["matched_intents"][0], intent);
    }
    #[test]
    fn memory_filtered_observation_cursor_rejects_association_change() {
        let c = fixture();
        memory(&c, "m", "roles", "current", "state");
        for n in 1..=3 {
            c.execute("INSERT INTO observations(observation_id,kind,description,conditions_text,assistance_text,source_refs_json,created_at_ms) VALUES(?1,'question','Observed work','conditions','help','[]',0)", [format!("o{n}")]).unwrap();
        }
        c.execute(
            "INSERT INTO memory_observations VALUES('m','o2'),('m','o3')",
            [],
        )
        .unwrap();
        let page = run(
            &c,
            json!({"action":"observations","memory_id":"m","limit":1}),
        )
        .unwrap();
        c.execute("INSERT INTO memory_observations VALUES('m','o1')", [])
            .unwrap();
        c.execute(
            "UPDATE workspace_meta SET memory_generation=memory_generation+1",
            [],
        )
        .unwrap();
        assert!(run(&c, json!({"action":"observations","memory_id":"m","limit":1,"cursor":page["next_cursor"]})).unwrap_err().starts_with("cursor_stale"));
    }
    #[test]
    fn small_directory_defeats_vocabulary_miss_and_rank_cap() {
        let c = fixture();
        for n in 0..18 {
            memory(
                &c,
                &format!("m{n}"),
                "symbol roles",
                "not a factoring gap",
                "short body",
            );
        }
        let p = run(
            &c,
            json!({"action":"prepare","goal":"totally unrelated vocabulary"}),
        )
        .unwrap();
        assert_eq!(p["selection_mode"], "complete_directory");
        assert_eq!(p["coverage"]["catalog_complete"], true);
        assert_eq!(p["memories"].as_array().unwrap().len(), 18);
        assert_eq!(p["memories"][0]["state_text"], "short body");
        assert_eq!(p["coverage"]["semantic_completeness"], "not_claimed");
    }
    #[test]
    fn browse_exact_once_stale_and_tampered_cursors() {
        let c = fixture();
        for n in 0..7 {
            memory(&c, &format!("m{n}"), "labels", "not confused", "state");
        }
        let mut seen = Vec::new();
        let mut cur = Value::Null;
        loop {
            let p = run(&c, json!({"action":"browse","limit":2,"cursor":cur})).unwrap();
            for x in p["memories"].as_array().unwrap() {
                seen.push(x["memory_id"].as_str().unwrap().to_owned());
            }
            cur = p["next_cursor"].clone();
            if cur.is_null() {
                assert_eq!(p["coverage"]["catalog_complete"], false);
                break;
            }
        }
        assert_eq!(seen.len(), 7);
        assert_eq!(seen.iter().collect::<BTreeSet<_>>().len(), 7);
        let p = run(&c, json!({"action":"browse","limit":2})).unwrap();
        let cursor = p["next_cursor"].clone();
        let mut bad = cursor.clone();
        bad["workspace_id"] = json!("another");
        assert!(
            run(&c, json!({"action":"browse","cursor":bad}))
                .unwrap_err()
                .starts_with("invalid_cursor")
        );
        c.execute("UPDATE workspace_meta SET memory_generation=1", [])
            .unwrap();
        assert!(
            run(&c, json!({"action":"browse","cursor":cursor}))
                .unwrap_err()
                .starts_with("cursor_stale")
        );
    }
    #[test]
    fn distinct_intent_retains_slot_amid_many_target_matches() {
        let c = fixture();
        for n in 0..30 {
            memory(
                &c,
                &format!("target{n:02}"),
                "quadratic formula",
                "substitution strength",
                "state",
            );
        }
        memory(
            &c,
            "prerequisite",
            "coefficient roles",
            "fixed labels unclear; arithmetic strong",
            "state",
        );
        search::rebuild_indexes(&c).unwrap();
        let p=run(&c,json!({"action":"prepare","mode":"focused","intents":[{"id":"target","purpose":"target","queries":[{"any_of":["quadratic"]},{"any_of":["formula"]}]},{"id":"alternative","purpose":"symbol roles","queries":[{"any_of":["coefficient"]}]}]})).unwrap();
        assert_eq!(p["memories"][1]["memory_id"], "prerequisite");
        assert_eq!(p["coverage"]["candidate_lists_truncated"], true);
        assert_eq!(p["memories"].as_array().unwrap().len(), 16);
    }
    #[test]
    fn bytes_preserve_negation_review_notice_and_ranged_unicode() {
        let c = fixture();
        memory(
            &c,
            "m",
            "resolved factoring",
            "No longer a factoring difficulty",
            "long ".repeat(2000).as_str(),
        );
        c.execute("UPDATE learner_memories SET review_note='Disputed: do not infer inability' WHERE memory_id='m'",[]).unwrap();
        let p = run(&c, json!({"action":"prepare","max_bytes":2000})).unwrap();
        assert!(size(&p) <= 2000);
        assert_eq!(
            p["memories"][0]["summary"],
            "No longer a factoring difficulty"
        );
        assert_eq!(
            p["memories"][0]["review_note"],
            "Disputed: do not infer inability"
        );
        assert_eq!(p["memories"][0]["detail_status"], "read_required");
        assert!(
            run(&c, json!({"action":"browse","max_bytes":100}))
                .unwrap_err()
                .starts_with("budget_too_small")
        );
        c.execute("INSERT INTO learning_events(event_id,host_kind,host_thread_id,external_event_key,speaker,kind,origin,text,metadata_json,captured_at_ms,processing) VALUES('e','test-host','fresh-thread','k','learner','answer','model_reported','α − b','{\"assistance\":\"unknown\"}',0,'pending')",[]).unwrap();
        assert!(run(&c,json!({"action":"read","refs":[{"kind":"event","id":"e","start_byte":1,"end_byte":2}]})).unwrap_err().starts_with("invalid_variant"));
        let p = run(
            &c,
            json!({"action":"read","refs":[{"kind":"event","id":"e","start_byte":0,"end_byte":2}]}),
        )
        .unwrap();
        assert_eq!(p["records"][0]["text"], "α");
        assert_eq!(p["records"][0]["partial"], true);
        assert_eq!(p["records"][0]["metadata"]["assistance"], "unknown");
    }
    #[test]
    fn revisions_retired_notices_and_exact_trace_controls() {
        let c = fixture();
        memory(&c, "m", "roles", "uncertain", "current text");
        c.execute("INSERT INTO host_bindings(host_kind,host_thread_id,learning_active,recording_enabled) VALUES('test-host','fresh-thread',1,1)",[]).unwrap();
        let p=run(&c,json!({"action":"read","refs":[{"kind":"current_memory","id":"m","expected_revision":7}]})).unwrap();
        assert_eq!(p["records"][0]["revision_changed"], true);
        let saved: String = c
            .query_row("SELECT response_json FROM context_receipts", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(serde_json::from_str::<Value>(&saved).unwrap(), p);
        c.execute("UPDATE host_bindings SET recording_enabled=0", [])
            .unwrap();
        run(&c, json!({"action":"prepare"})).unwrap();
        assert_eq!(
            c.query_row("SELECT count(*) FROM context_receipts", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        c.execute("UPDATE learner_memories SET lifecycle='retired',revision=2,replacement_ids_json='[\"replacement\"]'",[]).unwrap();
        let p=run(&c,json!({"action":"prepare","known_revisions":[{"memory_id":"m","revision":1}],"memory_ids":["m"]})).unwrap();
        assert_eq!(p["revision_notices"][0]["status"], "retired");
        assert_eq!(p["memories"][0]["state_text"], Value::Null);
        assert_eq!(p["memories"][0]["replacement_ids"][0], "replacement");
    }
    #[test]
    fn history_observations_literals_and_support_remain_explicit() {
        let c = fixture();
        memory(&c, "m", "roles", "Current qualified description", "current");
        c.execute("INSERT INTO observations(observation_id,kind,description,conditions_text,assistance_text,source_refs_json,created_at_ms) VALUES('o','question','Former confusion','coefficient question','help unknown','[]',0)",[]).unwrap();
        c.execute("INSERT INTO learning_events(event_id,host_kind,host_thread_id,external_event_key,speaker,kind,origin,text,metadata_json,captured_at_ms,processing) VALUES('e','h','t','e','learner','answer','model_reported','b ≠ x','{}',0,'pending')",[]).unwrap();
        c.execute(
            "INSERT INTO observation_events VALUES('o','e','learner_response','[]')",
            [],
        )
        .unwrap();
        c.execute("INSERT INTO memory_observations VALUES('m','o')", [])
            .unwrap();
        let mut snapshot = crate::learner::records::memory_snapshot(&c, "m").unwrap();
        snapshot.state_text = "old difficulty".into();
        c.execute("INSERT INTO memory_changes(memory_id,revision,change_kind,snapshot_json,reason,change_basis_json,created_at_ms) VALUES('m',1,'new_observation',?1,'initial question','[]',0)", [serde_json::to_string(&snapshot).unwrap()]).unwrap();
        search::rebuild_indexes(&c).unwrap();
        let p = run(
            &c,
            json!({"action":"observations","query":{"any_of":["confusion"]}}),
        )
        .unwrap();
        assert_eq!(p["observations"][0]["historical"], true);
        assert_eq!(
            p["observations"][0]["current_memory_refs"][0]["memory_id"],
            "m"
        );
        assert_eq!(
            p["observations"][0]["event_refs"][0]["origin"],
            "model_reported"
        );
        let p = run(
            &c,
            json!({"action":"observations","kind":"event","literal_terms":["≠"]}),
        )
        .unwrap();
        assert_eq!(p["observations"][0]["event_id"], "e");
        assert_eq!(p["observations"][0]["partial"], true);
        let p = run(
            &c,
            json!({"action":"observations","kind":"memory_refs","observation_id":"o"}),
        )
        .unwrap();
        assert_eq!(p["support"][0]["memory_id"], "m");
        let p = run(
            &c,
            json!({"action":"observations","kind":"event_refs","observation_id":"o"}),
        )
        .unwrap();
        assert_eq!(p["support"][0]["event_id"], "e");
        let p = run(&c, json!({"action":"history","memory_id":"m"})).unwrap();
        assert_eq!(p["history"][0]["historical"], true);
        assert_eq!(p["history"][0]["snapshot"]["state_text"], "old difficulty");
    }
    #[test]
    fn fresh_host_session_choices_page_without_binding_or_global_generation_claim() {
        let c = fixture();
        for n in 0..7 {
            c.execute(
                "INSERT INTO learning_sessions VALUES(?1,'goal','active','{}',1,0,0)",
                [format!("s{n}")],
            )
            .unwrap();
        }
        let p = run(&c, json!({"action":"prepare","literal_terms":["notation"]})).unwrap();
        assert_eq!(p["session_choices"].as_array().unwrap().len(), 3);
        assert_eq!(p["session"], Value::Null);
        let cursor = p["session_choices_more"]["arguments"]["cursor"].clone();
        c.execute("UPDATE workspace_meta SET memory_generation=1", [])
            .unwrap();
        let p = run(&c, json!({"action":"session_choices","cursor":cursor})).unwrap();
        assert_eq!(p["session_choices"][0]["session_id"], "s3");
        let p = run(
            &c,
            json!({"action":"session_choices","cursor":p["next_cursor"]}),
        )
        .unwrap();
        assert_eq!(p["session_choices"][0]["session_id"], "s6");
        assert_eq!(p["next_cursor"], Value::Null);
        assert_eq!(
            c.query_row("SELECT count(*) FROM host_bindings", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn topics_filter_cursor_tampering_and_current_revision_refresh() {
        let c = fixture();
        memory(&c, "m", "roles", "first state", "full first state");
        c.execute("INSERT INTO memory_topics VALUES('m','symbol roles')", [])
            .unwrap();
        let p = run(&c, json!({"action":"topics"})).unwrap();
        assert_eq!(p["topics"][0]["count"], 1);
        let p = run(&c, json!({"action":"browse","topic":"  Symbol   Roles  "})).unwrap();
        assert_eq!(p["memories"][0]["memory_id"], "m");
        memory(&c, "other", "other", "untagged", "still accessible");
        let p = run(&c, json!({"action":"browse","limit":1})).unwrap();
        let mut cursor = p["next_cursor"].clone();
        cursor["last"] = json!(200);
        assert!(
            run(&c, json!({"action":"browse","cursor":cursor}))
                .unwrap_err()
                .starts_with("invalid_cursor")
        );
        c.execute("UPDATE learner_memories SET revision=2,summary='corrected; not a current gap',state_text='current corrected description' WHERE memory_id='m'",[]).unwrap();
        let p=run(&c,json!({"action":"read","refs":[{"kind":"current_memory","id":"m","expected_revision":1}]})).unwrap();
        assert_eq!(p["records"][0]["revision_changed"], true);
        assert_eq!(
            p["records"][0]["state_text"],
            "current corrected description"
        );
    }
    #[test]
    fn one_packet_keeps_snapshot_when_another_connection_commits_mid_read() {
        let path = std::env::temp_dir().join(format!(
            "magnesium-context-snapshot-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let c = Connection::open(&path).unwrap();
        c.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
        c.execute_batch(crate::learner::store::SCHEMA_SQL).unwrap();
        c.execute("INSERT INTO workspace_meta(singleton,workspace_id,storage_family,schema_version,created_at_ms) VALUES(1,'w-test','learner-model/v2',2,0)",[]).unwrap();
        for n in 0..10 {
            memory(
                &c,
                &format!("m{n}"),
                "roles",
                "original summary",
                "original description",
            );
        }
        let other = Connection::open(&path).unwrap();
        let fired = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let changed = fired.clone();
        let other = std::sync::Mutex::new(other);
        let cancellation = crate::request::OperationCancellation::none().with_test_hook(std::sync::Arc::new(move |phase| {
            if phase == "learner_snapshot_open" && !changed.swap(true, std::sync::atomic::Ordering::SeqCst) {
                other.lock().unwrap().execute_batch("BEGIN IMMEDIATE; UPDATE learner_memories SET revision=2,summary='new summary',state_text='new description' WHERE memory_id='m0'; UPDATE workspace_meta SET memory_generation=1; COMMIT;").unwrap();
            }
        }));
        let scope = crate::request::enter_request_cancellation(&cancellation);
        let p = run(&c, json!({"action":"browse"})).unwrap();
        drop(scope);
        assert!(fired.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(p["memory_generation"], 0);
        assert_eq!(p["memories"][0]["summary"], "original summary");
        assert_eq!(p["memories"][0]["revision"], 1);
        let p=run(&c,json!({"action":"read","refs":[{"kind":"current_memory","id":"m0","expected_revision":1}]})).unwrap();
        assert_eq!(p["memory_generation"], 1);
        assert_eq!(p["records"][0]["state_text"], "new description");
        assert_eq!(p["records"][0]["revision_changed"], true);
        drop(c);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn pending_paging_trace_failure_and_explicit_session_receipt_are_honest() {
        let c = fixture();
        for id in ["bound", "selected"] {
            c.execute(
                "INSERT INTO learning_sessions VALUES(?1,'goal','active','{}',1,0,0)",
                [id],
            )
            .unwrap();
        }
        c.execute(
            "INSERT INTO host_bindings VALUES('test-host','fresh-thread','bound',1,1)",
            [],
        )
        .unwrap();
        for n in 0..4 {
            c.execute("INSERT INTO learning_events(event_id,session_id,host_kind,host_thread_id,external_event_key,speaker,kind,origin,text,metadata_json,captured_at_ms,processing) VALUES(?1,'selected','test-host','fresh-thread',?1,'learner','answer','model_reported','same answer','{}',0,'pending')",[format!("e{n}")]).unwrap();
        }
        let p = run(&c, json!({"action":"prepare","session_id":"selected"})).unwrap();
        assert_eq!(p["pending_event_refs"].as_array().unwrap().len(), 4);
        let logged: String = c
            .query_row(
                "SELECT session_id FROM context_receipts WHERE trace_id=?1",
                [p["trace_id"].as_str().unwrap()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(logged, "selected");
        let p = run(
            &c,
            json!({"action":"pending_events","session_id":"selected","limit":2}),
        )
        .unwrap();
        assert_eq!(p["pending_event_refs"][0]["id"], "e0");
        let p=run(&c,json!({"action":"pending_events","session_id":"selected","limit":2,"cursor":p["next_cursor"]})).unwrap();
        assert_eq!(p["pending_event_refs"][0]["id"], "e2");
        assert_eq!(p["next_cursor"], Value::Null);
        c.execute_batch("DROP TABLE context_receipts").unwrap();
        let p = run(&c, json!({"action":"prepare"})).unwrap();
        assert_eq!(p["trace_status"], "unavailable");
        assert_eq!(p["trace_id"], Value::Null);
        assert_eq!(p["memory_generation"], 0);
    }
    #[test]
    fn pending_choices_page_completed_sessions_without_reading_raw_text() {
        let c = fixture();
        for n in 0..8 {
            c.execute("INSERT INTO learning_sessions VALUES(?1,'coefficient roles','completed','{}',1,0,0)", [format!("s{n}")]).unwrap();
            c.execute("INSERT INTO learning_events(event_id,session_id,host_kind,host_thread_id,external_event_key,speaker,kind,origin,text,metadata_json,captured_at_ms,processing) VALUES(?1,?2,'old-host','old-thread',?1,'learner','answer','host_observed','RAW TEXT MUST NOT BE AUTO-INJECTED','{}',?3,'pending')",params![format!("e{n}"),format!("s{n}"),n]).unwrap();
        }
        c.execute("INSERT INTO learning_events(event_id,host_kind,host_thread_id,external_event_key,speaker,kind,origin,text,metadata_json,captured_at_ms,processing) VALUES('unbound','old-host','old-thread','unbound','learner','message','host_observed','OTHER RAW TEXT','{}',0,'pending')",[]).unwrap();
        let p = run(&c, json!({"action":"prepare"})).unwrap();
        assert_eq!(p["pending_evidence"]["workspace_pending_event_count"], 9);
        assert_eq!(p["pending_evidence"]["current_host_pending_event_count"], 0);
        assert_eq!(p["pending_evidence"]["unbound_pending_event_count"], 1);
        assert_eq!(
            p["pending_evidence"]["interpretation_status"],
            "uninterpreted"
        );
        assert_eq!(p["session"], Value::Null);
        assert!(p["pending_event_refs"].as_array().unwrap().is_empty());
        assert!(!p.to_string().contains("RAW TEXT"));
        let mut seen = p["session_choices"].as_array().unwrap().clone();
        let mut next = p["session_choices_more"]["arguments"].clone();
        loop {
            let page = run(&c, next).unwrap();
            for choice in page["session_choices"].as_array().unwrap() {
                assert_eq!(choice["pending_event_count"], 1);
                assert_eq!(choice["status"], "completed");
                assert_eq!(
                    choice["pending_events_ref"]["arguments"]["session_id"],
                    choice["session_id"]
                );
                assert_eq!(
                    choice["pending_events_ref"]["arguments"]["action"],
                    "pending_events"
                );
                assert!(choice["last_pending_captured_at_ms"].is_i64());
            }
            assert!(!page.to_string().contains("RAW TEXT"));
            seen.extend(page["session_choices"].as_array().unwrap().iter().cloned());
            if page["next_cursor"].is_null() {
                break;
            }
            next = page["next_call"]["arguments"].clone();
        }
        assert_eq!(
            seen.iter()
                .map(|s| s["session_id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7"]
        );
        assert_eq!(
            c.query_row("SELECT count(*) FROM host_bindings", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            c.query_row(
                "SELECT count(*) FROM learning_events WHERE processing='pending'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            9
        );
    }
    #[test]
    fn pending_session_directory_aggregates_events_once_at_scale() {
        let c = fixture();
        c.execute_batch("BEGIN").unwrap();
        for n in 0..600 {
            c.execute(
                "INSERT INTO learning_sessions VALUES(?1,'goal','completed','{}',1,0,0)",
                [format!("s{n:04}")],
            )
            .unwrap();
            for event in 0..10 {
                c.execute("INSERT INTO learning_events(event_id,session_id,host_kind,host_thread_id,external_event_key,speaker,kind,origin,text,metadata_json,captured_at_ms,processing) VALUES(?1,?2,'old-host','old-thread',?1,'learner','answer','host_observed','raw','{}',?3,'pending')",params![format!("e{n:04}-{event}"),format!("s{n:04}"),event]).unwrap();
            }
        }
        c.execute_batch("COMMIT").unwrap();
        let plan = c
            .prepare(&format!("EXPLAIN QUERY PLAN {SESSION_CHOICES_SQL}"))
            .unwrap()
            .query_map([""], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert!(
            !plan.iter().any(|step| step.contains("CORRELATED")),
            "{plan:?}"
        );
        assert_eq!(
            plan.iter()
                .filter(|step| {
                    step.contains("learning_events")
                        && (step.starts_with("SCAN ") || step.starts_with("SEARCH "))
                })
                .count(),
            1,
            "{plan:?}"
        );
        let rows = session_choice_rows(&c, "s0300").unwrap();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0]["session_id"], "s0301");
        assert_eq!(rows[3]["session_id"], "s0304");
        for row in rows {
            assert_eq!(row["pending_event_count"], 10);
            assert_eq!(row["last_pending_captured_at_ms"], 9);
        }
    }
    #[test]
    fn small_pending_context_budgets_keep_explicit_read_continuations() {
        let c = fixture();
        let baseline = run(&c, json!({"action":"prepare"})).unwrap();
        let small = size(&baseline) + 128;
        for n in 0..7 {
            c.execute(
                "INSERT INTO learning_sessions VALUES(?1,?2,'active','{}',1,0,0)",
                params![format!("s{n}"), "a".repeat(180)],
            )
            .unwrap();
        }
        let p = run(&c, json!({"action":"prepare","max_bytes":small})).unwrap();
        assert!(size(&p) <= small);
        assert!(p["session_choices"].as_array().unwrap().is_empty());
        assert_eq!(
            p["session_choices_more"]["arguments"]["action"],
            "session_choices"
        );
        let baseline = run(&c, json!({"action":"prepare","session_id":"s0"})).unwrap();
        let budget = size(&baseline) + 500;
        for n in 0..35 {
            c.execute("INSERT INTO learning_events(event_id,session_id,host_kind,host_thread_id,external_event_key,speaker,kind,origin,text,metadata_json,captured_at_ms,processing) VALUES(?1,'s0','old-host','old-thread',?1,'learner','answer','host_observed','b=-2','{}',0,'pending')",[format!("e{n:02}-{}","z".repeat(200))]).unwrap();
        }
        let p = run(
            &c,
            json!({"action":"prepare","session_id":"s0","max_bytes":budget}),
        )
        .unwrap();
        assert!(size(&p) <= budget);
        assert!(p["pending_event_refs"].as_array().unwrap().len() < 32);
        assert_eq!(p["pending_events_more"], true);
        assert_eq!(
            p["pending_events_continuation"]["arguments"]["session_id"],
            "s0"
        );
        let first = run(
            &c,
            json!({"action":"pending_events","session_id":"s0","limit":32}),
        )
        .unwrap();
        assert_eq!(first["pending_event_refs"].as_array().unwrap().len(), 32);
        let last=run(&c,json!({"action":"pending_events","session_id":"s0","limit":32,"cursor":first["next_cursor"]})).unwrap();
        assert_eq!(last["pending_event_refs"].as_array().unwrap().len(), 3);
        assert_eq!(last["next_cursor"], Value::Null);
        assert!(
            run(&c, json!({"action":"prepare","max_bytes":1}))
                .unwrap_err()
                .starts_with("budget_too_small")
        );
    }
    #[test]
    fn last_pages_fit_their_exact_returned_envelopes() {
        let c = fixture();
        memory(&c, "m", "roles", "whole summary", "whole state");
        for t in ["algebra", "symbols"] {
            c.execute("INSERT INTO memory_topics VALUES('m',?1)", [t])
                .unwrap();
        }
        for id in ["s1", "s2"] {
            c.execute(
                "INSERT INTO learning_sessions VALUES(?1,'goal','active','{}',1,0,0)",
                [id],
            )
            .unwrap();
        }
        for action in ["browse", "topics", "session_choices", "pending_events"] {
            let p = run(&c, json!({"action":action})).unwrap();
            let exact = size(&p);
            let result = run(&c, json!({"action":action,"max_bytes":exact}));
            assert!(result.is_ok(), "{action} exact budget {exact}: {result:?}");
        }
    }
}
