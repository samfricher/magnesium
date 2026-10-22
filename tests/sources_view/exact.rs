use super::*;

use rusqlite::{Connection, OpenFlags, OptionalExtension};

#[derive(Clone, Copy, Debug)]
enum SourceKind {
    Text,
    Pdf,
}

impl SourceKind {
    fn extension(self) -> &'static str {
        match self {
            Self::Text => "txt",
            Self::Pdf => "pdf",
        }
    }

    fn bytes(self) -> &'static [u8] {
        match self {
            Self::Text => b"Cobalt readers preserve the selected boundary.\n",
            Self::Pdf => include_bytes!("../fixtures/interaction/statistics/source-pages.pdf"),
        }
    }

    fn anchor(self) -> (&'static str, u32) {
        match self {
            Self::Text => ("line", 1),
            Self::Pdf => ("page", 2),
        }
    }

    fn query(self) -> &'static str {
        match self {
            Self::Text => "Cobalt readers",
            Self::Pdf => "bound",
        }
    }
}

fn filename_cases(extension: &str) -> Vec<String> {
    let basenames = vec![
        "ordinary".to_owned(),
        "lesson draft".to_owned(),
        "learner's draft".to_owned(),
        "lesson(draft)".to_owned(),
        "données/étude α".to_owned(),
        (0..33)
            .map(|word| format!("w{word}"))
            .collect::<Vec<_>>()
            .join(" "),
    ];
    let basenames = if cfg!(unix) {
        basenames
            .into_iter()
            .chain(["lesson\"draft".to_owned(), "lesson*draft".to_owned()])
            .collect()
    } else {
        basenames
    };
    basenames
        .into_iter()
        .map(|basename| format!("{basename}.{extension}"))
        .collect()
}

