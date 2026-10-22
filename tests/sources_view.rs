//! LEARN-206: original-source inspection with trustworthy anchors.
//!
//! A learner opens the exact cited physical anchor and compares it with the
//! extracted excerpt using the original file version. Changed, deleted,
//! ignored, nested, linked-but-unpermitted and symlink-escape sources leak
//! no bytes and never serve stale bytes as current authority. Viewing
//! creates no learner evidence and imports no book into model context.
//! Text-only hosts receive a usable status/anchor fallback.

use std::fs;

use serde_json::{Value, json};

use support::TestEnvironment;
use support::mcp::McpClient;
use support::snapshot::DatabaseSnapshot;

#[path = "sources_view/exact.rs"]
mod exact;
#[path = "sources_view/pdf.rs"]
mod pdf;

const CURRENT_CANONICAL_TABLES: &[&str] = &[
    "learning_sessions",
    "host_bindings",
    "learning_events",
    "observations",
    "learner_memories",
    "memory_changes",
    "operation_receipts",
    "review_items",
    "context_receipts",
];

const LESSON: &str = "# WAL end marks\n\nWAL readers pin the end mark while writers append frames.\nA second line names the boundary condition.\nA third line closes the lesson.\n";

fn initialize_product(environment: &TestEnvironment) {
    let output = environment.command().arg("init").output().expect("init");
    assert!(output.status.success());
}

fn store_path(environment: &TestEnvironment) -> std::path::PathBuf {
    environment
        .workspace
        .join(support::CONTROL_DIR)
        .join("state.sqlite")
}

fn search_hit_for_path(
    environment: &TestEnvironment,
    client: &mut McpClient,
    id: u64,
    query: &str,
    path: &str,
    bytes: impl AsRef<[u8]>,
) -> Value {
    let expected_hash = support::sources::fixture_hash(bytes.as_ref());
    let preparation =
        support::sources::prepare_text_fixture(environment, client, 1000 + id * 100, path, bytes);
    // Preserve the semantic query itself as one final, un-retried oracle.
    let search = client.call_value(id, "search_sources", json!({"query": query, "limit": 20}));
    let hits = search["hits"].as_array().expect("hits").clone();
    let hit = hits
        .iter()
        .find(|hit| hit["path"].as_str() == Some(path))
        .unwrap_or_else(|| {
            panic!("expected hit for {path}; got {search}; preparation: {preparation:?}")
        })
        .clone();
    assert_eq!(hit["content_sha256"], expected_hash, "{search}");
    assert_eq!(hit["anchor_kind"], "line", "{search}");
    assert_eq!(hit["authority"], "original_source", "{search}");
    hit
}

fn sources_error(client: &mut McpClient, id: u64, arguments: Value) -> String {
    let response = client.request(
        id,
        "tools/call",
        json!({"name": "sources", "arguments": arguments}),
    );
    assert_eq!(
        response["result"]["isError"], true,
        "expected a sources tool error: {response}"
    );
    response["result"]["content"][0]["text"]
        .as_str()
        .expect("error text")
        .to_owned()
}

