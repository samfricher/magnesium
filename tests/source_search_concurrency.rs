//! Concurrent source searches expose bounded snapshots, deadlines and source conflicts.
//!
//! Four independent MCP processes search the same initialized workspace at the
//! same time. Every result must be a valid bounded search or an explicit
//! partial/deferred snapshot, ranked deadline, or truthful source-revision
//! conflict; no raw reconciliation cursor retry instruction may reach the model.
//! A source conflict is terminal for that request and contains no old snapshot.
//! Later explicitly requested bounded searches must recover the complete corpus.

use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use support::TestEnvironment;
use support::mcp::McpClient;

const SENTINEL: &str = "concurrency recovery sentinel";
const SOURCE_SUPERSEDED: &str = "Magnesium could not invoke search_sources: source_superseded: a newer source state won; no old result was published. Repeat the source request if it is still useful";
const RANKED_TIMEOUT: &str = "Magnesium could not invoke search_sources: source_search_budget_exhausted: retrieval is incomplete; narrow the query or path scope and retry; this is not evidence that no source exists";

#[derive(Debug)]
enum SearchOutcome {
    Snapshot(Value),
    RankedBudgetExhausted,
    SourceSuperseded,
}

fn decode_search_outcome(response: &Value) -> SearchOutcome {
    let result = &response["result"];
    let text = result["content"][0]["text"].as_str().expect("tool text");
    if result["isError"] == true {
        // Only these exact supported terminal errors are allowed. Revision
        // conflict never borrows a current snapshot or returns old candidates.
        let outcome = match text {
            RANKED_TIMEOUT => SearchOutcome::RankedBudgetExhausted,
            SOURCE_SUPERSEDED => SearchOutcome::SourceSuperseded,
            _ => panic!("unexpected source search error: {response}"),
        };
        assert!(
            serde_json::to_vec(response).unwrap().len() <= 1024,
            "terminal source error must be bounded"
        );
        assert_eq!(result["content"].as_array().unwrap().len(), 1);
        assert!(
            result.get("hits").is_none()
                && result.get("coverage").is_none()
                && result.get("structuredContent").is_none(),
            "terminal source error must not fabricate a snapshot: {response}"
        );
        outcome
    } else {
        let packet = serde_json::from_str(text).expect("tool result JSON");
        assert_bounded_result(&packet);
        SearchOutcome::Snapshot(packet)
    }
}

fn search(client: &mut McpClient, id: u64) -> SearchOutcome {
    decode_search_outcome(&client.request(
        id,
        "tools/call",
        json!({"name":"search_sources","arguments":{"query":SENTINEL,"limit":5}}),
    ))
}

fn initialize_product(environment: &TestEnvironment) {
    let output = environment.command().arg("init").output().expect("init");
    assert!(output.status.success());
}

fn write_corpus(environment: &TestEnvironment, files: usize) {
    let corpus = environment.workspace.join("corpus");
    fs::create_dir_all(&corpus).expect("create corpus directory");
    for index in 0..files {
        fs::write(
            corpus.join(format!("page-{index:03}.md")),
            format!("# Corpus page {index}\n\n{SENTINEL} entry {index}\n"),
        )
        .expect("write corpus page");
    }
}

fn assert_bounded_packet(response: &Value) {
    let coverage = response["coverage"]
        .as_str()
        .unwrap_or_else(|| panic!("coverage field: {response}"));
    assert!(
        matches!(
            coverage,
            "complete" | "partial" | "stale" | "indexing_on_demand"
        ),
        "unexpected coverage: {response}"
    );
    assert!(response["hits"].is_array(), "hits array: {response}");
    assert!(
        response["hits"].as_array().unwrap().len() <= 5,
        "search exceeded its requested hit bound: {response}"
    );
    assert!(
        !response.to_string().contains("retry the query"),
        "a raw reconciliation retry instruction leaked to the model: {response}"
    );
}

fn assert_bounded_result(response: &Value) {
    assert_bounded_packet(response);
    let coverage = response["coverage"].as_str().unwrap();
    if let Some(reconciliation) = response
        .get("reconciliation")
        .filter(|value| !value.is_null())
    {
        assert!(
            matches!(
                reconciliation.as_str(),
                Some("deferred_concurrent_update" | "deferred_budget")
            ),
            "unexpected reconciliation diagnostic: {response}"
        );
        assert!(
            matches!(coverage, "partial" | "stale"),
            "a deferred snapshot cannot claim {coverage}: {response}"
        );
    }
}

