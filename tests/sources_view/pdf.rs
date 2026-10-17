use std::fs;

use pdf_extract::{Document, Object, Stream, content::Content, content::Operation, dictionary};
#[cfg(unix)]
use serde_json::Value;
use serde_json::json;
use sha2::{Digest, Sha256};

use support::TestEnvironment;
use support::mcp::McpClient;
use support::snapshot::DatabaseSnapshot;

use super::{
    CURRENT_CANONICAL_TABLES, assert_no_delivery_claim, assert_range_issue, initialize_product,
    sources_error, store_path,
};

#[test]
fn malformed_pdf_reports_text_unavailable_with_original_option() {
    let _source_capacity = support::sources::source_test_guard();
    let environment = TestEnvironment::from_fixture("basic").expect("env");
    initialize_product(&environment);
    fs::write(
        environment.workspace.join("doc.pdf"),
        b"%PDF-1.4 malformed but nominally pdf-suffixed bytes that no parser accepts (((((\x00\x01\x02",
    )
    .expect("write malformed pdf");
    let mut client = McpClient::spawn(&environment, true);
    client.initialize();

    // Reconciliation/extraction of the malformed file may itself report a
    // tool error; that outcome is fine — the assertion target is `sources`.
    let _ = client.request(
        100,
        "tools/call",
        json!({"name": "search_sources", "arguments": {"query": "doc.pdf", "limit": 4}}),
    );
    let inspected = client.call_value(
        101,
        "sources",
        json!({"action": "inspect", "path": "doc.pdf", "anchor_kind": "page", "anchor_start": 1, "anchor_end": 1}),
    );
    assert_eq!(inspected["file"]["source_kind"], "pdf");
    assert_ne!(inspected["coverage"]["status"], "complete", "{inspected}");
    assert!(
        ["text_unavailable", "partial", "unknown"]
            .contains(&inspected["coverage"]["status"].as_str().unwrap_or("")),
        "{inspected}"
    );
    assert_eq!(inspected["original"]["encoding"], "original_page_reference");
    assert_eq!(inspected["original"]["physical_page"], 1);
    assert_eq!(inspected["basis"]["visual_analysis"], false);
    assert_eq!(inspected["extraction"]["text_layer_only"], true);
    let serialized = serde_json::to_string(&inspected).expect("serialize");
    assert!(!serialized.contains("no relevant content"), "{inspected}");

    // External citations are labelled, never fetched, and carry no bytes.
    let external = client.call_value(
        102,
        "sources",
        json!({"action": "status", "path": "https://example.com/paper.pdf"}),
    );
    assert_eq!(
        external["authority"]["type"], "external_citation",
        "{external}"
    );
    assert!(external.get("original").is_none(), "{external}");
    assert_no_delivery_claim(&external);
    assert!(
        external["snapshot_guidance"]
            .as_str()
            .unwrap()
            .contains("explicitly authorize"),
        "{external}"
    );

    // Viewing used no learner-state path at all.
    client.close_cleanly();
}

#[test]
fn synthetic_math_pdf_inspection_keeps_physical_page_scope() {
    let _source_capacity = support::sources::source_test_guard();
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    initialize_product(&environment);
    let pdf = include_bytes!("../fixtures/interaction/statistics/source-pages.pdf");
    fs::write(environment.workspace.join("math.pdf"), pdf).unwrap();
    let hash = Sha256::digest(pdf)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut client = McpClient::spawn(&environment, true);
    client.initialize();
    let preparation =
        support::sources::prepare_pdf_fixture(&environment, &mut client, 900, "math.pdf", &hash);
    let inspected = client.call_value(950, "sources", json!({"action":"inspect", "path":"math.pdf", "content_sha256":hash, "anchor_kind":"page", "anchor_start":2, "anchor_end":2}));
    assert_eq!(inspected["anchor"]["physical_start"], 2, "{inspected}");
    assert_eq!(inspected["anchor"]["physical_end"], 2);
    let excerpt = inspected["excerpt"].as_str().unwrap();
    assert!(
        excerpt.contains("bound"),
        "{inspected}; preparation: {preparation:?}"
    );
    assert!(
        excerpt.contains("A bound that the text layer cannot verify"),
        "{inspected}"
    );
    assert!(!excerpt.contains("proof-first outline"));
    assert!(!excerpt.contains("pentagon"));
    assert_eq!(
        inspected["file"]["content_sha256"].as_str().unwrap().len(),
        64
    );
    assert_eq!(inspected["file"]["content_sha256"], hash);
    assert_eq!(inspected["original"]["content_sha256"], hash);
    assert_eq!(inspected["original"]["physical_page"], 2);
    assert_eq!(inspected["coverage"]["status"], "complete", "{inspected}");
    assert!(inspected.to_string().len() < 32 * 1024);
    let path_only = client.call_value(
        951,
        "sources",
        json!({"action":"inspect", "path":"math.pdf"}),
    );
    assert_eq!(path_only["anchor"]["kind"], "page", "{path_only}");
    assert_eq!(path_only["coverage"]["status"], "complete", "{path_only}");
    assert!(path_only["anchor"]["physical_end"].is_null(), "{path_only}");
    client.close_cleanly();
}

