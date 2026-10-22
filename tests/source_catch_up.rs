//! A search indexes only what fits its own budget. After it returns, the MCP
//! server keeps indexing the study folder in the background, and stops that
//! work, including any PDF extractor, when the host closes the connection.

use std::fs;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use rusqlite::{Connection, OpenFlags};
use serde_json::json;

use support::TestEnvironment;
use support::mcp::McpClient;
use support::sources::source_test_guard;
use support::v2::init;

const LESSONS: i64 = 200;

#[test]
fn one_search_leads_to_a_fully_indexed_folder() {
    let _guard = source_test_guard();
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    init(&environment);
    let notes = environment.workspace.join("notes");
    fs::create_dir_all(&notes).unwrap();
    for index in 0..LESSONS {
        fs::write(
            notes.join(format!("lesson-{index:03}.md")),
            format!("# Lesson {index}\n\nSpaced retrieval note {index}.\n"),
        )
        .unwrap();
    }
    let mut client = McpClient::spawn(&environment, true);
    client.initialize();
    let first = client.call_value(2, "search_sources", json!({"query": "spaced retrieval"}));
    assert_ne!(
        first["coverage"], "complete",
        "one search indexed the whole folder, so this proves nothing: {first}"
    );

    let index = environment
        .workspace
        .join(support::CONTROL_DIR)
        .join("index/source-search.sqlite");
    let deadline = Instant::now() + Duration::from_secs(30);
    while lesson_progress(&index) != Some((LESSONS, false)) {
        assert!(
            Instant::now() < deadline,
            "without more searches, (indexed lessons, scan in progress) stayed {:?}",
            lesson_progress(&index)
        );
        thread::sleep(Duration::from_millis(50));
    }
    let next = client.call_value(3, "search_sources", json!({"query": "spaced retrieval"}));
    assert_eq!(next["coverage"], "complete", "{next}");
    client.close_cleanly();
}

fn lesson_progress(index: &Path) -> Option<(i64, bool)> {
    let connection = Connection::open_with_flags(
        index,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    connection
        .query_row(
            "SELECT
                 (SELECT count(*) FROM source_manifest
                  WHERE display_path LIKE 'notes/lesson-%' AND present = 1
                    AND index_status = 'indexed'),
                 EXISTS(SELECT 1 FROM source_reconciliation_state)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok()
}

#[cfg(unix)]
#[test]
fn closing_the_server_during_catch_up_stops_its_pdf_extractor() {
    let _guard = source_test_guard();
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    init(&environment);
    fs::create_dir_all(environment.workspace.join("slides")).unwrap();
    fs::write(
        environment.workspace.join("slides/textbook.pdf"),
        long_pdf(),
    )
    .unwrap();
    let mut client = McpClient::spawn(&environment, true);
    client.initialize();
    client.call_value(2, "search_sources", json!({"query": "textbook"}));

    let server = client.child.id();
    let deadline = Instant::now() + Duration::from_secs(30);
    let extractor = loop {
        if let Some(pid) = pdf_extractors(server).first() {
            break *pid;
        }
        assert!(
            Instant::now() < deadline,
            "catch-up never started a PDF extractor"
        );
        thread::sleep(Duration::from_millis(5));
    };
    let McpClient {
        mut child,
        stdin,
        stdout,
    } = client;
    assert!(
        alive(extractor),
        "the extractor finished before the server was closed"
    );
    drop(stdin);
    let closed = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if closed.elapsed() >= Duration::from_secs(1) {
            child.kill().unwrap();
            panic!("the server did not exit within a second of its input closing");
        }
        thread::sleep(Duration::from_millis(5));
    };
    drop(stdout);
    assert!(status.success(), "{status}");
    assert!(!alive(extractor), "the PDF extractor outlived the server");
}

#[cfg(unix)]
fn pdf_extractors(server: u32) -> Vec<u32> {
    let output = std::process::Command::new("pgrep")
        .args(["-P", &server.to_string(), "-f", "_internal extract-pdf"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .filter_map(|pid| pid.parse().ok())
        .collect()
}

#[cfg(unix)]
fn alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success()
}

/// Close to the page limit, so extraction takes long enough that the
/// extractor is still running when the test closes the server.
#[cfg(unix)]
fn long_pdf() -> Vec<u8> {
    use pdf_extract::{Document, Object, Stream, content::Content, content::Operation, dictionary};

    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let font_id = document
        .add_object(dictionary! {"Type"=>"Font", "Subtype"=>"Type1", "BaseFont"=>"Helvetica"});
    let resources_id = document.add_object(dictionary! {"Font"=>dictionary! {"F1"=>font_id}});
    let pages = (0..2_000)
        .map(|page| {
            let mut operations = vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), 10.into()]),
                Operation::new("TL", vec![12.into()]),
                Operation::new("Td", vec![40.into(), 760.into()]),
            ];
            operations.extend((0..45).map(|line| {
                Operation::new(
                    "'",
                    vec![Object::string_literal(format!(
                        "page {page} line {line} recursion base case induction proof lemma"
                    ))],
                )
            }));
            operations.push(Operation::new("ET", vec![]));
            let content = Content { operations }.encode().unwrap();
            let content_id = document.add_object(Stream::new(dictionary! {}, content));
            Object::Reference(document.add_object(dictionary! {
                "Type"=>"Page", "Parent"=>pages_id, "Contents"=>content_id,
                "Resources"=>resources_id, "MediaBox"=>vec![0.into(),0.into(),612.into(),792.into()],
            }))
        })
        .collect::<Vec<_>>();
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {"Type"=>"Pages", "Kids"=>pages, "Count"=>2_000}),
    );
    let catalog_id = document.add_object(dictionary! {"Type"=>"Catalog", "Pages"=>pages_id});
    document.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).unwrap();
    bytes
}