fn source_index(environment: &TestEnvironment) -> Connection {
    Connection::open_with_flags(
        environment
            .workspace
            .join(support::CONTROL_DIR)
            .join("index/source-search.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap()
}

fn prepare_with_content_query(client: &mut McpClient, kind: SourceKind, path: &str, hash: &str) {
    for attempt in 0..3 {
        let response = client.call_value(
            10 + attempt,
            "search_sources",
            json!({"query":kind.query(), "path_prefix":path, "limit":20}),
        );
        if response["hits"].as_array().unwrap().iter().any(|hit| {
            hit["path"] == path
                && hit["content_sha256"] == hash
                && hit["authority"] == "original_source"
        }) {
            return;
        }
        assert!(
            attempt < 2
                && matches!(
                    response["reconciliation"].as_str(),
                    Some("deferred_budget" | "deferred_concurrent_update")
                ),
            "content query did not prepare {kind:?} {path}: {response}"
        );
    }
    unreachable!()
}

fn call_exact_source(
    client: &mut McpClient,
    id: u64,
    request: Value,
    kind: SourceKind,
    path: &str,
    hash: &str,
) -> Value {
    for attempt in 0..3 {
        let response = client.call_value(id + attempt, "sources", request.clone());
        if !matches!(
            response["preparation"].as_str(),
            Some("deferred_budget" | "deferred_concurrent_update")
        ) {
            return response;
        }
        if !response["original"].is_null() {
            assert_eq!(response["original"]["content_sha256"], hash, "{response}");
            assert_eq!(response["file"]["path"], path, "{response}");
            match kind {
                SourceKind::Text => assert_eq!(
                    response["original"]["text"], "Cobalt readers preserve the selected boundary.",
                    "{response}"
                ),
                SourceKind::Pdf => {
                    assert_eq!(
                        response["original"]["encoding"], "original_page_reference",
                        "{response}"
                    );
                    assert!(response["original"].get("bytes").is_none(), "{response}");
                }
            }
        }
        assert!(
            attempt < 2,
            "exact preparation remained deferred for {kind:?} {path}: {response}"
        );
    }
    unreachable!()
}

fn assert_exact_source(response: &Value, kind: SourceKind, path: &str, hash: &str, action: &str) {
    let diagnostic = format!("{kind:?} {path} {action}: {response}");
    let (anchor_kind, anchor_start) = kind.anchor();
    assert_eq!(response["schema_version"], 1, "{diagnostic}");
    assert_eq!(response["active"], true, "{diagnostic}");
    assert_eq!(response["action"], action, "{diagnostic}");
    assert_eq!(response["file"]["path"], path, "{diagnostic}");
    assert_eq!(response["file"]["content_sha256"], hash, "{diagnostic}");
    assert_eq!(
        response["file"]["source_kind"],
        match kind {
            SourceKind::Text => "text",
            SourceKind::Pdf => "pdf",
        },
        "{diagnostic}"
    );
    assert_eq!(response["coverage"]["status"], "complete", "{diagnostic}");
    assert!(response["coverage"]["last_error"].is_null(), "{diagnostic}");
    assert_eq!(response["anchor"]["kind"], anchor_kind, "{diagnostic}");
    assert_eq!(
        response["anchor"]["physical_start"], anchor_start,
        "{diagnostic}"
    );
    assert_eq!(
        response["anchor"]["physical_end"], anchor_start,
        "{diagnostic}"
    );
    assert_eq!(
        response["authority"]["type"], "original_source",
        "{diagnostic}"
    );
    if action == "status" {
        assert!(response.get("excerpt").is_none(), "{diagnostic}");
        assert!(response.get("original").is_none(), "{diagnostic}");
        return;
    }
    assert_eq!(response["original"]["content_sha256"], hash, "{diagnostic}");
    match kind {
        SourceKind::Text => {
            assert_eq!(response["original"]["encoding"], "utf8", "{diagnostic}");
            assert_eq!(
                response["original"]["text"], "Cobalt readers preserve the selected boundary.",
                "{diagnostic}"
            );
            assert!(
                response["excerpt"]
                    .as_str()
                    .unwrap()
                    .contains("Cobalt readers preserve the selected boundary."),
                "{diagnostic}"
            );
        }
        SourceKind::Pdf => {
            assert_eq!(
                response["original"]["encoding"], "original_page_reference",
                "{diagnostic}"
            );
            assert_eq!(response["original"]["physical_page"], 2, "{diagnostic}");
            assert_eq!(
                response["original"]["file_size"],
                kind.bytes().len(),
                "{diagnostic}"
            );
            assert!(
                response["excerpt"]
                    .as_str()
                    .unwrap()
                    .contains("A bound that the text layer cannot verify"),
                "{diagnostic}"
            );
            assert!(response["original"].get("bytes").is_none(), "{diagnostic}");
        }
    }
}

fn check_filename_matrix(kind: SourceKind, warm: bool) {
    let mut first_call_rows = 0;
    for path in filename_cases(kind.extension()) {
        let requests = ["inspect", "status"]
            .into_iter()
            .flat_map(|action| [false, true].map(|bound| (action, bound)))
            .collect::<Vec<_>>();
        let batches = if warm {
            vec![requests]
        } else {
            requests.into_iter().map(|request| vec![request]).collect()
        };
        for requests in batches {
            let environment = TestEnvironment::from_fixture("basic").unwrap();
            initialize_product(&environment);
            let target = environment.workspace.join(&path);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, kind.bytes()).unwrap();
            let hash = support::sources::fixture_hash(kind.bytes());
            let mut client = McpClient::spawn(&environment, true);
            client.initialize();
            if warm {
                prepare_with_content_query(&mut client, kind, &path, &hash);
            }
            let before =
                DatabaseSnapshot::capture(&store_path(&environment), CURRENT_CANONICAL_TABLES)
                    .unwrap();
            let (anchor_kind, anchor_start) = kind.anchor();
            for (position, (action, bound)) in requests.into_iter().enumerate() {
                let mut request = json!({"action":action, "path":path,
                    "anchor_kind":anchor_kind, "anchor_start":anchor_start, "anchor_end":anchor_start});
                if bound {
                    request["content_sha256"] = json!(hash);
                }
                let id = 100 + position as u64 * 3;
                // A cold, hash-bound inspect of this one filename must succeed
                // on its first call; the rest of the matrix tolerates deferred
                // preparation because it is load-sensitive.
                let succeeds_on_first_call = !warm
                    && matches!(kind, SourceKind::Text)
                    && path == "lesson\"draft.txt"
                    && action == "inspect"
                    && bound;
                let response = if succeeds_on_first_call {
                    first_call_rows += 1;
                    client.call_value(id, "sources", request)
                } else {
                    call_exact_source(&mut client, id, request, kind, &path, &hash)
                };
                assert_exact_source(&response, kind, &path, &hash, action);
                assert_eq!(
                    before,
                    DatabaseSnapshot::capture(&store_path(&environment), CURRENT_CANONICAL_TABLES)
                        .unwrap(),
                    "{kind:?} {path} {action} warm={warm} bound={bound}"
                );
            }
            client.close_cleanly();
        }
    }
    let expected_first_call_rows = usize::from(!warm && matches!(kind, SourceKind::Text));
    assert_eq!(
        first_call_rows, expected_first_call_rows,
        "the first-call quote filename case must stay in filename_cases"
    );
}

#[test]
fn cold_text_filename_matrix_prepares_each_exact_action_and_binding() {
    let _source_capacity = support::sources::source_test_guard();
    check_filename_matrix(SourceKind::Text, false);
}

#[test]
fn warm_text_filename_matrix_uses_content_query_preparation() {
    let _source_capacity = support::sources::source_test_guard();
    check_filename_matrix(SourceKind::Text, true);
}

#[test]
fn cold_pdf_filename_matrix_prepares_each_exact_action_and_binding() {
    let _source_capacity = support::sources::source_test_guard();
    check_filename_matrix(SourceKind::Pdf, false);
}

#[test]
fn warm_pdf_filename_matrix_uses_content_query_preparation() {
    let _source_capacity = support::sources::source_test_guard();
    check_filename_matrix(SourceKind::Pdf, true);
}

#[test]
fn exact_sources_reject_wrong_hash_and_same_size_changed_bytes() {
    let _source_capacity = support::sources::source_test_guard();
    for kind in [SourceKind::Text, SourceKind::Pdf] {
        let environment = TestEnvironment::from_fixture("basic").unwrap();
        initialize_product(&environment);
        let path = format!("learner's (étude).{}", kind.extension());
        let target = environment.workspace.join(&path);
        fs::write(&target, kind.bytes()).unwrap();
        let hash = support::sources::fixture_hash(kind.bytes());
        let mut client = McpClient::spawn(&environment, true);
        client.initialize();
        prepare_with_content_query(&mut client, kind, &path, &hash);
        let (anchor_kind, anchor_start) = kind.anchor();
        let before =
            DatabaseSnapshot::capture(&store_path(&environment), CURRENT_CANONICAL_TABLES).unwrap();
        for (index, action) in ["inspect", "status"].into_iter().enumerate() {
            let response = client.call_value(
                100 + index as u64,
                "sources",
                json!({"action":action, "path":path, "content_sha256":"0".repeat(64),
                    "anchor_kind":anchor_kind, "anchor_start":anchor_start, "anchor_end":anchor_start}),
            );
            assert_eq!(
                response["coverage"]["status"], "stale",
                "wrong hash {kind:?} {action}: {response}"
            );
            assert_eq!(response["current_content_sha256"], hash, "{response}");
            assert!(
                response.get("excerpt").is_none() && response.get("original").is_none(),
                "{response}"
            );
        }
        let mut changed = kind.bytes().to_vec();
        match kind {
            SourceKind::Text => changed[..6].copy_from_slice(b"Indigo"),
            SourceKind::Pdf => {
                assert!(changed.starts_with(b"%PDF-1.4"));
                changed[7] = b'5';
            }
        }
        assert_eq!(changed.len(), kind.bytes().len());
        let changed_hash = support::sources::fixture_hash(&changed);
        assert_ne!(changed_hash, hash);
        fs::write(&target, changed).unwrap();
        for (index, action) in ["inspect", "status"].into_iter().enumerate() {
            let response = client.call_value(
                200 + index as u64,
                "sources",
                json!({"action":action, "path":path, "content_sha256":hash,
                    "anchor_kind":anchor_kind, "anchor_start":anchor_start, "anchor_end":anchor_start}),
            );
            assert_eq!(
                response["coverage"]["status"], "stale",
                "changed bytes {kind:?} {action}: {response}"
            );
            assert_eq!(
                response["current_content_sha256"], changed_hash,
                "{response}"
            );
            assert!(
                response.get("excerpt").is_none() && response.get("original").is_none(),
                "{response}"
            );
        }
        assert_eq!(
            before,
            DatabaseSnapshot::capture(&store_path(&environment), CURRENT_CANONICAL_TABLES).unwrap()
        );
        client.close_cleanly();
    }
}

#[test]
fn cold_exact_source_does_not_discover_or_extract_unrelated_files() {
    let _source_capacity = support::sources::source_test_guard();
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    initialize_product(&environment);
    let kind = SourceKind::Text;
    fs::write(environment.workspace.join("selected.txt"), kind.bytes()).unwrap();
    fs::write(
        environment.workspace.join("unrelated.txt"),
        "unrelated extraction sentinel\n",
    )
    .unwrap();
    fs::write(
        environment.workspace.join("unrelated.pdf"),
        b"%PDF-1.4 malformed unrelated input",
    )
    .unwrap();
    let hash = support::sources::fixture_hash(kind.bytes());
    let mut client = McpClient::spawn(&environment, true);
    client.initialize();
    let response = client.call_value(100, "sources", json!({"action":"inspect", "path":"selected.txt", "content_sha256":hash, "anchor_start":1, "anchor_end":1}));
    assert_exact_source(&response, kind, "selected.txt", &hash, "inspect");
    let index = source_index(&environment);
    let unrelated: i64 = index.query_row(
        "SELECT count(*) FROM source_manifest WHERE display_path IN ('unrelated.txt', 'unrelated.pdf')",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(unrelated, 0, "selected access prepared an unrelated source");
    assert_eq!(
        fs::read_to_string(environment.workspace.join("unrelated.txt")).unwrap(),
        "unrelated extraction sentinel\n"
    );
    client.close_cleanly();
}

#[test]
fn unknown_missing_exact_path_does_not_create_a_manifest_row() {
    let _source_capacity = support::sources::source_test_guard();
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    initialize_product(&environment);
    let mut client = McpClient::spawn(&environment, true);
    client.initialize();
    let before =
        DatabaseSnapshot::capture(&store_path(&environment), CURRENT_CANONICAL_TABLES).unwrap();
    for (position, action) in ["inspect", "status"].into_iter().enumerate() {
        let response = client.call_value(
            100 + position as u64,
            "sources",
            json!({"action":action, "path":"never-existed.txt", "anchor_start":1, "anchor_end":1}),
        );
        assert_eq!(response["coverage"]["status"], "unknown", "{response}");
        assert!(
            response.get("excerpt").is_none() && response.get("original").is_none(),
            "{response}"
        );
        let count: i64 = source_index(&environment)
            .query_row(
                "SELECT count(*) FROM source_manifest WHERE display_path = 'never-existed.txt'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            count, 0,
            "unknown missing path fabricated canonical source state"
        );
    }
    assert_eq!(
        before,
        DatabaseSnapshot::capture(&store_path(&environment), CURRENT_CANONICAL_TABLES).unwrap()
    );
    client.close_cleanly();
}

#[test]
fn recording_off_exact_source_reads_preserve_canonical_learner_state() {
    let _source_capacity = support::sources::source_test_guard();
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    initialize_product(&environment);
    support::v2::start(&environment, "codex", "exact-source-recording-off");
    let controlled = support::v2::tool(
        &environment,
        "codex",
        "exact-source-recording-off",
        "learning_step",
        json!({"schema":"learner-model/v2", "action":"control", "request_id":"privacy", "control":"recording_off", "reported_event":{"external_event_key":"privacy-control", "speaker":"learner", "kind":"message", "text":"Turn recording off."}}),
    );
    assert!(controlled.get("error").is_none(), "{controlled}");
    let recording: i64 = Connection::open_with_flags(store_path(&environment), OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap().query_row(
        "SELECT recording_enabled FROM host_bindings WHERE host_kind = 'codex' AND host_thread_id = 'exact-source-recording-off'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        recording, 0,
        "recording_off must be in effect before source reads"
    );
    let kind = SourceKind::Text;
    let path = "learner's (étude).txt";
    fs::write(environment.workspace.join(path), kind.bytes()).unwrap();
    let hash = support::sources::fixture_hash(kind.bytes());
    let before =
        DatabaseSnapshot::capture(&store_path(&environment), CURRENT_CANONICAL_TABLES).unwrap();
    let mut client = McpClient::spawn(&environment, true);
    client.initialize();
    for (position, action) in ["inspect", "status"].into_iter().enumerate() {
        let response = client.call_value(100 + position as u64, "sources", json!({"action":action, "path":path, "content_sha256":hash, "anchor_start":1, "anchor_end":1}));
        assert_exact_source(&response, kind, path, &hash, action);
    }
    assert_eq!(
        before,
        DatabaseSnapshot::capture(&store_path(&environment), CURRENT_CANONICAL_TABLES).unwrap()
    );
    client.close_cleanly();
}

fn scan_cursor(environment: &TestEnvironment) -> Option<(i64, String)> {
    source_index(environment).query_row(
        "SELECT scan_generation, cursor_json FROM source_reconciliation_state WHERE singleton = 1",
        [], |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional().unwrap()
}

fn unrelated_manifest(environment: &TestEnvironment) -> Vec<Value> {
    let index = source_index(environment);
    let mut statement = index.prepare(
        "SELECT display_path, source_revision, present, index_status, content_sha256, observed_generation
         FROM source_manifest WHERE display_path <> 'selected.txt' ORDER BY display_path",
    ).unwrap();
    statement
        .query_map([], |row| {
            Ok(json!([
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, i64>(5)?,
            ]))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

#[test]
fn exact_preparation_preserves_unrestricted_cursor_and_unrelated_deletion() {
    let _source_capacity = support::sources::source_test_guard();
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    initialize_product(&environment);
    let mut client = McpClient::spawn(&environment, true);
    client.initialize();
    let kind = SourceKind::Text;
    fs::write(environment.workspace.join("keeper.txt"), kind.bytes()).unwrap();
    let hash = support::sources::fixture_hash(kind.bytes());
    prepare_with_content_query(&mut client, kind, "keeper.txt", &hash);
    fs::remove_file(environment.workspace.join("keeper.txt")).unwrap();
    let corpus = environment.workspace.join("corpus");
    fs::create_dir(&corpus).unwrap();
    for index in 0..300 {
        fs::write(
            corpus.join(format!("metadata-{index:03}.txt")),
            "metadata cursor fixture\n",
        )
        .unwrap();
    }
    let discovery = client.call_value(
        20,
        "search_sources",
        json!({"query":"metadata cursor fixture", "limit":5}),
    );
    assert!(discovery["hits"].is_array(), "{discovery}");
    let cursor = scan_cursor(&environment).expect("unrestricted scan must remain active");
    let manifest_before = unrelated_manifest(&environment);
    assert!(
        manifest_before
            .iter()
            .any(|row| row[0] == "keeper.txt" && row[2] == 1 && row[3] == "indexed"),
        "{manifest_before:?}"
    );
    fs::write(environment.workspace.join("selected.txt"), kind.bytes()).unwrap();
    let response = client.call_value(100, "sources", json!({"action":"inspect", "path":"selected.txt", "content_sha256":hash, "anchor_start":1, "anchor_end":1}));
    assert_exact_source(&response, kind, "selected.txt", &hash, "inspect");
    assert_eq!(
        scan_cursor(&environment),
        Some(cursor),
        "selected preparation changed the unrestricted scan cursor"
    );
    assert_eq!(
        unrelated_manifest(&environment),
        manifest_before,
        "selected preparation changed unrelated manifest state"
    );
    let mut complete = false;
    for attempt in 0..8 {
        let response = client.call_value(
            200 + attempt,
            "search_sources",
            json!({"query":"metadata cursor fixture", "limit":5}),
        );
        if scan_cursor(&environment).is_none() {
            complete = true;
            break;
        }
        assert!(response["hits"].is_array(), "{response}");
    }
    assert!(
        complete,
        "unrestricted scan did not complete within eight explicit deferred-budget continuations"
    );
    let keeper: (i64, String) = source_index(&environment)
        .query_row(
            "SELECT present, index_status FROM source_manifest WHERE display_path = 'keeper.txt'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(keeper, (0, "deleted".to_owned()));
    let selected: (i64, String, String) = source_index(&environment).query_row(
        "SELECT present, index_status, content_sha256 FROM source_manifest WHERE display_path = 'selected.txt'",
        [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).unwrap();
    assert_eq!(
        selected,
        (1, "indexed".to_owned(), hash),
        "unrestricted scan completion erased the explicitly prepared source"
    );
    client.close_cleanly();
}