#[test]
fn inspect_text_anchor_returns_bounded_original_without_learner_state() {
    let _source_capacity = support::sources::source_test_guard();
    let environment = TestEnvironment::from_fixture("basic").expect("env");
    initialize_product(&environment);
    fs::write(environment.workspace.join("lesson.md"), LESSON).expect("write lesson");
    let mut client = McpClient::spawn(&environment, true);
    client.initialize();

    let reference = search_hit_for_path(
        &environment,
        &mut client,
        100,
        "end mark",
        "lesson.md",
        LESSON,
    );
    let hash = reference["content_sha256"]
        .as_str()
        .expect("hash")
        .to_owned();
    let start = reference["anchor_start"].as_u64().expect("start");
    let end = reference["anchor_end"].as_u64().expect("end");

    let before = DatabaseSnapshot::capture(&store_path(&environment), CURRENT_CANONICAL_TABLES)
        .expect("canonical snapshot");

    // Inspect via the returned path/hash/anchor: exact anchor, excerpt, original.
    let inspected = client.call_value(
        101,
        "sources",
        json!({"action": "inspect", "path": reference["path"], "content_sha256": reference["content_sha256"], "anchor_kind": reference["anchor_kind"], "anchor_start": reference["anchor_start"], "anchor_end": reference["anchor_end"]}),
    );
    assert_eq!(inspected["schema_version"], 1);
    assert_eq!(inspected["action"], "inspect");
    assert_eq!(inspected["file"]["path"], "lesson.md");
    assert_eq!(inspected["file"]["content_sha256"], hash);
    assert_eq!(inspected["file"]["source_kind"], "text");
    assert_eq!(inspected["anchor"]["kind"], "line");
    assert_eq!(inspected["anchor"]["physical_start"], start);
    assert_eq!(inspected["anchor"]["physical_end"], end);
    assert!(
        inspected["anchor"]["display_label"]
            .as_str()
            .is_some_and(|label| label.contains("line")),
        "{inspected}"
    );
    assert_eq!(inspected["authority"]["type"], "original_source");
    assert!(inspected["authority"]["workspace_id"].is_string());
    assert_eq!(inspected["coverage"]["status"], "complete", "{inspected}");
    assert!(
        inspected["excerpt"]
            .as_str()
            .is_some_and(|excerpt| excerpt.contains("end mark")),
        "{inspected}"
    );
    assert_eq!(inspected["original"]["encoding"], "utf8");
    assert_eq!(inspected["original"]["content_sha256"], hash);
    assert!(
        inspected["original"]["text"]
            .as_str()
            .is_some_and(|text| text.contains("end mark")),
        "original bytes must match the cited version: {inspected}"
    );
    // Bounded: no book is imported into model context.
    assert!(
        inspected["excerpt"].as_str().unwrap().chars().count() <= 65_536,
        "{inspected}"
    );
    assert!(inspected["original"]["text"].as_str().unwrap().len() <= 64 * 1024);
    // Never a visual-analysis claim from text.
    assert_eq!(inspected["basis"]["visual_analysis"], false);
    assert_eq!(inspected["extraction"]["text_layer_only"], true);

    // The same anchor opens via an explicit path reference.
    let opened = client.call_value(
        102,
        "sources",
        json!({
            "action": "inspect",
            "path": "lesson.md",
            "content_sha256": hash,
            "anchor_kind": "line",
            "anchor_start": start,
            "anchor_end": end,
        }),
    );
    assert_eq!(opened["coverage"]["status"], "complete", "{opened}");
    assert_eq!(opened["original"]["content_sha256"], hash);

    // Text-only fallback: status carries identity/anchor/coverage, no bytes.
    let status = client.call_value(
        103,
        "sources",
        json!({"action": "status", "path": reference["path"], "content_sha256": reference["content_sha256"], "anchor_kind": reference["anchor_kind"], "anchor_start": reference["anchor_start"], "anchor_end": reference["anchor_end"]}),
    );
    assert_eq!(status["coverage"]["status"], "complete", "{status}");
    assert_eq!(status["file"]["content_sha256"], hash);
    assert!(status["anchor"]["display_label"].is_string());
    assert!(status.get("excerpt").is_none(), "{status}");
    assert!(status.get("original").is_none(), "{status}");

    // Viewing created no learner evidence.
    let after = DatabaseSnapshot::capture(&store_path(&environment), CURRENT_CANONICAL_TABLES)
        .expect("canonical snapshot");
    assert_eq!(
        before, after,
        "source viewing changed canonical schema or contents"
    );

    client.close_cleanly();
}

