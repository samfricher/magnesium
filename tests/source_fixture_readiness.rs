//! Deterministic continuation policy for readable source integration fixtures.

use std::time::Duration;

use serde_json::{Value, json};
use support::sources::{FixtureKind, prepare_source_fixture_with};

const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn observation(status: &str, coverage: &str, reconciliation: Value) -> (Value, Value) {
    let hits = if status == "indexed" {
        json!([{"path":"math.pdf", "content_sha256":HASH,
                "anchor_kind":"page", "anchor_start":1, "anchor_end":1, "authority":"original_source"}])
    } else {
        json!([])
    };
    let packet = json!({"coverage":coverage, "reconciliation":reconciliation, "hits":hits});
    (
        json!({"result":{"content":[{"text":packet.to_string()}]}}),
        json!({"path":"math.pdf", "content_sha256":HASH, "source_kind":"pdf",
               "index_status":status, "present":1, "exclusion_reason":null, "last_error":null}),
    )
}

#[test]
fn explicit_pending_preparation_resumes_before_one_indexed_result() {
    for (coverage, diagnostic) in [
        ("partial", json!("deferred_budget")),
        ("partial", json!("deferred_concurrent_update")),
        ("indexing_on_demand", Value::Null),
    ] {
        let mut calls = 0;
        let history = prepare_source_fixture_with(
            "math.pdf",
            FixtureKind::Pdf,
            HASH,
            Duration::from_secs(5),
            3,
            |attempt, arguments| {
                assert_eq!(
                    arguments,
                    &json!({"query":"math.pdf", "path_prefix":"math.pdf", "limit":50})
                );
                calls += 1;
                let (response, state) = if attempt == 0 {
                    observation("pending", coverage, diagnostic.clone())
                } else {
                    // On-demand work describes indexing performed, not missing
                    // content. A verified indexed hit is already ready.
                    observation("indexed", "indexing_on_demand", Value::Null)
                };
                (response, Ok(Some(state)))
            },
            || Duration::ZERO,
        )
        .unwrap();
        assert_eq!(calls, 2);
        assert_eq!(history.len(), 2);
        assert_eq!(history[0]["manifest"]["Ok"]["index_status"], "pending");
    }
}

#[test]
fn pending_without_explicit_progress_and_terminal_states_fail_first_observation() {
    let mut cases = Vec::new();
    cases.push(observation("pending", "partial", Value::Null));
    for status in ["failed", "stale", "excluded", "deleted"] {
        cases.push(observation(status, "partial", json!("deferred_budget")));
    }
    for (field, value) in [
        ("content_sha256", json!("b".repeat(64))),
        ("present", json!(0)),
        ("exclusion_reason", json!("magnesiumignore")),
        ("last_error", json!("PDF extraction failed")),
        ("source_kind", json!("text")),
    ] {
        let (response, mut state) = observation("pending", "partial", json!("deferred_budget"));
        state[field] = value;
        cases.push((response, state));
    }
    let (mut terminal, state) = observation("pending", "partial", json!("deferred_budget"));
    terminal["result"]["isError"] = json!(true);
    cases.push((terminal, state));
    for mutate in ["wrong_hash", "no_hits", "malformed_hits"] {
        let (mut response, state) = observation("indexed", "complete", Value::Null);
        let mut packet: Value =
            serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        match mutate {
            "wrong_hash" => packet["hits"][0]["content_sha256"] = json!("b".repeat(64)),
            "no_hits" => packet["hits"] = json!([]),
            _ => packet["hits"] = json!({}),
        }
        response["result"]["content"][0]["text"] = json!(packet.to_string());
        cases.push((response, state));
    }
    for (response, state) in cases {
        let mut calls = 0;
        let error = prepare_source_fixture_with(
            "math.pdf",
            FixtureKind::Pdf,
            HASH,
            Duration::from_secs(5),
            3,
            |_, _| {
                calls += 1;
                (response.clone(), Ok(Some(state.clone())))
            },
            || Duration::ZERO,
        )
        .unwrap_err();
        assert_eq!(calls, 1, "{error}");
        assert!(error.contains("observations:"), "{error}");
        assert!(error.contains("math.pdf"), "{error}");
    }
}

#[test]
fn preparation_has_one_deadline_and_request_cap_without_reset_or_sleep() {
    let (response, state) = observation("pending", "partial", json!("deferred_budget"));
    let mut calls = 0;
    let error = prepare_source_fixture_with(
        "math.pdf",
        FixtureKind::Pdf,
        HASH,
        Duration::from_secs(5),
        2,
        |_, _| {
            calls += 1;
            (response.clone(), Ok(Some(state.clone())))
        },
        || Duration::ZERO,
    )
    .unwrap_err();
    assert_eq!(calls, 2);
    assert!(error.contains("request cap exhausted"), "{error}");

    let mut clock_calls = 0;
    let mut calls = 0;
    let error = prepare_source_fixture_with(
        "math.pdf",
        FixtureKind::Pdf,
        HASH,
        Duration::from_secs(5),
        32,
        |_, _| {
            calls += 1;
            (response.clone(), Ok(Some(state.clone())))
        },
        || {
            clock_calls += 1;
            Duration::from_secs(if clock_calls == 1 { 0 } else { 5 })
        },
    )
    .unwrap_err();
    assert_eq!(calls, 1);
    assert!(
        error.contains("shared deadline exhausted during request"),
        "{error}"
    );
}