fn complete_corpus_with_bounded_requests(environment: &TestEnvironment, files: usize) {
    let mut warm = McpClient::spawn(environment, true);
    warm.initialize();
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut requests = 0;
    loop {
        let outcome = search(&mut warm, 10 + requests);
        requests += 1;
        // Resumption uses the ordinary per-request production budget. A valid
        // deferred response alone is not success: every file must be indexed
        // and the metadata cursor must be complete before this loop finishes.
        if let SearchOutcome::Snapshot(response) = &outcome
            && response["coverage"] == "complete"
        {
            assert_eq!(
                response["hits"].as_array().unwrap().len(),
                5,
                "sequential recovery must return indexed corpus hits: {response}"
            );
            let conn = rusqlite::Connection::open(
                environment
                    .workspace
                    .join(support::CONTROL_DIR)
                    .join("index/source-search.sqlite"),
            )
            .unwrap();
            let indexed: i64 = conn.query_row(
                "SELECT count(*) FROM source_manifest WHERE display_path LIKE 'corpus/page-%.md'
                 AND present=1 AND index_status='indexed'", [], |row| row.get(0),
            ).unwrap();
            assert_eq!(
                indexed,
                i64::try_from(files).unwrap(),
                "concurrency fixture must be fully indexed"
            );
            assert_eq!(
                conn.query_row(
                    "SELECT count(*) FROM source_reconciliation_state",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
                0,
                "cold metadata scan must be finished"
            );
            eprintln!(
                "LEARN-251 resumed: {files} indexed corpus files after {requests} bounded requests"
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "bounded corpus resumption did not complete: {outcome:?}"
        );
    }
    warm.close_cleanly();
}

fn parallel_searches(environment: &TestEnvironment, phase: &str) -> Vec<SearchOutcome> {
    let mut clients = (0..4)
        .map(|_| {
            let mut client = McpClient::spawn(environment, true);
            client.initialize();
            client
        })
        .collect::<Vec<_>>();
    let barrier = Arc::new(Barrier::new(clients.len()));
    let searches = clients
        .drain(..)
        .map(|mut client| {
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                let mut responses = Vec::new();
                let mut request_panicked = false;
                for round in 0..3_u64 {
                    barrier.wait();
                    // Every worker reaches every round barrier, even if an
                    // MCP I/O assertion fails. Decode/assert tool outcomes only
                    // after all workers have completed, so a truthful terminal
                    // error cannot strand the other participants at a barrier.
                    if !request_panicked {
                        let response = catch_unwind(AssertUnwindSafe(|| client.request(
                            100 + round, "tools/call",
                            json!({"name":"search_sources","arguments":{"query":SENTINEL,"limit":5}}),
                        )));
                        request_panicked = response.is_err();
                        responses.push(response);
                    }
                }
                client.close_cleanly();
                responses
            })
        })
        .collect::<Vec<_>>();
    // Join every participant before any assertion or decoding can panic.
    let joined = searches
        .into_iter()
        .map(thread::JoinHandle::join)
        .collect::<Vec<_>>();
    let raw = joined
        .into_iter()
        .flat_map(|worker| worker.expect("join concurrent search"))
        .collect::<Vec<_>>();
    let responses = raw
        .into_iter()
        .map(|response| {
            let response = response.expect("concurrent MCP request panicked");
            if response["result"]["isError"] == true {
                eprintln!("LEARN-251 {phase} terminal response: {response}");
            }
            decode_search_outcome(&response)
        })
        .collect::<Vec<_>>();

    let deferred = responses
        .iter()
        .filter(|outcome| {
            matches!(outcome, SearchOutcome::Snapshot(response)
            if response.get("reconciliation").is_some_and(|value| !value.is_null()))
        })
        .count();
    let deadlines = responses
        .iter()
        .filter(|outcome| matches!(outcome, SearchOutcome::RankedBudgetExhausted))
        .count();
    let conflicts = responses
        .iter()
        .filter(|outcome| matches!(outcome, SearchOutcome::SourceSuperseded))
        .count();
    assert_eq!(
        responses.len(),
        12,
        "all four callers made their three explicit requests"
    );
    eprintln!(
        "LEARN-251 {phase} concurrency: {deferred} preparation deferrals, {deadlines} ranked deadlines, {conflicts} source conflicts of {} responses",
        responses.len()
    );
    responses
}