#[test]
fn stale_changed_source_reports_stale_without_bytes() {
    let _source_capacity = support::sources::source_test_guard();
    let environment = TestEnvironment::from_fixture("basic").expect("env");
    initialize_product(&environment);
    fs::write(environment.workspace.join("lesson.md"), LESSON).expect("write v1");
    let mut client = McpClient::spawn(&environment, true);
    client.initialize();

    let reference_v1 = search_hit_for_path(
        &environment,
        &mut client,
        100,
        "end mark",
        "lesson.md",
        LESSON,
    );
    let hash_v1 = reference_v1["content_sha256"]
        .as_str()
        .expect("v1 hash")
        .to_owned();

    fs::write(
        environment.workspace.join("lesson.md"),
        format!("{LESSON}An extra sentence changes the snapshot.\n"),
    )
    .expect("write v2");

    let stale = client.call_value(
        101,
        "sources",
        json!({"action": "inspect", "path": reference_v1["path"], "content_sha256": reference_v1["content_sha256"], "anchor_kind": reference_v1["anchor_kind"], "anchor_start": reference_v1["anchor_start"], "anchor_end": reference_v1["anchor_end"]}),
    );
    assert_eq!(stale["coverage"]["status"], "stale", "{stale}");
    assert!(stale.get("excerpt").is_none(), "{stale}");
    assert!(stale.get("original").is_none(), "{stale}");
    let current = stale["current_content_sha256"]
        .as_str()
        .expect("current hash");
    assert_ne!(current, hash_v1);

    // The fresh anchor inspects the new version.
    let reference_v2 = search_hit_for_path(
        &environment,
        &mut client,
        102,
        "extra sentence",
        "lesson.md",
        format!("{LESSON}An extra sentence changes the snapshot.\n"),
    );
    assert_eq!(reference_v2["content_sha256"].as_str().unwrap(), current);
    let fresh = client.call_value(
        103,
        "sources",
        json!({"action": "inspect", "path": reference_v2["path"], "content_sha256": reference_v2["content_sha256"], "anchor_kind": reference_v2["anchor_kind"], "anchor_start": reference_v2["anchor_start"], "anchor_end": reference_v2["anchor_end"]}),
    );
    assert_eq!(fresh["coverage"]["status"], "complete", "{fresh}");
    assert!(
        fresh["original"]["text"]
            .as_str()
            .is_some_and(|text| text.contains("extra sentence")),
        "{fresh}"
    );

    client.close_cleanly();
}

#[test]
fn deleted_ignored_and_symlink_sources_leak_no_bytes() {
    let _source_capacity = support::sources::source_test_guard();
    let environment = TestEnvironment::from_fixture("basic").expect("env");
    initialize_product(&environment);
    fs::write(environment.workspace.join("lesson.md"), LESSON).expect("write lesson");
    let mut client = McpClient::spawn(&environment, true);
    client.initialize();

    let reference = search_hit_for_path(
        &environment,
        &mut client,
        100,
        "end mark",
        "lesson.md",
        LESSON,
    );

    // Deleted sources authorize nothing.
    fs::remove_file(environment.workspace.join("lesson.md")).expect("delete source");
    let deleted = client.call_value(
        101,
        "sources",
        json!({"action": "inspect", "path": reference["path"], "content_sha256": reference["content_sha256"], "anchor_kind": reference["anchor_kind"], "anchor_start": reference["anchor_start"], "anchor_end": reference["anchor_end"]}),
    );
    assert_eq!(deleted["coverage"]["status"], "deleted", "{deleted}");
    assert!(deleted.get("excerpt").is_none(), "{deleted}");
    assert!(deleted.get("original").is_none(), "{deleted}");

    // Ignored sources authorize nothing.
    fs::write(environment.workspace.join("lesson.md"), LESSON).expect("restore");
    fs::write(
        environment
            .workspace
            .join(support::CONTROL_DIR)
            .join("magnesiumignore"),
        "lesson.md\n",
    )
    .expect("exclude");
    let excluded = client.call_value(
        102,
        "sources",
        json!({"action": "status", "path": "lesson.md"}),
    );
    assert_eq!(excluded["coverage"]["status"], "excluded", "{excluded}");
    assert!(excluded.get("excerpt").is_none(), "{excluded}");
    assert!(excluded.get("original").is_none(), "{excluded}");
    assert_no_delivery_claim(&excluded);
    fs::remove_file(
        environment
            .workspace
            .join(support::CONTROL_DIR)
            .join("magnesiumignore"),
    )
    .expect("unexclude");

    // Symlink escapes never serve the link target's bytes.
    let outside = environment.root.join("outside-secret.md");
    fs::write(&outside, "outside secret bytes\n").expect("outside file");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, environment.workspace.join("evil.md"))
        .expect("symlink escape");
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&outside, environment.workspace.join("evil.md"))
        .expect("symlink escape");
    let escaped = client.call_value(
        103,
        "sources",
        json!({"action": "inspect", "path": "evil.md"}),
    );
    assert_ne!(escaped["coverage"]["status"], "complete", "{escaped}");
    let serialized = serde_json::to_string(&escaped).expect("serialize");
    assert!(!serialized.contains("outside secret bytes"), "{escaped}");
    assert!(escaped.get("original").is_none(), "{escaped}");

    // Traversal never reaches outside the workspace.
    let traversal = sources_error(
        &mut client,
        104,
        json!({"action": "inspect", "path": "../outside-secret.md"}),
    );
    assert!(traversal.contains("workspace-relative"), "{traversal}");

    client.close_cleanly();
}

