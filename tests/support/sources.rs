//! Prepare readable source fixtures without treating cold discovery as fidelity.
//!
//! Search exposes a bounded coordinator snapshot, not per-file readiness.
//! The read-only manifest check distinguishes the fixture's pending work from
//! unrelated deferred work, exclusions, changed hashes and extraction failures.
//! Once indexed, callers perform their original single source request.
//! Those positive/negative authorization oracles never retry.

use sha2::{Digest, Sha256};
use std::{
    fs,
    time::{Duration, Instant},
};

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::{Value, json};

use super::{TestEnvironment, mcp::McpClient};

const PREPARATION_DEADLINE: Duration = Duration::from_secs(5);
const PREPARATION_ATTEMPT_CAP: u64 = 32;

pub fn source_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static CAPACITY: std::sync::Mutex<()> = std::sync::Mutex::new(());
    CAPACITY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Clone, Copy, Debug)]
pub enum FixtureKind {
    Text,
    Pdf,
}

impl FixtureKind {
    fn source(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Pdf => "pdf",
        }
    }
    fn anchor(self) -> &'static str {
        match self {
            Self::Text => "line",
            Self::Pdf => "page",
        }
    }
}

pub fn fixture_hash(bytes: impl AsRef<[u8]>) -> String {
    Sha256::digest(bytes.as_ref())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn prepare_source_fixture(
    environment: &TestEnvironment,
    client: &mut McpClient,
    first_id: u64,
    path: &str,
    kind: FixtureKind,
    hash: &str,
) -> Vec<Value> {
    let started = Instant::now();
    prepare_source_fixture_with(
        path,
        kind,
        hash,
        PREPARATION_DEADLINE,
        PREPARATION_ATTEMPT_CAP,
        |attempt, arguments| {
            let response = client.request(
                first_id + attempt * 2,
                "tools/call",
                json!({"name":"search_sources", "arguments":arguments}),
            );
            let state = validate_search_packet(&response, path, kind, hash).and_then(|_| {
                observe_fixture_state(
                    environment,
                    client,
                    first_id + attempt * 2 + 1,
                    path,
                    kind,
                    hash,
                )
            });
            (response, state)
        },
        || started.elapsed(),
    )
    .unwrap_or_else(|error| panic!("{error}"))
}

pub fn prepare_pdf_fixture(
    environment: &TestEnvironment,
    client: &mut McpClient,
    first_id: u64,
    path: &str,
    hash: &str,
) -> Vec<Value> {
    prepare_source_fixture(environment, client, first_id, path, FixtureKind::Pdf, hash)
}

pub fn prepare_text_fixture(
    environment: &TestEnvironment,
    client: &mut McpClient,
    first_id: u64,
    path: &str,
    bytes: impl AsRef<[u8]>,
) -> Vec<Value> {
    prepare_source_fixture(
        environment,
        client,
        first_id,
        path,
        FixtureKind::Text,
        &fixture_hash(bytes),
    )
}

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

/// A deferred discovery snapshot may precede the target's first manifest row.
/// The current public status independently enforces live discovery policy;
/// regular, contained authored bytes corroborate only this undiscovered case.
/// No policy parser or permission grant is recreated by the fixture helper.
pub fn observe_fixture_state(
    environment: &TestEnvironment,
    client: &mut McpClient,
    id: u64,
    path: &str,
    kind: FixtureKind,
    hash: &str,
) -> Result<Option<Value>, String> {
    let before_status = read_fixture_state(environment, path)?;
    let arguments = json!({"action":"status", "path":path, "anchor_kind":kind.anchor(), "anchor_start":1, "anchor_end":1});
    let response = request_status_after_manifest(before_status.as_ref(), path, kind, hash, || {
        client.request(
            id,
            "tools/call",
            json!({"name":"sources", "arguments":arguments}),
        )
    })?;
    let status = decode_packet(&response)?;
    if status["schema_version"] != 1
        || status["active"] != true
        || status["action"] != "status"
        || status["file"]["path"] != path
        || status["anchor"]["kind"] != kind.anchor()
        || status["anchor"]["physical_start"] != 1
        || status["anchor"]["physical_end"] != 1
        || status.get("excerpt").is_some()
        || status.get("original").is_some()
        || !matches!(
            status["coverage"]["status"].as_str(),
            Some("unknown" | "complete" | "partial")
        )
    {
        return Err(format!(
            "fixture status is terminal or malformed: {response}"
        ));
    }
    let state = read_fixture_state(environment, path)?;
    let target = environment.workspace.join(path);
    let root = fs::canonicalize(&environment.workspace).map_err(|error| error.to_string())?;
    if !fs::symlink_metadata(&target)
        .map_err(|error| error.to_string())?
        .file_type()
        .is_file()
        || !fs::canonicalize(&target)
            .map_err(|error| error.to_string())?
            .starts_with(root)
    {
        return Err(format!(
            "undiscovered fixture is not a regular contained file: {response}"
        ));
    }
    let actual_hash = fixture_hash(fs::read(&target).map_err(|error| error.to_string())?);
    if actual_hash != hash {
        return Err(format!(
            "undiscovered fixture changed hash: {actual_hash}; {response}"
        ));
    }
    if let Some(mut state) = state {
        if status["file"]["content_sha256"] != hash
            || status["file"]["source_kind"] != kind.source()
            || status["coverage"]["status"] == "unknown"
            || !status["coverage"]["last_error"].is_null()
        {
            return Err(format!(
                "fixture status changed identity or has an extraction error: {response}"
            ));
        }
        state["status_advanced"] = json!(
            state["index_status"] == "indexed"
                && before_status
                    .as_ref()
                    .is_none_or(|before| before["index_status"] == "pending")
        );
        state["before_status"] = json!(before_status);
        state["discovery_status"] = json!({"arguments":arguments, "response":response});
        return Ok(Some(state));
    }
    if status["coverage"]["status"] != "unknown" {
        return Err(format!(
            "fixture status has no corresponding manifest: {response}"
        ));
    }
    Ok(Some(
        json!({"path":path, "content_sha256":actual_hash, "source_kind":kind.source(),
        "index_status":"undiscovered", "present":1, "exclusion_reason":null, "last_error":null,
        "manifest":null, "inferred_live_fixture":true, "before_status":before_status,
        "discovery_status":{"arguments":arguments, "response":response}}),
    ))
}

fn read_fixture_state(environment: &TestEnvironment, path: &str) -> Result<Option<Value>, String> {
    let connection = Connection::open_with_flags(
        environment
            .workspace
            .join(crate::CONTROL_DIR)
            .join("index/source-search.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| error.to_string())?;
    connection
        .query_row(
            "SELECT display_path, content_sha256, source_kind, index_status,
                    present, exclusion_reason, last_error
             FROM source_manifest WHERE display_path = ?1",
            [path],
            |row| {
                Ok(json!({
                    "path": row.get::<_, String>(0)?,
                    "content_sha256": row.get::<_, Option<String>>(1)?,
                    "source_kind": row.get::<_, String>(2)?,
                    "index_status": row.get::<_, String>(3)?,
                    "present": row.get::<_, i64>(4)?,
                    "exclusion_reason": row.get::<_, Option<String>>(5)?,
                    "last_error": row.get::<_, Option<String>>(6)?,
                }))
            },
        )
        .optional()
        .map_err(|error| error.to_string())
}

/// Injectable transport/clock let integration tests exercise the exact
/// continuation policy without timing-sensitive sleeps or SQL mutations.
pub fn prepare_source_fixture_with(
    path: &str,
    kind: FixtureKind,
    hash: &str,
    deadline: Duration,
    request_cap: u64,
    mut observe: impl FnMut(u64, &Value) -> (Value, Result<Option<Value>, String>),
    mut elapsed: impl FnMut() -> Duration,
) -> Result<Vec<Value>, String> {
    let arguments = json!({"query":path, "path_prefix":path, "limit":50});
    let mut history = Vec::new();
    for attempt in 0..request_cap {
        if elapsed() >= deadline {
            return Err(format!(
                "incomplete source fixture preparation: shared deadline exhausted; observations: {history:?}"
            ));
        }
        let (response, state) = observe(attempt, &arguments);
        history.push(json!({"tool":"search_sources", "arguments":arguments,
                            "response":response, "manifest":state}));
        let ready = fixture_ready(&response, state, path, kind, hash)
            .map_err(|error| format!("{error}; observations: {history:?}"))?;
        if elapsed() >= deadline {
            return Err(format!(
                "incomplete source fixture preparation: shared deadline exhausted during request; observations: {history:?}"
            ));
        }
        if ready {
            return Ok(history);
        }
    }
    Err(format!(
        "incomplete source fixture preparation: request cap exhausted; observations: {history:?}"
    ))
}

fn fixture_ready(
    response: &Value,
    state: Result<Option<Value>, String>,
    path: &str,
    kind: FixtureKind,
    hash: &str,
) -> Result<bool, String> {
    let packet = validate_search_packet(response, path, kind, hash)?;
    let coverage = packet["coverage"].as_str();
    let reconciliation = packet["reconciliation"].as_str();
    let hits = packet["hits"].as_array().unwrap();
    let state = state?.ok_or("source fixture has no discovered manifest state")?;
    validate_manifest(&state, path, kind, hash)?;
    match state["index_status"].as_str() {
        Some("indexed") if !hits.is_empty() => Ok(true),
        Some("pending")
            if hits.is_empty()
                && (coverage == Some("indexing_on_demand") || reconciliation.is_some()) =>
        {
            Ok(false)
        }
        Some("indexed")
            if hits.is_empty()
                && state["status_advanced"] == true
                && (coverage == Some("indexing_on_demand") || reconciliation.is_some()) =>
        {
            Ok(false)
        }
        Some("undiscovered") if hits.is_empty() && reconciliation.is_some() => {
            let status = decode_packet(&state["discovery_status"]["response"])?;
            if status["schema_version"] != 1
                || status["action"] != "status"
                || status["active"] != true
                || status["file"]["path"] != path
                || status["anchor"]["kind"] != kind.anchor()
                || status["anchor"]["physical_start"] != 1
                || status["anchor"]["physical_end"] != 1
                || status["coverage"]["status"] != "unknown"
                || status.get("excerpt").is_some()
                || status.get("original").is_some()
            {
                return Err(
                    "undiscovered source fixture lacks current policy qualification".into(),
                );
            }
            Ok(false)
        }
        _ => Err("source fixture is not indexed or explicitly preparing".into()),
    }
}

fn validate_search_packet(
    response: &Value,
    path: &str,
    kind: FixtureKind,
    hash: &str,
) -> Result<Value, String> {
    let packet = decode_packet(response)?;
    let coverage = packet["coverage"].as_str();
    let reconciliation = packet["reconciliation"].as_str();
    if !matches!(
        coverage,
        Some("complete" | "partial" | "stale" | "indexing_on_demand")
    ) || !(packet["reconciliation"].is_null()
        || matches!(
            reconciliation,
            Some("deferred_budget" | "deferred_concurrent_update")
        ))
        || (reconciliation.is_some() && !matches!(coverage, Some("partial" | "stale")))
    {
        return Err("malformed source fixture preparation coordinator state".into());
    }
    let hits = packet["hits"]
        .as_array()
        .ok_or("malformed source fixture preparation hits")?;
    for hit in hits {
        if hit["path"] != path
            || hit["content_sha256"] != hash
            || hit["anchor_kind"] != kind.anchor()
            || hit["authority"] != "original_source"
            || hit["anchor_start"].as_u64().is_none_or(|start| start < 1)
            || hit["anchor_end"]
                .as_u64()
                .zip(hit["anchor_start"].as_u64())
                .is_none_or(|(end, start)| end < start)
        {
            return Err("source fixture preparation hit changed identity or authority".into());
        }
    }
    Ok(packet)
}

/// An observed terminal row cannot be healed by another reconciliation call.
pub fn request_status_after_manifest(
    before: Option<&Value>,
    path: &str,
    kind: FixtureKind,
    hash: &str,
    request: impl FnOnce() -> Value,
) -> Result<Value, String> {
    if let Some(state) = before {
        validate_manifest(state, path, kind, hash)?;
    }
    Ok(request())
}

fn validate_manifest(
    state: &Value,
    path: &str,
    kind: FixtureKind,
    hash: &str,
) -> Result<(), String> {
    if state["path"] != path
        || state["content_sha256"] != hash
        || state["source_kind"] != kind.source()
        || state["present"] != 1
        || !state["exclusion_reason"].is_null()
        || !state["last_error"].is_null()
        || !matches!(
            state["index_status"].as_str(),
            Some("indexed" | "pending" | "undiscovered")
        )
    {
        return Err(format!(
            "source fixture changed identity, policy, presence or extraction error: {state}"
        ));
    }
    Ok(())
}

fn decode_packet(response: &Value) -> Result<Value, String> {
    if response["result"]["isError"] == true || response.get("error").is_some() {
        return Err(format!(
            "source fixture preparation returned a terminal tool error: {response}"
        ));
    }
    response["result"]["content"][0]["text"]
        .as_str()
        .ok_or_else(|| format!("malformed source fixture preparation response: {response}"))
        .and_then(|text| {
            serde_json::from_str(text)
                .map_err(|_| format!("malformed source fixture preparation JSON: {response}"))
        })
}