fn pdf_pages(texts: &[&str], padding: usize) -> Vec<u8> {
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let font_id = document
        .add_object(dictionary! {"Type"=>"Font", "Subtype"=>"Type1", "BaseFont"=>"Helvetica"});
    let resources_id = document.add_object(dictionary! {"Font"=>dictionary! {"F1"=>font_id}});
    let mut pages = Vec::new();
    for text in texts {
        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), 18.into()]),
                Operation::new("Td", vec![72.into(), 720.into()]),
                Operation::new("Tj", vec![Object::string_literal(*text)]),
                Operation::new("ET", vec![]),
            ],
        };
        let content_id =
            document.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        pages.push(Object::Reference(document.add_object(dictionary! {
            "Type"=>"Page", "Parent"=>pages_id, "Contents"=>content_id,
            "Resources"=>resources_id, "MediaBox"=>vec![0.into(),0.into(),612.into(),792.into()],
        })));
    }
    document.objects.insert(
        pages_id,
        Object::Dictionary(
            dictionary! {"Type"=>"Pages", "Kids"=>pages, "Count"=>texts.len() as i64},
        ),
    );
    let catalog_id = document.add_object(dictionary! {"Type"=>"Catalog", "Pages"=>pages_id});
    document.trailer.set("Root", catalog_id);
    if padding > 0 {
        document.add_object(Stream::new(dictionary! {}, vec![b'x'; padding]));
    }
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).unwrap();
    bytes
}