fn assert_range_issue(response: &Value, kind: &str, start: u32, end: u32, reason: &str) {
    let issue = json!({"kind":kind, "start":start, "end":end, "reason":reason});
    assert!(
        response["coverage"]["range_issues"]
            .as_array()
            .is_some_and(|issues| issues.contains(&issue)),
        "expected {issue}: {response}"
    );
}

fn assert_no_delivery_claim(response: &Value) {
    assert_eq!(response["basis"]["visual_analysis"], false, "{response}");
    assert_eq!(
        response["basis"]["note"], "No visual analysis was performed.",
        "{response}"
    );
}

#[test]
fn text_requests_keep_requested_bounds_and_exact_original_intersections() {
    let _source_capacity = support::sources::source_test_guard();
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    initialize_product(&environment);
    let bytes = b"first line\nlast line\n";
    fs::write(environment.workspace.join("two.txt"), bytes).unwrap();
    let mut client = McpClient::spawn(&environment, true);
    client.initialize();
    support::sources::prepare_text_fixture(&environment, &mut client, 1000, "two.txt", bytes);
    let before =
        DatabaseSnapshot::capture(&store_path(&environment), CURRENT_CANONICAL_TABLES).unwrap();
    let hash = support::sources::fixture_hash(bytes);

    for (index, action) in ["inspect", "status"].into_iter().enumerate() {
        let response = client.call_value(
            100 + index as u64,
            "sources",
            json!({"action":action, "path":"two.txt", "content_sha256":hash,
                "anchor_start":1, "anchor_end":10}),
        );
        assert_eq!(response["anchor"]["kind"], "line", "{response}");
        assert_eq!(response["anchor"]["physical_start"], 1, "{response}");
        assert_eq!(response["anchor"]["physical_end"], 10, "{response}");
        assert_eq!(response["coverage"]["status"], "partial", "{response}");
        assert_range_issue(&response, "line", 3, 10, "outside_known_extent");
        assert_no_delivery_claim(&response);
        if action == "status" {
            assert!(response.get("original").is_none(), "{response}");
            assert!(response.get("excerpt").is_none(), "{response}");
            assert!(response.get("excerpt_ranges").is_none(), "{response}");
        } else {
            assert_eq!(response["original"]["text"], "first line\nlast line");
            assert_eq!(response["original"]["line_start"], 1, "{response}");
            assert_eq!(response["original"]["line_end"], 2, "{response}");
            assert_eq!(response["original"]["total_lines"], 2, "{response}");
            assert_eq!(response["original"]["content_sha256"], hash, "{response}");
        }
    }

    let nonexistent = client.call_value(
        110,
        "sources",
        json!({"action":"inspect", "path":"two.txt", "content_sha256":hash,
            "anchor_start":10, "anchor_end":12}),
    );
    assert_eq!(
        nonexistent["coverage"]["status"], "partial",
        "{nonexistent}"
    );
    assert_eq!(nonexistent["anchor"]["physical_start"], 10);
    assert!(nonexistent["original"].is_null(), "{nonexistent}");
    assert_range_issue(&nonexistent, "line", 10, 12, "outside_known_extent");
    assert!(
        !nonexistent["excerpt"]
            .as_str()
            .unwrap_or_default()
            .contains("last line")
    );

    let open = client.call_value(
        111,
        "sources",
        json!({"action":"inspect", "path":"two.txt"}),
    );
    assert_eq!(open["coverage"]["status"], "complete", "{open}");
    assert!(open["anchor"]["physical_end"].is_null(), "{open}");
    assert_eq!(open["original"]["text"], "first line\nlast line");
    let maximum = client.call_value(
        112,
        "sources",
        json!({"action":"inspect", "path":"two.txt", "anchor_start":1, "anchor_end":u32::MAX}),
    );
    assert_eq!(maximum["anchor"]["physical_end"], u32::MAX, "{maximum}");
    assert_eq!(maximum["coverage"]["status"], "partial", "{maximum}");
    assert_range_issue(&maximum, "line", 3, u32::MAX, "outside_known_extent");
    assert!(maximum.to_string().len() < 16 * 1024, "{maximum}");
    let missing_start = sources_error(
        &mut client,
        113,
        json!({"action":"inspect", "path":"two.txt", "anchor_end":u32::MAX}),
    );
    assert!(
        missing_start.contains("anchor_end without anchor_start"),
        "{missing_start}"
    );
    assert_eq!(
        before,
        DatabaseSnapshot::capture(&store_path(&environment), CURRENT_CANONICAL_TABLES).unwrap()
    );
    client.close_cleanly();
}

