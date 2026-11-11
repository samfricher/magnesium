//! Make a source file searchable before a test reads it.
//!
//! A search indexes only what fits its time budget, so a new file can take
//! several searches to appear. These helpers repeat the search until the
//! file is indexed and a hit names it. The test then makes its own request
//! once, so its own assertions never depend on a retry.

use std::time::{Duration, Instant};

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{TestEnvironment, mcp::McpClient};

/// Long enough for a loaded machine. A file is usually ready within two searches.
const PREPARATION_DEADLINE: Duration = Duration::from_secs(30);

/// Hold this for the whole of a test that searches or reads sources. The
/// runtime gives each search 150 ms, so these tests run one at a time: side
/// by side on a busy machine they run out of that time and fail.
pub fn source_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn fixture_hash(bytes: impl AsRef<[u8]>) -> String {
    Sha256::digest(bytes.as_ref())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn prepare_text_fixture(
    environment: &TestEnvironment,
    client: &mut McpClient,
    first_id: u64,
    path: &str,
    bytes: impl AsRef<[u8]>,
) {
    prepare_source_fixture(environment, client, first_id, path, &fixture_hash(bytes));
}

pub fn prepare_pdf_fixture(
    environment: &TestEnvironment,
    client: &mut McpClient,
    first_id: u64,
    path: &str,
    hash: &str,
) {
    prepare_source_fixture(environment, client, first_id, path, hash);
}

/// `prepare_text_fixture` through a server of its own, for a test that has none.
pub fn prepare_text_fixture_separately(
    environment: &TestEnvironment,
    path: &str,
    bytes: impl AsRef<[u8]>,
) {
    let mut client = McpClient::spawn(environment, true);
    client.initialize();
    prepare_text_fixture(environment, &mut client, 1000, path, bytes);
    client.close_cleanly();
}

fn prepare_source_fixture(
    environment: &TestEnvironment,
    client: &mut McpClient,
    first_id: u64,
    path: &str,
    hash: &str,
) {
    let deadline = Instant::now() + PREPARATION_DEADLINE;
    for attempt in 0.. {
        let response = client.request(
            first_id + attempt,
            "tools/call",
            json!({"name":"search_sources", "arguments":{"query":path, "path_prefix":path, "limit":50}}),
        );
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("search for {path} returned no text: {response}"));
        if response["result"]["isError"] == true {
            // The server asks for the request again when background indexing
            // published a newer state of the file first.
            assert!(
                text.contains("source_superseded"),
                "search for {path} failed: {response}"
            );
        } else {
            let packet: Value = serde_json::from_str(text)
                .unwrap_or_else(|error| panic!("search for {path} is not JSON ({error}): {text}"));
            let hits = packet["hits"]
                .as_array()
                .unwrap_or_else(|| panic!("search for {path} has no hits list: {packet}"));
            for hit in hits {
                assert_eq!(hit["path"], path, "{packet}");
                assert_eq!(hit["content_sha256"], hash, "{packet}");
                assert_eq!(hit["authority"], "original_source", "{packet}");
            }
            if !hits.is_empty() && index_status(environment, path).as_deref() == Some("indexed") {
                return;
            }
        }
        assert!(
            Instant::now() < deadline,
            "{path} was not indexed in time; last response: {response}"
        );
    }
}

/// The file's row in the source index, read without changing it.
fn index_status(environment: &TestEnvironment, path: &str) -> Option<String> {
    Connection::open_with_flags(
        environment
            .workspace
            .join(super::CONTROL_DIR)
            .join("index/source-search.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .and_then(|connection| {
        connection
            .query_row(
                "SELECT index_status FROM source_manifest WHERE display_path = ?1 AND present = 1",
                [path],
                |row| row.get(0),
            )
            .optional()
    })
    .unwrap_or_else(|error| panic!("read the source index row of {path}: {error}"))
}