#[test]
fn native_pdf_defaults_to_pages_and_distinguishes_blank_from_missing_text() {
    let _source_capacity = support::sources::source_test_guard();
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    initialize_product(&environment);
    let pdf = pdf_pages(&["First adjacent page", "", "Third adjacent page"], 0);
    fs::write(environment.workspace.join("blank-middle.pdf"), &pdf).unwrap();
    let hash = support::sources::fixture_hash(&pdf);
    let mut client = McpClient::spawn(&environment, true);
    client.initialize();
    support::sources::prepare_pdf_fixture(
        &environment,
        &mut client,
        1000,
        "blank-middle.pdf",
        &hash,
    );
    let response = client.call_value(
        100,
        "sources",
        json!({"action":"inspect", "path":"blank-middle.pdf"}),
    );
    assert_eq!(response["anchor"]["kind"], "page", "{response}");
    assert!(response["anchor"]["physical_end"].is_null(), "{response}");
    assert_eq!(response["coverage"]["status"], "partial", "{response}");
    assert_range_issue(&response, "page", 2, 2, "unreadable_extraction");
    assert!(
        !response["coverage"]["range_issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["reason"] == "missing_extraction"),
        "{response}"
    );
    assert_eq!(response["original"]["encoding"], "original_page_reference");
    assert_eq!(response["original"]["content_sha256"], hash);
    assert_no_delivery_claim(&response);
    let blank = client.call_value(
        101,
        "sources",
        json!({"action":"inspect", "path":"blank-middle.pdf", "anchor_start":2, "anchor_end":2}),
    );
    assert_eq!(blank["coverage"]["status"], "text_unavailable", "{blank}");
    assert_range_issue(&blank, "page", 2, 2, "unreadable_extraction");
    assert_eq!(blank["original"]["physical_page"], 2, "{blank}");
    let outside = client.call_value(
        102,
        "sources",
        json!({"action":"inspect", "path":"blank-middle.pdf", "anchor_start":4, "anchor_end":4}),
    );
    assert_eq!(outside["coverage"]["status"], "partial", "{outside}");
    assert_range_issue(&outside, "page", 4, 4, "outside_known_extent");
    assert!(outside["original"].is_null(), "{outside}");
    let mismatch = sources_error(
        &mut client,
        103,
        json!({"action":"inspect", "path":"blank-middle.pdf", "anchor_kind":"line", "anchor_start":1, "anchor_end":1}),
    );
    assert!(
        mismatch.contains("kind") && mismatch.contains("pdf"),
        "{mismatch}"
    );
    client.close_cleanly();
}

#[test]
fn deferred_reconciliation_cannot_certify_changed_or_deleted_pdf_status_or_extent() {
    let _source_capacity = support::sources::source_test_guard();
    for deleted in [false, true] {
        let environment = TestEnvironment::from_fixture("basic").unwrap();
        initialize_product(&environment);
        let pdf = pdf_pages(&["Alpha original page", "Beta original page"], 0);
        let path = environment.workspace.join("source.pdf");
        fs::write(&path, &pdf).unwrap();
        let hash = support::sources::fixture_hash(&pdf);
        let mut client = McpClient::spawn(&environment, true);
        client.initialize();
        support::sources::prepare_pdf_fixture(&environment, &mut client, 1000, "source.pdf", &hash);
        let before = client.call_value(
            100,
            "sources",
            json!({"action":"status", "path":"source.pdf"}),
        );
        assert_eq!(before["coverage"]["status"], "complete", "{before}");
        assert_eq!(before["file"]["content_sha256"], hash, "{before}");
        let learner_before =
            DatabaseSnapshot::capture(&store_path(&environment), CURRENT_CANONICAL_TABLES).unwrap();
        let writer = rusqlite::Connection::open(
            environment
                .workspace
                .join(support::CONTROL_DIR)
                .join("index/source-search.sqlite"),
        )
        .unwrap();
        writer.execute_batch("BEGIN IMMEDIATE").unwrap();
        if deleted {
            fs::remove_file(&path).unwrap();
        } else {
            let mut changed = pdf.clone();
            let offset = changed
                .windows(5)
                .position(|bytes| bytes == b"Alpha")
                .unwrap();
            changed[offset..offset + 5].copy_from_slice(b"Omega");
            assert_eq!(changed.len(), pdf.len());
            assert_ne!(support::sources::fixture_hash(&changed), hash);
            fs::write(&path, changed).unwrap();
        }
        for (index, request) in [
            json!({"action":"status", "path":"source.pdf"}),
            json!({"action":"inspect", "path":"source.pdf", "anchor_start":3, "anchor_end":3}),
        ]
        .into_iter()
        .enumerate()
        {
            let error = sources_error(&mut client, 101 + index as u64, request);
            let cache_failure = error.contains(&format!(
                "{}/index/source-search.sqlite",
                support::CONTROL_DIR
            )) && (error.contains("initialization budget exhausted")
                || error.contains("database is locked"));
            if deleted {
                assert!(
                    cache_failure
                        || (error.contains("inspect source") && error.contains("source.pdf")),
                    "{error}"
                );
            } else {
                assert!(
                    cache_failure || error.contains("source changed after reconciliation"),
                    "{error}"
                );
            }
            assert!(!error.contains("Alpha original page"), "{error}");
            assert!(!error.contains("Omega original page"), "{error}");
            assert!(!error.contains("Beta original page"), "{error}");
        }
        writer.execute_batch("ROLLBACK").unwrap();
        assert_eq!(
            learner_before,
            DatabaseSnapshot::capture(&store_path(&environment), CURRENT_CANONICAL_TABLES).unwrap()
        );
        client.close_cleanly();
    }
}

#[cfg(unix)]
#[test]
fn converter_pdf_holes_and_unknown_tail_come_from_real_publication() {
    let _source_capacity = support::sources::source_test_guard();
    use std::os::unix::fs::PermissionsExt;
    for complete in [false, true] {
        let environment = TestEnvironment::from_fixture("basic").unwrap();
        initialize_product(&environment);
        let pdf = pdf_pages(&["", "", ""], 0);
        fs::write(environment.workspace.join("converted.pdf"), &pdf).unwrap();
        let hash = support::sources::fixture_hash(&pdf);
        let worker = environment.root.join("converter.sh");
        let response_file = environment.root.join("converter-response.bin");
        let input_file = environment.root.join("converter-request.bin");
        let pages = if complete { vec![1, 2, 3] } else { vec![1, 3] };
        let blocks = pages.into_iter().map(|page| json!({"anchor_kind":"page", "anchor_start":page, "anchor_end":page, "text":format!("Converted page {page}")})).collect::<Vec<_>>();
        let body = serde_json::to_vec(&json!({"version":1, "method":"finite-fixture", "method_version":"1", "source_sha256":hash, "blocks":blocks, "coverage":if complete {"complete"} else {"partial"}, "missing":if complete {vec![]} else {vec!["page 2"]}})).unwrap();
        let mut framed = format!("MAGNESIUM_DOCS_V1 {}\n", body.len()).into_bytes();
        framed.extend_from_slice(&body);
        fs::write(&response_file, framed).unwrap();
        fs::write(
            &worker,
            format!(
                "#!/bin/sh\ncat >'{}'\ncat '{}'\n",
                input_file.display(),
                response_file.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&worker, fs::Permissions::from_mode(0o700)).unwrap();
        let worker_path = worker.to_str().unwrap();
        let mut client = McpClient::spawn_with_env(
            &environment,
            true,
            &[
                ("MAGNESIUM_DOCS_WORKER", worker_path),
                ("MAGNESIUM_DOCS_WORKER_OPT_IN", "1"),
            ],
        );
        client.initialize();
        support::sources::prepare_pdf_fixture(
            &environment,
            &mut client,
            1000,
            "converted.pdf",
            &hash,
        );
        let finite = client.call_value(
            100,
            "sources",
            json!({"action":"inspect", "path":"converted.pdf", "anchor_start":1, "anchor_end":3}),
        );
        assert_eq!(
            finite["extraction"]["converter"]["method"], "finite-fixture",
            "{finite}"
        );
        assert_eq!(
            finite["coverage"]["status"],
            if complete { "complete" } else { "partial" },
            "{finite}"
        );
        if !complete {
            assert_range_issue(&finite, "page", 2, 2, "missing_extraction");
        }
        let open = client.call_value(
            101,
            "sources",
            json!({"action":"status", "path":"converted.pdf"}),
        );
        assert_eq!(open["coverage"]["status"], "partial", "{open}");
        assert_eq!(open["coverage"]["unknown_tail"], true, "{open}");
        assert!(
            open.get("excerpt").is_none() && open.get("original").is_none(),
            "{open}"
        );
        assert_no_delivery_claim(&open);
        if !complete {
            assert_range_issue(&open, "page", 2, 2, "missing_extraction");
        }
        let request = fs::read(input_file).unwrap();
        let separator = request.iter().position(|byte| *byte == b'\n').unwrap();
        let header = std::str::from_utf8(&request[..separator]).unwrap();
        let length = header
            .strip_prefix("MAGNESIUM_DOCS_V1 ")
            .unwrap()
            .parse::<usize>()
            .unwrap();
        assert_eq!(request.len() - separator - 1, length);
        let input: Value = serde_json::from_slice(&request[separator + 1..]).unwrap();
        assert_eq!(input["source_sha256"], hash, "{input}");
        client.close_cleanly();
    }
}

#[test]
fn large_pdf_reference_reports_exact_identity_without_bulk_bytes() {
    let _source_capacity = support::sources::source_test_guard();
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    initialize_product(&environment);
    let pdf = pdf_pages(
        &["Large reference retains exact identity"],
        20 * 1024 * 1024,
    );
    assert!(pdf.len() > 20 * 1024 * 1024);
    fs::write(environment.workspace.join("large.pdf"), &pdf).unwrap();
    let hash = support::sources::fixture_hash(&pdf);
    let mut client = McpClient::spawn(&environment, true);
    client.initialize();
    support::sources::prepare_pdf_fixture(&environment, &mut client, 1000, "large.pdf", &hash);
    let response = client.call_value(100, "sources", json!({"action":"inspect", "path":"large.pdf", "content_sha256":hash, "anchor_start":1, "anchor_end":1}));
    assert_eq!(response["coverage"]["status"], "complete", "{response}");
    assert_eq!(response["original"]["encoding"], "original_page_reference");
    assert_eq!(response["original"]["content_sha256"], hash);
    assert_eq!(response["original"]["file_size"], pdf.len(), "{response}");
    assert!(
        response["original"].get("bytes").is_none() && response["original"].get("text").is_none(),
        "{response}"
    );
    assert!(response.to_string().len() < 16 * 1024, "{response}");
    client.close_cleanly();
}