#[test]
fn text_line_selection_matches_unicode_crlf_and_final_newline_semantics() {
    let _source_capacity = support::sources::source_test_guard();
    for (bytes, expected, lines) in [
        ("α first\nβ last", "α first\nβ last", 2),
        ("α first\nβ last\n", "α first\nβ last", 2),
        ("α first\r\nβ last\r\n", "α first\nβ last", 2),
        ("α first\n\n", "α first\n", 2),
    ] {
        let environment = TestEnvironment::from_fixture("basic").unwrap();
        initialize_product(&environment);
        fs::write(environment.workspace.join("unicode.txt"), bytes).unwrap();
        let mut client = McpClient::spawn(&environment, true);
        client.initialize();
        support::sources::prepare_text_fixture(
            &environment,
            &mut client,
            1000,
            "unicode.txt",
            bytes,
        );
        let response = client.call_value(
            100,
            "sources",
            json!({"action":"inspect", "path":"unicode.txt", "anchor_start":1, "anchor_end":lines}),
        );
        assert_eq!(response["original"]["text"], expected, "{response}");
        assert_eq!(response["original"]["total_lines"], lines, "{response}");
        assert_eq!(
            response["original"]["content_sha256"],
            support::sources::fixture_hash(bytes)
        );
        let last = client.call_value(
            101,
            "sources",
            json!({"action":"inspect", "path":"unicode.txt", "anchor_start":2, "anchor_end":2}),
        );
        assert_eq!(last["original"]["line_start"], 2, "{last}");
        assert_eq!(last["original"]["line_end"], 2, "{last}");
        assert_eq!(
            last["original"]["text"],
            if bytes.ends_with("\n\n") {
                ""
            } else {
                "β last"
            }
        );
        client.close_cleanly();
    }
}

#[test]
fn empty_and_unicode_blank_text_have_no_fabricated_or_readable_line() {
    let _source_capacity = support::sources::source_test_guard();
    for (bytes, empty) in [("", true), ("\u{2003}\u{a0}\n", false)] {
        let environment = TestEnvironment::from_fixture("basic").unwrap();
        initialize_product(&environment);
        fs::write(environment.workspace.join("blank.txt"), bytes).unwrap();
        let mut client = McpClient::spawn(&environment, true);
        client.initialize();
        let _ = client.call_value(
            100,
            "search_sources",
            json!({"query":"blank.txt", "path_prefix":"blank.txt"}),
        );
        let response = client.call_value(
            101,
            "sources",
            json!({"action":"inspect", "path":"blank.txt"}),
        );
        assert_eq!(
            response["file"]["content_sha256"],
            support::sources::fixture_hash(bytes),
            "{response}"
        );
        assert!(response["anchor"]["physical_end"].is_null(), "{response}");
        if empty {
            assert_eq!(response["coverage"]["status"], "partial", "{response}");
            assert_eq!(response["coverage"]["empty_source"], true, "{response}");
            assert!(response["original"].is_null(), "{response}");
        } else {
            assert_eq!(
                response["coverage"]["status"], "text_unavailable",
                "{response}"
            );
            assert_range_issue(&response, "line", 1, 1, "unreadable_extraction");
            assert_eq!(response["original"]["text"], "\u{2003}\u{a0}");
        }
        let explicit = client.call_value(
            102,
            "sources",
            json!({"action":"status", "path":"blank.txt", "anchor_start":1, "anchor_end":3}),
        );
        assert_range_issue(
            &explicit,
            "line",
            if empty { 1 } else { 2 },
            3,
            "outside_known_extent",
        );
        assert!(explicit.get("original").is_none(), "{explicit}");
        client.close_cleanly();
    }
}

