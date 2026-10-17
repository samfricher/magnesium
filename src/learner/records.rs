//! Canonical stored learner meaning, shared by writes and current/history reads.
//!
//! Stored basis IDs retain the existing database representation. Current cards
//! deliberately project them into reusable observation-reference objects.
use super::{
    LearnerError, Result,
    model::{Checkpoint, EventMetadata, MemoryRevision, PendingTask, Speaker},
};
use rusqlite::{Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// Initial sessions store {}. Nonempty checkpoints use the same strict typed
/// contract at read and completion boundaries, never a permissive JSON mirror.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum StoredCheckpoint {
    Saved(Box<Checkpoint>),
    Empty(EmptyCheckpoint),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EmptyCheckpoint {}
impl StoredCheckpoint {
    pub(crate) fn decode(text: &str, session_id: &str) -> Result<Self> {
        let checkpoint: Self = stored(
            text,
            &format!("learning_sessions[{session_id}].checkpoint_json"),
        )?;
        if let Self::Saved(saved) = &checkpoint
            && (saved.memory_refs.len() > 16
                || saved.memory_refs.iter().any(|reference| {
                    let bounded_id = reference.memory_id.len() <= super::limits::REFERENCE_BYTES;
                    let saved_revision = matches!(
                        reference.revision,
                        MemoryRevision::Known(revision) if revision >= 1
                    );
                    !bounded_id || !saved_revision
                }))
        {
            return Err(LearnerError::new(
                "stored_data_invalid",
                "checkpoint.memory_refs: saved checkpoints require at most 16 bounded IDs and positive saved revisions",
            ));
        }
        Ok(checkpoint)
    }
    pub(crate) fn pending_task(&self) -> Option<&PendingTask> {
        match self {
            Self::Saved(checkpoint) => checkpoint.pending_task.as_ref(),
            Self::Empty(_) => None,
        }
    }
    pub(crate) fn memory_refs(&self) -> &[super::model::MemoryRevisionRef] {
        match self {
            Self::Saved(checkpoint) => &checkpoint.memory_refs,
            Self::Empty(_) => &[],
        }
    }
}

/// The original stored speaker and explicit delivery are separate from origin.
/// Loading these facts does not upgrade model-reported provenance or certify
/// learner attendance. Observations and task references share this decoder.
pub(crate) struct EventFacts {
    pub speaker: Speaker,
    pub metadata: Option<EventMetadata>,
    pub occurred_at_ms: Option<i64>,
}
pub(crate) fn event_facts(conn: &Connection, event_id: &str) -> Result<EventFacts> {
    let row = conn
        .query_row(
            "SELECT speaker,metadata_json,occurred_at_ms FROM learning_events WHERE event_id=?1",
            [event_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get(2)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| LearnerError::new("unknown_reference", event_id))?;
    Ok(EventFacts {
        speaker: stored(
            &serde_json::to_string(&row.0)?,
            &format!("learning_events[{event_id}].speaker"),
        )?,
        metadata: stored(
            &row.1,
            &format!("learning_events[{event_id}].metadata_json"),
        )?,
        occurred_at_ms: row.2,
    })
}
impl EventFacts {
    pub(crate) fn observed_tutor_delivery(&self) -> bool {
        self.speaker == Speaker::Tutor
            && self
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.delivery)
                == Some(super::model::Delivery::Observed)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Lifecycle {
    Active,
    Retired,
}
impl Lifecycle {
    pub(crate) fn active(&self) -> bool {
        matches!(self, Self::Active)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ObservationRef {
    pub observation_id: String,
}

/// Every persisted snapshot contains all required state, including empty lists.
/// Only review resolution is absent on changes that did not resolve a review.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MemorySnapshot {
    pub memory_id: String,
    pub title: String,
    pub scope: String,
    pub summary: String,
    pub state_text: String,
    pub retrieval_cues: Vec<String>,
    #[serde(deserialize_with = "required_nullable")]
    pub review_note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_resolution: Option<String>,
    pub current_basis: Vec<String>,
    pub replacement_ids: Vec<String>,
    pub lifecycle: Lifecycle,
    pub revision: i64,
    #[serde(deserialize_with = "required_nullable")]
    pub last_observed_at_ms: Option<i64>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub topics: Vec<String>,
}

fn required_nullable<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

pub(crate) fn stored<T: DeserializeOwned>(text: &str, field: &str) -> Result<T> {
    let mut decoder = serde_json::Deserializer::from_str(text);
    let value = serde_path_to_error::deserialize(&mut decoder).map_err(|error| {
        LearnerError::new(
            "stored_data_invalid",
            format!(
                "{field}.{}: {}; preserve the store and repair the malformed canonical data",
                error.path(),
                error.inner()
            ),
        )
    })?;
    decoder
        .end()
        .map_err(|error| LearnerError::new("stored_data_invalid", format!("{field}: {error}")))?;
    Ok(value)
}

/// Compact canonical fields. This is a projection of the same row, never a
/// second durable summary; full snapshots and cards share this strict decoder.
pub(crate) struct MemoryFields {
    pub memory_id: String,
    pub title: String,
    pub scope: String,
    pub summary: String,
    pub review_note: Option<String>,
    pub current_basis: Vec<String>,
    pub replacement_ids: Vec<String>,
    pub lifecycle: Lifecycle,
    pub revision: i64,
    pub last_observed_at_ms: Option<i64>,
}
impl MemoryFields {
    pub(crate) fn from_snapshot(memory: MemorySnapshot) -> (Self, String) {
        (
            Self {
                memory_id: memory.memory_id,
                title: memory.title,
                scope: memory.scope,
                summary: memory.summary,
                review_note: memory.review_note,
                current_basis: memory.current_basis,
                replacement_ids: memory.replacement_ids,
                lifecycle: memory.lifecycle,
                revision: memory.revision,
                last_observed_at_ms: memory.last_observed_at_ms,
            },
            memory.state_text,
        )
    }
}
struct StoredMemoryFields {
    memory_id: String,
    title: String,
    scope: String,
    summary: String,
    review_note: Option<String>,
    current_basis_json: String,
    replacement_ids_json: String,
    lifecycle: String,
    revision: i64,
    last_observed_at_ms: Option<i64>,
}
impl StoredMemoryFields {
    fn decode(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            memory_id: row.get(0)?,
            title: row.get(1)?,
            scope: row.get(2)?,
            summary: row.get(3)?,
            review_note: row.get(4)?,
            current_basis_json: row.get(5)?,
            replacement_ids_json: row.get(6)?,
            lifecycle: row.get(7)?,
            revision: row.get(8)?,
            last_observed_at_ms: row.get(9)?,
        })
    }
    fn into_fields(self) -> Result<MemoryFields> {
        let field = |name: &str| format!("learner_memories[{}].{name}", self.memory_id);
        Ok(MemoryFields {
            current_basis: stored(&self.current_basis_json, &field("current_basis_json"))?,
            replacement_ids: stored(&self.replacement_ids_json, &field("replacement_ids_json"))?,
            lifecycle: stored(
                &serde_json::to_string(&self.lifecycle)?,
                &field("lifecycle"),
            )?,
            memory_id: self.memory_id,
            title: self.title,
            scope: self.scope,
            summary: self.summary,
            review_note: self.review_note,
            revision: self.revision,
            last_observed_at_ms: self.last_observed_at_ms,
        })
    }
}
const COMPACT_COLUMNS: &str = "m.memory_id, m.title, m.scope, m.summary,
    m.review_note, m.current_basis_json, m.replacement_ids_json, m.lifecycle,
    m.revision, m.last_observed_at_ms";

/// Preserve caller order, including duplicate explicit references. JSON supplies
/// bounded IDs as data, without constructing SQL or one statement per memory.
pub(crate) fn memory_fields(conn: &Connection, ids: &[String]) -> Result<Vec<MemoryFields>> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let mut statement = conn.prepare(&format!(
        "SELECT {COMPACT_COLUMNS} FROM json_each(?1) requested
         JOIN learner_memories m ON m.memory_id=requested.value
         ORDER BY CAST(requested.key AS INTEGER)"
    ))?;
    let rows = statement
        .query_map([serde_json::to_string(ids)?], StoredMemoryFields::decode)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if rows.len() != ids.len() {
        let missing = ids
            .iter()
            .find(|id| !rows.iter().any(|row| row.memory_id == **id));
        return Err(LearnerError::new(
            "unknown_reference",
            missing.map_or("memory", String::as_str),
        ));
    }
    rows.into_iter()
        .map(StoredMemoryFields::into_fields)
        .collect()
}

pub(crate) fn memory_bodies(
    conn: &Connection,
    ids: &[String],
) -> Result<std::collections::BTreeMap<String, String>> {
    if ids.is_empty() {
        return Ok(std::collections::BTreeMap::new());
    }
    let mut statement = conn.prepare(
        "SELECT memory_id,state_text FROM learner_memories
         WHERE lifecycle='active' AND memory_id IN (SELECT value FROM json_each(?1))",
    )?;
    Ok(statement
        .query_map([serde_json::to_string(ids)?], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect::<rusqlite::Result<_>>()?)
}

struct StoredMemoryRow {
    fields: StoredMemoryFields,
    state_text: String,
    retrieval_cues_json: String,
    created_at_ms: i64,
    updated_at_ms: i64,
}
impl StoredMemoryRow {
    fn decode(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            fields: StoredMemoryFields::decode(row)?,
            state_text: row.get(10)?,
            retrieval_cues_json: row.get(11)?,
            created_at_ms: row.get(12)?,
            updated_at_ms: row.get(13)?,
        })
    }
    fn into_snapshot(self, topics: Vec<String>) -> Result<MemorySnapshot> {
        let fields = self.fields.into_fields()?;
        Ok(MemorySnapshot {
            retrieval_cues: stored(
                &self.retrieval_cues_json,
                &format!("learner_memories[{}].retrieval_cues_json", fields.memory_id),
            )?,
            current_basis: fields.current_basis,
            replacement_ids: fields.replacement_ids,
            lifecycle: fields.lifecycle,
            memory_id: fields.memory_id,
            title: fields.title,
            scope: fields.scope,
            summary: fields.summary,
            state_text: self.state_text,
            review_note: fields.review_note,
            review_resolution: None,
            revision: fields.revision,
            last_observed_at_ms: fields.last_observed_at_ms,
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
            topics,
        })
    }
}

pub(crate) fn memory_snapshot(conn: &Connection, id: &str) -> Result<MemorySnapshot> {
    let row = conn
        .query_row(
            &format!(
                "SELECT {COMPACT_COLUMNS}, m.state_text, m.retrieval_cues_json,
                    m.created_at_ms, m.updated_at_ms
             FROM learner_memories m WHERE m.memory_id = ?1"
            ),
            [id],
            StoredMemoryRow::decode,
        )
        .optional()?
        .ok_or_else(|| LearnerError::new("unknown_reference", id))?;
    let mut statement =
        conn.prepare("SELECT topic FROM memory_topics WHERE memory_id = ?1 ORDER BY topic")?;
    let topics = statement
        .query_map([id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    row.into_snapshot(topics)
}
