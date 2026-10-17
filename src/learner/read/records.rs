//! Canonical row decoding and explicit current/historical views.
use super::requests::{NextCall, ObservationKind, ReadRef};
use crate::learner::model::{SchemaVersion, SourceRef};
use crate::learner::records::{self as canonical, ObservationRef};
pub(super) use crate::learner::records::{Lifecycle, MemorySnapshot};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde_json::Value;

pub(super) fn db(e: rusqlite::Error) -> String {
    e.to_string()
}
pub(super) fn stored<T: serde::de::DeserializeOwned>(text: String) -> rusqlite::Result<T> {
    canonical::stored(&text, "stored record").map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
    })
}
pub(super) struct WorkspaceState {
    pub workspace_id: String,
    pub memory_generation: i64,
    pub observation_generation: i64,
    pub review_generation: i64,
    pub event_generation: i64,
}
pub(super) fn workspace_state(conn: &Connection) -> Result<WorkspaceState, String> {
    conn.query_row("SELECT workspace_id,memory_generation,observation_generation,review_generation,event_generation FROM workspace_meta WHERE singleton=1",[],|r|Ok(WorkspaceState {workspace_id:r.get(0)?,memory_generation:r.get(1)?,observation_generation:r.get(2)?,review_generation:r.get(3)?,event_generation:r.get(4)?})).map_err(db)
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct MemoryCard {
    pub memory_id: String,
    pub revision: i64,
    pub title: String,
    pub scope: String,
    pub summary: Option<String>,
    pub review_note: Option<String>,
    pub lifecycle: Lifecycle,
    pub last_observed_at_ms: Option<i64>,
    pub current_basis: Vec<ObservationRef>,
    pub replacement_ids: Vec<String>,
    pub matched_intents: Vec<String>,
    pub match_channels: Vec<String>,
    pub state_text: Option<String>,
    pub detail_status: &'static str,
    pub read_ref: ReadRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision_changed: Option<bool>,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct CurrentMemoryRef {
    pub memory_id: String,
    pub revision: i64,
    pub lifecycle: Lifecycle,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct RevisionNotice {
    pub memory_id: String,
    pub revision: i64,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_ref: Option<ReadRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replacement_ids: Option<Vec<String>>,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct PendingNotice {
    pub workspace_pending_event_count: i64,
    pub current_host_pending_event_count: i64,
    pub unbound_pending_event_count: i64,
    pub interpretation_status: &'static str,
    pub session_choices_ref: NextCall,
    pub pending_work_ref: NextCall,
}
#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub(super) enum PendingRef {
    Id(String),
    Read(ReadRef),
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct PendingWork {
    pub event_id: String,
    pub session_id: Option<String>,
    pub host_kind: String,
    pub host_thread_id: String,
    pub origin: String,
    pub captured_at_ms: i64,
    pub capture_time_meaning: &'static str,
    pub read_ref: ReadRef,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct Session {
    pub session_id: String,
    pub goal: String,
    pub status: String,
    pub checkpoint: canonical::StoredCheckpoint,
    pub revision: i64,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct CheckpointBasis {
    /// Mechanical dependency comparison only, not verification of a diagnosis.
    pub status: &'static str,
    pub memories: Vec<CheckpointBasisNotice>,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct CheckpointBasisNotice {
    pub memory_id: String,
    pub used_revision: i64,
    pub current_revision: i64,
    pub lifecycle: Lifecycle,
    pub status: &'static str,
    pub has_review_note: bool,
    pub replacement_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_call: Option<NextCall>,
}
pub(super) fn checkpoint_basis(
    conn: &Connection,
    checkpoint: &canonical::StoredCheckpoint,
) -> Result<CheckpointBasis, String> {
    let saved = checkpoint.memory_refs();
    let ids = saved
        .iter()
        .map(|reference| reference.memory_id.clone())
        .collect::<Vec<_>>();
    let current = canonical::memory_fields(conn, &ids).map_err(|error| error.to_string())?;
    let mut reconsider = false;
    let memories = saved
        .iter()
        .zip(current)
        .map(|(reference, memory)| {
            // The shared stored decoder rejects unresolved after_edit sentinels.
            let crate::learner::model::MemoryRevision::Known(used_revision) = reference.revision
            else {
                unreachable!("canonical stored revision")
            };
            let changed = used_revision != memory.revision;
            let retired = !memory.lifecycle.active();
            let has_review_note = memory
                .review_note
                .as_deref()
                .is_some_and(|note| !note.is_empty());
            let needs_read = changed || retired || has_review_note;
            reconsider |= needs_read;
            CheckpointBasisNotice {
                next_call: needs_read.then(|| NextCall::current_memory(&memory.memory_id)),
                memory_id: memory.memory_id,
                used_revision,
                current_revision: memory.revision,
                lifecycle: memory.lifecycle,
                status: if retired {
                    "retired"
                } else if changed {
                    "changed"
                } else {
                    "unchanged"
                },
                has_review_note,
                replacement_ids: memory.replacement_ids,
            }
        })
        .collect();
    Ok(CheckpointBasis {
        status: if saved.is_empty() {
            "unreferenced"
        } else if reconsider {
            "needs_reconsideration"
        } else {
            "unchanged"
        },
        memories,
    })
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct SessionChoice {
    pub session_id: String,
    pub goal: String,
    pub status: String,
    pub revision: i64,
    pub pending_event_count: i64,
    pub last_pending_captured_at_ms: Option<i64>,
    pub pending_events_ref: NextCall,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct Topic {
    pub topic: String,
    pub count: i64,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct HistoricalChange {
    pub historical: bool,
    pub revision: i64,
    pub snapshot: MemorySnapshot,
    pub change_kind: String,
    pub reason: String,
    pub change_basis: Vec<String>,
    pub created_at_ms: i64,
}
impl HistoricalChange {
    pub(super) fn decode(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            historical: true,
            revision: row.get(0)?,
            snapshot: stored(row.get(1)?)?,
            change_kind: row.get(2)?,
            reason: row.get(3)?,
            change_basis: stored(row.get(4)?)?,
            created_at_ms: row.get(5)?,
        })
    }
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct EventRef {
    pub event_id: String,
    pub role: String,
    pub excerpts: Vec<String>,
    pub speaker: String,
    pub origin: String,
    pub occurred_at_ms: Option<i64>,
    pub captured_at_ms: i64,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct Observation {
    pub kind: &'static str,
    pub historical: bool,
    pub observation_id: String,
    pub session_id: Option<String>,
    pub observation_kind: String,
    pub description: String,
    pub conditions_text: String,
    pub assistance_text: String,
    pub source_refs: Vec<SourceRef>,
    pub observed_at_ms: Option<i64>,
    pub created_at_ms: i64,
    pub current_memory_refs: Vec<CurrentMemoryRef>,
    pub current_memory_refs_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_memory_refs_continuation: Option<NextCall>,
    pub event_refs: Vec<EventRef>,
    pub event_refs_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_refs_continuation: Option<NextCall>,
}
impl Observation {
    fn decode(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            kind: "observation",
            historical: true,
            observation_id: row.get(0)?,
            session_id: row.get(1)?,
            observation_kind: row.get(2)?,
            description: row.get(3)?,
            conditions_text: row.get(4)?,
            assistance_text: row.get(5)?,
            source_refs: stored(row.get(6)?)?,
            observed_at_ms: row.get(7)?,
            created_at_ms: row.get(8)?,
            current_memory_refs: vec![],
            current_memory_refs_more: false,
            current_memory_refs_continuation: None,
            event_refs: vec![],
            event_refs_more: false,
            event_refs_continuation: None,
        })
    }
}
impl EventRef {
    fn decode(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            event_id: row.get(0)?,
            role: row.get(1)?,
            excerpts: stored(row.get(2)?)?,
            speaker: row.get(3)?,
            origin: row.get(4)?,
            occurred_at_ms: row.get(5)?,
            captured_at_ms: row.get(6)?,
        })
    }
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct ObservationRole {
    pub role: String,
    pub excerpts: Vec<String>,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct Event {
    pub kind: &'static str,
    pub historical: bool,
    pub event_id: String,
    pub session_id: Option<String>,
    pub speaker: String,
    pub event_kind: String,
    pub origin: String,
    pub text: String,
    pub metadata: Value,
    pub occurred_at_ms: Option<i64>,
    pub captured_at_ms: i64,
    pub processing: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub content_length: usize,
    pub partial: bool,
    pub current_memory_refs: Vec<CurrentMemoryRef>,
    pub current_memory_refs_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_memory_refs_continuation: Option<NextCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_ref: Option<ReadRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation_roles: Option<Vec<ObservationRole>>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub(super) enum Record {
    Memory(MemoryCard),
    Observation(Observation),
    Event(Event),
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct Review {
    pub review_id: String,
    pub memory_id: String,
    pub memory_revision: i64,
    pub purpose: String,
    pub reason: String,
    pub due_at_ms: Option<i64>,
    pub stored_status: String,
    pub revision: i64,
    pub updated_at_ms: i64,
    pub status: &'static str,
    pub current_memory: MemoryCard,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct Identity {
    pub schema: SchemaVersion,
    pub package_version: &'static str,
    pub teaching_policy_version: &'static str,
    pub host_kind: String,
    pub capture: &'static str,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct HostBinding {
    pub host_kind: String,
    pub host_thread_id: String,
    pub recording_active: bool,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct Status {
    pub identity: Identity,
    pub storage_family: SchemaVersion,
    pub schema_version: i64,
    pub instruction_identity: &'static str,
    pub host_binding: HostBinding,
    pub observation_index_current: bool,
}

pub(super) fn cards(conn: &Connection, ids: &[String]) -> Result<Vec<MemoryCard>, String> {
    canonical::memory_fields(conn, ids)
        .map(|rows| rows.into_iter().map(project_card).collect())
        .map_err(|error| error.to_string())
}
pub(super) fn card(conn: &Connection, id: &str) -> Result<MemoryCard, String> {
    cards(conn, &[id.to_owned()])?
        .pop()
        .ok_or_else(|| format!("unknown_reference: {id}"))
}
fn project_card(memory: canonical::MemoryFields) -> MemoryCard {
    let active = memory.lifecycle.active();
    MemoryCard {
        read_ref: ReadRef::CurrentMemory {
            id: memory.memory_id.clone(),
            expected_revision: None,
        },
        memory_id: memory.memory_id,
        revision: memory.revision,
        title: memory.title,
        scope: memory.scope,
        summary: active.then_some(memory.summary),
        review_note: memory.review_note,
        lifecycle: memory.lifecycle,
        last_observed_at_ms: memory.last_observed_at_ms,
        current_basis: memory
            .current_basis
            .into_iter()
            .map(|observation_id| ObservationRef { observation_id })
            .collect(),
        replacement_ids: memory.replacement_ids,
        matched_intents: vec![],
        match_channels: vec![],
        state_text: None,
        detail_status: if active { "read_required" } else { "retired" },
        revision_changed: None,
    }
}
/// Whole-record representation shared by read execution and write admission.
/// Decode the canonical snapshot once; its body is already available.
pub(super) fn complete_memory(
    conn: &Connection,
    id: &str,
    expected_revision: Option<i64>,
) -> Result<MemoryCard, String> {
    let snapshot = canonical::memory_snapshot(conn, id).map_err(|error| error.to_string())?;
    let (fields, state) = canonical::MemoryFields::from_snapshot(snapshot);
    let mut memory = project_card(fields);
    if memory.lifecycle.active() {
        memory.state_text = Some(state);
        memory.detail_status = "complete";
    }
    memory.revision_changed = Some(expected_revision.is_some_and(|n| memory.revision != n));
    Ok(memory)
}
pub(super) fn bodies(
    conn: &Connection,
    cards: &[MemoryCard],
) -> Result<std::collections::BTreeMap<String, String>, String> {
    let ids = cards
        .iter()
        .filter(|card| card.lifecycle.active())
        .map(|card| card.memory_id.clone())
        .collect::<Vec<_>>();
    canonical::memory_bodies(conn, &ids).map_err(|error| error.to_string())
}
pub(super) fn current_ref(card: &MemoryCard) -> CurrentMemoryRef {
    CurrentMemoryRef {
        memory_id: card.memory_id.clone(),
        revision: card.revision,
        lifecycle: card.lifecycle.clone(),
    }
}
pub(super) fn page_ids(
    conn: &Connection,
    topic: Option<&str>,
    last: i64,
    limit: usize,
) -> Result<Vec<(i64, String)>, String> {
    let mut st=conn.prepare("SELECT m.memory_no,m.memory_id FROM learner_memories m WHERE m.lifecycle='active' AND m.memory_no>?1 AND (?2 IS NULL OR EXISTS(SELECT 1 FROM memory_topics t WHERE t.memory_id=m.memory_id AND t.topic=?2)) ORDER BY m.memory_no LIMIT ?3").map_err(db)?;
    st.query_map(params![last, topic, limit as i64], |r| {
        Ok((r.get(0)?, r.get(1)?))
    })
    .map_err(db)?
    .collect::<rusqlite::Result<_>>()
    .map_err(db)
}
pub(super) fn observation(conn: &Connection, id: &str) -> Result<Observation, String> {
    let mut v = conn
        .query_row(
            "SELECT observation_id, session_id, kind, description, conditions_text,
                assistance_text, source_refs_json, observed_at_ms, created_at_ms
         FROM observations WHERE observation_id = ?1",
            [id],
            Observation::decode,
        )
        .optional()
        .map_err(db)?
        .ok_or_else(|| format!("unknown_reference: {id}"))?;
    let mut st = conn
        .prepare(
            "SELECT m.memory_id, m.revision, m.lifecycle
         FROM memory_observations a JOIN learner_memories m ON m.memory_id = a.memory_id
         WHERE a.observation_id = ?1 ORDER BY m.memory_no LIMIT 33",
        )
        .map_err(db)?;
    let related = st
        .query_map([id], |r| {
            Ok(CurrentMemoryRef {
                memory_id: r.get(0)?,
                revision: r.get(1)?,
                lifecycle: stored(format!("\"{}\"", r.get::<_, String>(2)?))?,
            })
        })
        .map_err(db)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(db)?;
    v.current_memory_refs_more = related.len() > 32;
    v.current_memory_refs = related.into_iter().take(32).collect();
    if v.current_memory_refs_more {
        v.current_memory_refs_continuation = Some(NextCall::support(
            ObservationKind::MemoryRefs,
            Some(id.into()),
            None,
        ));
    }
    let mut st = conn
        .prepare(
            "SELECT oe.event_id, oe.role, oe.excerpts_json, e.speaker, e.origin,
                e.occurred_at_ms, e.captured_at_ms
         FROM observation_events oe JOIN learning_events e ON e.event_id = oe.event_id
         WHERE oe.observation_id = ?1 ORDER BY e.event_no, oe.role LIMIT 33",
        )
        .map_err(db)?;
    let related = st
        .query_map([id], EventRef::decode)
        .map_err(db)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(db)?;
    v.event_refs_more = related.len() > 32;
    v.event_refs = related.into_iter().take(32).collect();
    if v.event_refs_more {
        v.event_refs_continuation = Some(NextCall::support(
            ObservationKind::EventRefs,
            Some(id.into()),
            None,
        ));
    }
    super::readability::fit_observation_preview(conn, &mut v)?;
    Ok(v)
}
pub(super) fn event(
    conn: &Connection,
    id: &str,
    start: Option<usize>,
    end: Option<usize>,
) -> Result<Event, String> {
    let mut v=conn.query_row("SELECT event_id,session_id,speaker,kind,origin,text,metadata_json,occurred_at_ms,captured_at_ms,processing FROM learning_events WHERE event_id=?1",[id],|r|Ok(Event {kind:"event",historical:true,event_id:r.get(0)?,session_id:r.get(1)?,speaker:r.get(2)?,event_kind:r.get(3)?,origin:r.get(4)?,text:r.get(5)?,metadata:stored(r.get(6)?)?,occurred_at_ms:r.get(7)?,captured_at_ms:r.get(8)?,processing:r.get(9)?,start_byte:0,end_byte:0,content_length:0,partial:false,current_memory_refs:vec![],current_memory_refs_more:false,current_memory_refs_continuation:None,read_ref:None,observation_roles:None})).optional().map_err(db)?.ok_or_else(||format!("unknown_reference: {id}"))?;
    let len = v.text.len();
    let start = start.unwrap_or(0);
    let end = end.unwrap_or(len);
    if start > end || end > len || !v.text.is_char_boundary(start) || !v.text.is_char_boundary(end)
    {
        return Err(
            "invalid_variant: event byte range must use UTF-8 boundaries within content_length"
                .into(),
        );
    }
    v.text = v.text[start..end].into();
    v.start_byte = start;
    v.end_byte = end;
    v.content_length = len;
    v.partial = start != 0 || end != len;
    let mut st=conn.prepare("SELECT DISTINCT m.memory_no,m.memory_id,m.revision,m.lifecycle FROM learner_memories m JOIN memory_observations mo ON mo.memory_id=m.memory_id JOIN observation_events oe ON oe.observation_id=mo.observation_id WHERE oe.event_id=?1 ORDER BY m.memory_no LIMIT 33").map_err(db)?;
    let related = st
        .query_map([id], |r| {
            Ok(CurrentMemoryRef {
                memory_id: r.get(1)?,
                revision: r.get(2)?,
                lifecycle: stored(format!("\"{}\"", r.get::<_, String>(3)?))?,
            })
        })
        .map_err(db)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(db)?;
    v.current_memory_refs_more = related.len() > 32;
    v.current_memory_refs = related.into_iter().take(32).collect();
    if v.current_memory_refs_more {
        v.current_memory_refs_continuation = Some(NextCall::support(
            ObservationKind::EventMemoryRefs,
            None,
            Some(id.into()),
        ));
    }
    Ok(v)
}