#[test]
fn source_path_policy_accepts_local_text_and_rejects_invalid_references() {
    let _source_capacity = support::sources::source_test_guard();
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    initialize_product(&environment);
    fs::write(environment.workspace.join("valid.txt"), "permitted text\n").unwrap();
    let mut client = McpClient::spawn(&environment, true);
    client.initialize();
    support::sources::prepare_text_fixture(
        &environment,
        &mut client,
        1000,
        "valid.txt",
        "permitted text\n",
    );
    let valid = client.call_value(
        100,
        "sources",
        json!({"action":"inspect", "path":"valid.txt"}),
    );
    assert_eq!(valid["original"]["text"], "permitted text", "{valid}");
    for (index, path) in [
        format!("{}/{}.pdf", "a".repeat(130), "b".repeat(130)),
        "valid\n.txt".to_owned(),
        "../valid.txt".to_owned(),
        "/valid.txt".to_owned(),
    ]
    .into_iter()
    .enumerate()
    {
        let error = sources_error(
            &mut client,
            101 + index as u64,
            json!({"action":"inspect", "path":path}),
        );
        assert!(error.contains("path"), "{error}");
    }
    client.close_cleanly();
}

#[test]
fn overlapping_text_chunks_report_their_actual_ranges_and_cover_the_request() {
    let _source_capacity = support::sources::source_test_guard();
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    initialize_product(&environment);
    let text = (1..=250)
        .map(|line| format!("overlap line {line:03}\n"))
        .collect::<String>();
    fs::write(environment.workspace.join("overlap.txt"), &text).unwrap();
    let mut client = McpClient::spawn(&environment, true);
    client.initialize();
    support::sources::prepare_text_fixture(&environment, &mut client, 1000, "overlap.txt", &text);
    let response = client.call_value(
        100,
        "sources",
        json!({"action":"inspect", "path":"overlap.txt", "anchor_start":115, "anchor_end":125}),
    );
    assert_eq!(response["coverage"]["status"], "complete", "{response}");
    assert_eq!(
        response["original"]["text"],
        (115..=125)
            .map(|line| format!("overlap line {line:03}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(
        response["coverage"]["range_issues"]
            .as_array()
            .unwrap()
            .is_empty(),
        "{response}"
    );
    let ranges = response["excerpt_ranges"]
        .as_array()
        .expect("excerpt ranges");
    assert!(
        ranges.contains(&json!({"kind":"line", "start":1, "end":120})),
        "{response}"
    );
    assert!(
        ranges.contains(&json!({"kind":"line", "start":113, "end":232})),
        "{response}"
    );
    assert!(
        response["excerpt"]
            .as_str()
            .unwrap()
            .contains("overlap line 001"),
        "{response}"
    );
    client.close_cleanly();
}

/// search_sources and the search layer share one limit bound, so a limit the
/// tool accepts never fails deeper down.
#[test]
fn search_accepts_every_limit_the_tool_accepts() {
    let _source_capacity = support::sources::source_test_guard();
    let environment = TestEnvironment::from_fixture("basic").expect("env");
    initialize_product(&environment);
    fs::write(environment.workspace.join("lesson.md"), LESSON).expect("write lesson");
    let mut client = McpClient::spawn(&environment, true);
    client.initialize();
    for (id, limit) in [(100, 1), (101, 50), (102, 51), (103, 64)] {
        let found = client.call_value(
            id,
            "search_sources",
            json!({"query": "end mark", "limit": limit}),
        );
        assert!(found["hits"].is_array(), "limit {limit}: {found}");
    }
    let over = client.request(
        104,
        "tools/call",
        json!({"name": "search_sources", "arguments": {"query": "end mark", "limit": 65}}),
    );
    assert_eq!(over["result"]["isError"], true, "{over}");
    client.close_cleanly();
}