fn text_observation(status: &str, coverage: &str, reconciliation: Value) -> (Value, Value) {
    let (mut response, mut state) = observation(status, coverage, reconciliation);
    let mut packet: Value =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    for hit in packet["hits"].as_array_mut().unwrap() {
        hit["path"] = json!("lesson.md");
        hit["anchor_kind"] = json!("line");
    }
    response["result"]["content"][0]["text"] = json!(packet.to_string());
    state["path"] = json!("lesson.md");
    state["source_kind"] = json!("text");
    if status == "undiscovered" {
        let status = json!({"schema_version":1,"action":"status","active":true,"file":{"path":"lesson.md"},
            "anchor":{"kind":"line","physical_start":1,"physical_end":1},"coverage":{"status":"unknown"}});
        state["discovery_status"] =
            json!({"response":{"result":{"content":[{"text":status.to_string()}]}}});
    }
    (response, state)
}

#[test]
fn text_deferred_discovery_is_not_ready_until_exact_indexed_hit() {
    for diagnostic in ["deferred_budget", "deferred_concurrent_update"] {
        let mut calls = 0;
        let history = prepare_source_fixture_with(
            "lesson.md",
            FixtureKind::Text,
            HASH,
            Duration::from_secs(5),
            4,
            |attempt, _| {
                calls += 1;
                let (response, state) = match attempt {
                    0 => text_observation("undiscovered", "partial", json!(diagnostic)),
                    1 => text_observation("pending", "indexing_on_demand", Value::Null),
                    _ => text_observation("indexed", "complete", Value::Null),
                };
                (response, Ok(Some(state)))
            },
            || Duration::ZERO,
        )
        .unwrap();
        assert_eq!(calls, 3);
        assert_eq!(history.len(), 3);
    }
}

#[test]
fn text_identity_policy_and_unqualified_discovery_are_terminal_once() {
    let mut cases = vec![
        text_observation("undiscovered", "partial", Value::Null),
        text_observation("undiscovered", "indexing_on_demand", Value::Null),
        text_observation("indexed", "partial", json!("deferred_budget")),
    ];
    // A correctly indexed hit is removed: unrelated deferral cannot excuse a
    // missing hit unless the recorded status request actually indexed it.
    let mut missing = cases.pop().unwrap();
    let packet = json!({"coverage":"partial", "reconciliation":"deferred_budget", "hits":[]});
    missing.0["result"]["content"][0]["text"] = json!(packet.to_string());
    cases.push(missing);
    for (field, value) in [
        ("content_sha256", json!("b".repeat(64))),
        ("present", json!(0)),
        ("exclusion_reason", json!("magnesiumignore")),
        ("last_error", json!("failed")),
        ("index_status", json!("stale")),
        ("source_kind", json!("pdf")),
    ] {
        let (response, mut state) =
            text_observation("pending", "partial", json!("deferred_budget"));
        state[field] = value;
        cases.push((response, state));
    }
    let (response, mut state) =
        text_observation("undiscovered", "partial", json!("deferred_budget"));
    state["discovery_status"]["response"]["result"]["content"][0]["text"] = json!("{}");
    cases.push((response, state));
    for (response, state) in cases {
        let mut calls = 0;
        let error = prepare_source_fixture_with(
            "lesson.md",
            FixtureKind::Text,
            HASH,
            Duration::from_secs(5),
            4,
            |_, _| {
                calls += 1;
                (response.clone(), Ok(Some(state.clone())))
            },
            || Duration::ZERO,
        )
        .unwrap_err();
        assert_eq!(calls, 1, "{error}");
        assert!(error.contains("observations:"), "{error}");
    }
}

#[test]
fn status_advancement_requires_explicit_progress_before_final_search_hit() {
    let (response, mut state) = text_observation("pending", "partial", json!("deferred_budget"));
    state["index_status"] = json!("indexed");
    state["status_advanced"] = json!(true);
    let mut calls = 0;
    prepare_source_fixture_with(
        "lesson.md",
        FixtureKind::Text,
        HASH,
        Duration::from_secs(5),
        3,
        |attempt, _| {
            calls += 1;
            let observation = if attempt == 0 {
                (response.clone(), state.clone())
            } else {
                text_observation("indexed", "complete", Value::Null)
            };
            (observation.0, Ok(Some(observation.1)))
        },
        || Duration::ZERO,
    )
    .unwrap();
    assert_eq!(calls, 2);
}

#[test]
fn observed_terminal_manifest_cannot_be_healed_by_a_status_request() {
    for (field, value) in [
        ("index_status", json!("failed")),
        ("index_status", json!("stale")),
        ("index_status", json!("excluded")),
        ("index_status", json!("deleted")),
        ("content_sha256", json!("b".repeat(64))),
        ("source_kind", json!("pdf")),
        ("present", json!(0)),
        ("exclusion_reason", json!("magnesiumignore")),
        ("last_error", json!("failed extraction")),
    ] {
        let (_, mut before) = text_observation("pending", "partial", json!("deferred_budget"));
        before[field] = value;
        let mut status_requests = 0;
        let error = support::sources::request_status_after_manifest(
            Some(&before),
            "lesson.md",
            FixtureKind::Text,
            HASH,
            || {
                status_requests += 1;
                let (_, healed) = text_observation("indexed", "complete", Value::Null);
                healed
            },
        )
        .unwrap_err();
        assert_eq!(status_requests, 0, "{error}");
        assert!(error.contains("lesson.md"), "{error}");
    }
}