#[test]
fn terminal_source_conflict_never_admits_other_errors_or_a_snapshot() {
    let response = json!({"jsonrpc":"2.0","id":1,"result":{
        "isError":true,"content":[{"type":"text","text":SOURCE_SUPERSEDED}]
    }});
    for text in [
        "source_anchor_unavailable",
        "source reconciliation cursor changed concurrently; retry the query",
        "Magnesium could not invoke search_sources: source_superseded: stale hits attached",
    ] {
        let mut invalid = response.clone();
        invalid["result"]["content"][0]["text"] = json!(text);
        assert!(
            catch_unwind(|| decode_search_outcome(&invalid)).is_err(),
            "unrecognized error was admitted: {text}"
        );
    }
    for key in ["hits", "coverage", "structuredContent"] {
        let mut invalid = response.clone();
        invalid["result"][key] = json!([]);
        assert!(
            catch_unwind(|| decode_search_outcome(&invalid)).is_err(),
            "terminal source conflict admitted {key}"
        );
    }
}

#[test]
fn four_parallel_searches_report_bounded_outcomes_then_explicit_requests_recover() {
    // Exercise terminal-result decoding even when this machine happens to
    // complete every parallel ranked query inside its deadline on this run.
    assert!(matches!(
        decode_search_outcome(&json!({"jsonrpc":"2.0","id":1,
        "result":{"isError":true,"content":[{"type":"text","text":RANKED_TIMEOUT}]}})),
        SearchOutcome::RankedBudgetExhausted
    ));
    assert!(matches!(
        decode_search_outcome(&json!({"jsonrpc":"2.0","id":2,
        "result":{"isError":true,"content":[{"type":"text","text":SOURCE_SUPERSEDED}]}})),
        SearchOutcome::SourceSuperseded
    ));
    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    initialize_product(&environment);
    write_corpus(&environment, 320);

    // Initialize the source-index schema and establish a saved snapshot. One
    // query deliberately leaves this large corpus cold, preserving the real
    // bounded reconciliation/extraction contention of the original fixture.
    let mut initial = McpClient::spawn(&environment, true);
    initial.initialize();
    search(&mut initial, 10);
    initial.close_cleanly();
    let conn = rusqlite::Connection::open(
        environment
            .workspace
            .join(support::CONTROL_DIR)
            .join("index/source-search.sqlite"),
    )
    .unwrap();
    assert!(
        conn.query_row(
            "SELECT count(*) FROM source_manifest WHERE display_path LIKE 'corpus/page-%.md'
         AND present=1 AND index_status='indexed'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
            < 320,
        "the first parallel phase must exercise a cold corpus"
    );
    drop(conn);
    parallel_searches(&environment, "cold");

    // Deferred snapshots and source-conflict requests can be followed by fresh
    // explicitly requested searches. Verify actual complete indexing, then race
    // again on a fully indexed corpus; the product does not retry a conflict.
    complete_corpus_with_bounded_requests(&environment, 320);
    for outcome in parallel_searches(&environment, "warm") {
        let response = match outcome {
            SearchOutcome::Snapshot(response) => response,
            // A later explicit request must establish successful hits.
            SearchOutcome::RankedBudgetExhausted => continue,
            SearchOutcome::SourceSuperseded => {
                panic!("unchanged fully indexed warm sources must not attempt extraction")
            }
        };
        assert_eq!(
            response["hits"].as_array().unwrap().len(),
            5,
            "a warm snapshot must retain its indexed hits: {response}"
        );
        assert!(
            response["hits"]
                .as_array()
                .unwrap()
                .iter()
                .all(|hit| hit["path"].as_str().unwrap().starts_with("corpus/"))
        );
        assert!(response["generation"].as_i64().unwrap_or(0) >= 1);
        assert!(
            matches!(response["coverage"].as_str(), Some("complete" | "partial")),
            "unchanged fully indexed sources cannot need extraction or be stale: {response}"
        );
    }
    complete_corpus_with_bounded_requests(&environment, 320);
}
