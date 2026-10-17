//! Admission for immutable whole records using the actual read packet serializer.
//!
//! Field limits bound decoded input. This owner additionally checks its encoded
//! read envelope, including JSON escaping, canonical references, trace receipts,
//! and full-width counters. Raw event text retains its separate ranged read path.
use super::{
    pages::{
        Admission, Builder, CursorScope, Dependencies, Item, Packet, PageCoverage, PageItem,
        PageKey, size,
    },
    records::{self, HistoricalChange, Lifecycle, Observation, Record},
    requests::{HistoryRequest, NextCall, ObservationKind, ObservationsRequest, Request},
};
use crate::learner::{LearnerError, Result, limits, model::SchemaVersion};
use rusqlite::{Connection, params};

/// Workspace identity is immutable. All other changing mandatory scalar fields
/// reserve their supported maximum encoded width; no unrelated byte cap exists.
fn envelope(conn: &Connection, action: &str) -> std::result::Result<Packet, String> {
    let mut state = records::workspace_state(conn)?;
    state.memory_generation = i64::MAX;
    state.observation_generation = i64::MAX;
    state.review_generation = i64::MAX;
    state.event_generation = i64::MAX;
    let mut packet = Packet::new(
        &state,
        action,
        i64::MAX,
        false,
        format!("trace-{}", "f".repeat(64)),
        false,
    );
    // A recorded trace has its full ID, unlike either unsuccessful status.
    packet.trace_status = "recorded";
    Ok(packet)
}

fn data_error(error: String) -> LearnerError {
    LearnerError::new("stored_data_invalid", error)
}

fn too_large(field: &str, representation: &str) -> LearnerError {
    let mut error = LearnerError::new(
        "field_too_large",
        format!(
            "{field}: encoded {representation} and required read metadata exceed the {}-byte maximum packet; use a shorter coherent synthesis, keeping lengthy original material in event support",
            limits::PACKET_BYTES
        ),
    );
    error.recovery = Some(serde_json::json!({
        "field_path": field,
        "instruction": "Shorten the derived description or excerpts without dropping qualifications; keep the original input in event support and retry with a fresh request_id."
    }));
    error
}

pub(crate) fn validate_memory_envelopes(
    conn: &Connection,
    id: &str,
    revision: i64,
    field: &str,
) -> Result<()> {
    let memory = records::complete_memory(conn, id, None).map_err(data_error)?;
    let mut packet = envelope(conn, "read").map_err(data_error)?;
    packet.records = Some(vec![Record::Memory(memory.clone())]);
    if size(&packet) > limits::PACKET_BYTES {
        return Err(too_large(field, "current memory"));
    }

    let change = conn.query_row(
        "SELECT revision, snapshot_json, change_kind, reason, change_basis_json, created_at_ms
         FROM memory_changes WHERE memory_id = ?1 AND revision = ?2",
        params![id, revision],
        HistoricalChange::decode,
    )?;
    let request = Request::History(HistoryRequest {
        schema: SchemaVersion::V2,
        memory_id: id.into(),
        max_bytes: Some(limits::PACKET_BYTES),
        limit: Some(1),
        cursor: None,
    });
    let mut packet = envelope(conn, "history").map_err(data_error)?;
    let mut current = records::current_ref(&memory);
    // A future revision or retirement must not make an older snapshot unreadable.
    current.revision = i64::MAX;
    current.lifecycle = Lifecycle::Retired;
    packet.current_ref = Some(current);
    packet.history = Some(vec![]);
    packet.page_count = Some(0);
    let scope = CursorScope::new(
        &request,
        &packet.workspace_id,
        Dependencies::Snapshot {
            memory_generation: Some(i64::MAX),
            observation_generation: None,
            review_generation: None,
            event_generation: None,
        },
    );
    let mut builder = Builder::new(packet, limits::PACKET_BYTES);
    // Reserve a real executable history continuation, even for the first change:
    // later revisions can put this immutable snapshot on an intermediate page.
    match builder.try_push(
        PageItem {
            after: PageKey::Number(revision),
            value: Item::History(change),
        },
        true,
        &scope,
        PageCoverage::None,
    ) {
        Admission::Added => Ok(()),
        Admission::FullBeforeItem => Err(too_large(field, "historical snapshot")),
    }
}

fn defer_memory_refs(observation: &mut Observation) {
    observation.current_memory_refs_more = true;
    observation.current_memory_refs_continuation = Some(NextCall::support(
        ObservationKind::MemoryRefs,
        Some(observation.observation_id.clone()),
        None,
    ));
}

fn single_observation_fits(
    conn: &Connection,
    observation: &Observation,
) -> std::result::Result<bool, String> {
    let mut packet = envelope(conn, "read")?;
    packet.records = Some(vec![Record::Observation(observation.clone())]);
    Ok(size(&packet) <= limits::PACKET_BYTES)
}

fn observation_fits(
    conn: &Connection,
    observation: &Observation,
) -> std::result::Result<bool, String> {
    if !single_observation_fits(conn, observation)? {
        return Ok(false);
    }
    // An accepted observation must remain discoverable in the canonical browse
    // as well as by known ID. Reserve the real intermediate-page continuation.
    let request = Request::Observations(ObservationsRequest {
        schema: SchemaVersion::V2,
        max_bytes: Some(limits::PACKET_BYTES),
        limit: Some(1),
        cursor: None,
        kind: ObservationKind::Observation,
        memory_id: None,
        session_id: None,
        observation_id: None,
        event_id: None,
        query: None,
        literal_terms: vec![],
    });
    let mut packet = envelope(conn, "observations")?;
    packet.observations = Some(vec![]);
    packet.page_count = Some(0);
    let scope = CursorScope::new(
        &request,
        &packet.workspace_id,
        Dependencies::Snapshot {
            memory_generation: None,
            observation_generation: Some(i64::MAX),
            review_generation: None,
            event_generation: None,
        },
    );
    let mut builder = Builder::new(packet, limits::PACKET_BYTES);
    Ok(matches!(
        builder.try_push(
            PageItem {
                after: PageKey::Number(i64::MAX),
                value: Item::Observation(Record::Observation(observation.clone())),
            },
            true,
            &scope,
            PageCoverage::None,
        ),
        Admission::Added
    ))
}

/// Relationship previews can grow after an observation is committed. They are
/// optional; an executable support call preserves access without cutting prose.
pub(super) fn fit_observation_preview(
    conn: &Connection,
    observation: &mut Observation,
) -> std::result::Result<(), String> {
    while !observation_fits(conn, observation)? {
        if observation.current_memory_refs.pop().is_none() {
            // Existing stores predate this conservative write envelope. Once
            // optional refs are exhausted, the actual read Builder decides what
            // fits; synthetic future metadata must not break a valid old read.
            return Ok(());
        }
        defer_memory_refs(observation);
    }
    Ok(())
}

pub(crate) fn validate_observation_envelope(
    conn: &Connection,
    id: &str,
    field: &str,
) -> Result<()> {
    let mut observation = records::observation(conn, id).map_err(data_error)?;
    observation.current_memory_refs.clear();
    // Reserve the actual typed support call for any future memory associations.
    defer_memory_refs(&mut observation);
    if !observation_fits(conn, &observation).map_err(data_error)? {
        return Err(too_large(field, "observation"));
    }
    Ok(())
}
