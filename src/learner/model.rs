use serde::{Deserialize, Serialize};
use serde_json::Value;
pub const SCHEMA: &str = SchemaVersion::V2.as_str();
#[derive(Debug, Serialize)]
pub struct LearnerError {
    pub code: String,
    pub message: String,
    pub captured_event_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery: Option<Value>,
}
impl LearnerError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        let mut message = message.into();
        if message.len() > 2048 {
            let mut end = 2048;
            while !message.is_char_boundary(end) {
                end -= 1;
            }
            message.truncate(end);
            message.push_str(" [error detail truncated]");
        }
        Self {
            code: code.into(),
            message,
            captured_event_ids: vec![],
            recovery: None,
        }
    }
}
impl LearnerError {
    /// A request ID names one request per workspace. Tutors tend to reuse a
    /// readable ID for a new request, so say what a fresh one must be.
    pub fn request_key_conflict(message: impl Into<String>) -> Self {
        let mut error = Self::new("request_key_conflict", message);
        error.recovery = Some(serde_json::json!({
            "field_path": "request_id",
            "instruction": "This request_id already names a different request in this workspace; IDs are shared by every conversation. Retry once with a new request_id that has never been used, such as a fresh suffix. Reuse an ID only to replay the identical request after an unknown outcome.",
        }));
        error
    }
}
impl std::fmt::Display for LearnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for LearnerError {}
impl From<rusqlite::Error> for LearnerError {
    fn from(e: rusqlite::Error) -> Self {
        Self::new("storage_error", e.to_string())
    }
}
impl From<serde_json::Error> for LearnerError {
    fn from(e: serde_json::Error) -> Self {
        Self::new("invalid_variant", e.to_string())
    }
}
pub type Result<T> = std::result::Result<T, LearnerError>;
// Runtime serialization, decoding, storage spelling and schema all use these
// same closed choices. There are no schema-only copies of the protocol.
macro_rules! protocol_enum {
    ($(#[$enum_attr:meta])* $name:ident { $($(#[$variant_attr:meta])* $variant:ident => $wire:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
        $(#[$enum_attr])*
        pub enum $name {
            $($(#[$variant_attr])* #[serde(rename = $wire)] $variant),+
        }
        impl $name {
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $wire),+ }
            }
        }
        impl rusqlite::types::ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                Ok(rusqlite::types::ToSqlOutput::from(self.as_str()))
            }
        }
    };
}
protocol_enum!(SchemaVersion { V2 => "learner-model/v2" });
protocol_enum!(UpdateAction { Update => "update" });
protocol_enum!(#[derive(Default)] UpdateMode {
    #[default] Normal => "normal", CompleteCaptured => "complete_captured"
});
protocol_enum!(Speaker { Learner => "learner", Tutor => "tutor", Tool => "tool", Source => "source" });
protocol_enum!(ObservationKind {
    LearnerWork => "learner_work", LearnerQuestion => "learner_question",
    SelfReport => "self_report", Correction => "correction",
    TeachingResponse => "teaching_response", ExternalCheck => "external_check"
});
protocol_enum!(ObservationRole {
    LearnerResponse => "learner_response", TaskContext => "task_context",
    Assistance => "assistance", TeachingContext => "teaching_context",
    CheckResult => "check_result", CorrectionContext => "correction_context"
});
protocol_enum!(ChangeKind { NewObservation => "new_observation", Reinterpretation => "reinterpretation", Editorial => "editorial" });
protocol_enum!(Disposition { Processed => "processed", NoChange => "no_change" });
protocol_enum!(ReviewStatus { Suggested => "suggested", Done => "done", Dismissed => "dismissed" });
protocol_enum!(Delivery { Proposed => "proposed", Observed => "observed", Unknown => "unknown" });

/// Serde's internally tagged enum derive buffers payload fields and loses their
/// nested diagnostic path. The runtime variants still derive the wire/schema
/// contract; this same declaration dispatches a chosen payload exactly once.
macro_rules! tagged_protocol {
    ($name:ident, operation { $($variant:ident => $wire:literal, $payload:ty;)+ }) => {
        tagged_protocol!($name { $($variant => $wire, $payload;)+ });
        impl $name {
            pub const fn operation(&self) -> &'static str {
                match self { $(Self::$variant(_) => $wire),+ }
            }
        }
    };
    ($name:ident { $($variant:ident => $wire:literal, $payload:ty;)+ }) => {
        #[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
        #[serde(tag = "operation", deny_unknown_fields)]
        pub enum $name { $(#[serde(rename = $wire)] $variant($payload)),+ }
        impl $name {
            pub(crate) fn decode(value: &Value, path: &str, host: &HostContext) -> Result<Self> {
                let object = value.as_object().ok_or_else(|| super::update::field_error(path, "Expected an operation object", host))?;
                let operation = object.get("operation").and_then(Value::as_str).ok_or_else(|| super::update::field_error(&format!("{path}.operation"), "Required string operation", host))?;
                let mut fields = object.clone();
                fields.remove("operation");
                let fields = Value::Object(fields);
                match operation {
                    $($wire => super::update::decode_at::<$payload>(&fields, path, host).map(Self::$variant)),+,
                    _ => Err(super::update::field_error(&format!("{path}.operation"), concat!("Supported operations: ", $($wire, " "),+), host)),
                }
            }
        }
    };
}

/// Supplied by the adapter, never deserialized from model tool arguments.
#[derive(Clone, Debug)]
pub struct HostContext {
    pub host_kind: String,
    pub host_thread_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RawEvent {
    pub external_event_key: String,
    pub speaker: Speaker,
    pub kind: String,
    pub text: String,
    #[serde(default)]
    pub metadata: Option<EventMetadata>,
    #[serde(default)]
    pub occurred_at_ms: Option<i64>,
}
/// Exactly one object key identifies change support. Direct event references
/// may describe a learner challenge; current state still requires observations.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub enum ChangeBasisRef {
    #[serde(rename = "observation_id")]
    Observation(String),
    #[serde(rename = "new_observation")]
    NewObservation(String),
    #[serde(rename = "event_id")]
    Event(String),
}
/// Only observations can support the current descriptive state. Externally
/// tagged newtypes retain the selected single-key object wire form and paths.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub enum CurrentBasisRef {
    #[serde(rename = "observation_id")]
    Observation(String),
    #[serde(rename = "new_observation")]
    NewObservation(String),
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObservationEvent {
    pub event_id: String,
    pub role: ObservationRole,
    #[serde(default)]
    #[schemars(length(max = 4))]
    pub excerpts: Vec<String>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceRef {
    pub path: Option<String>,
    pub uri: Option<String>,
    pub hash: Option<String>,
    pub version: Option<String>,
    pub location: Option<String>,
    pub available_form: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObservationInput {
    /// Nonempty request-local name of at most 256 UTF-8 bytes, unique across all
    /// new objects in this batch. Runtime IDs (e/o/m/s/r-32 hex digits) are reserved.
    /// Refer to it with {"new_observation":"this-name"}.
    pub client_ref: String,
    /// Use self_report for an explicitly stated preference; learner_preference is not a kind.
    pub kind: ObservationKind,
    /// Concrete observed work and its limited implications; not a generic mastery score.
    pub description: String,
    /// Existing captured event IDs with each event's evidentiary role. Not event_ids.
    #[schemars(length(min = 1, max = 16))]
    pub events: Vec<ObservationEvent>,
    #[serde(default)]
    /// Conditions under which the work occurred; state unknowns explicitly.
    pub conditions_text: String,
    #[serde(default)]
    /// Help actually supplied before the work, distinguishing unknown from no help.
    pub assistance_text: String,
    #[serde(default)]
    pub source_refs: Vec<SourceRef>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryRecord {
    pub title: String,
    pub scope: String,
    pub summary: String,
    pub state_text: String,
    pub topics: Option<Vec<String>>,
    pub retrieval_cues: Option<Vec<String>>,
    pub review_note: Option<String>,
    /// Select 1–8 observation objects supporting the complete current description.
    /// This is not an append-only history; prior support remains in memory revisions.
    /// Use {"observation_id":"id"} or {"new_observation":"client_ref"}; event_id is only allowed in change_basis.
    #[schemars(length(min = 1, max = 8))]
    pub current_basis: Vec<CurrentBasisRef>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateMemoryEdit {
    /// Nonempty request-local name of at most 256 UTF-8 bytes, unique across all
    /// new objects in this batch. Runtime IDs (e/o/m/s/r-32 hex digits) are reserved.
    pub client_ref: String,
    /// Complete memory description nested under record; title/state_text/current_basis are not edit-level fields.
    pub record: MemoryRecord,
    pub reason: String,
    /// Evidence objects explaining this change; required separately from record.current_basis.
    #[schemars(length(min = 1, max = 16))]
    pub change_basis: Vec<ChangeBasisRef>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviseMemoryEdit {
    pub memory_id: String,
    pub expected_revision: i64,
    /// Complete memory description nested under record; title/state_text/current_basis are not edit-level fields.
    pub record: MemoryRecord,
    pub change_kind: ChangeKind,
    pub reason: String,
    /// Evidence objects explaining this change; required separately from record.current_basis.
    #[schemars(length(min = 1, max = 16))]
    pub change_basis: Vec<ChangeBasisRef>,
    pub review_resolution: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FlagMemoryEdit {
    pub memory_id: String,
    pub expected_revision: i64,
    pub review_note: String,
    pub reason: String,
    /// Evidence objects explaining this change; required separately from record.current_basis.
    #[schemars(length(min = 1, max = 16))]
    pub change_basis: Vec<ChangeBasisRef>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RetireMemoryEdit {
    pub memory_id: String,
    pub expected_revision: i64,
    pub reason: String,
    #[serde(default)]
    pub replacement_ids: Vec<String>,
    #[serde(default)]
    /// Evidence objects explaining this change; required separately from record.current_basis.
    #[schemars(length(max = 16))]
    pub change_basis: Vec<ChangeBasisRef>,
}
tagged_protocol!(MemoryEdit {
    Create => "create", CreateMemoryEdit;
    Revise => "revise", ReviseMemoryEdit;
    Flag => "flag", FlagMemoryEdit;
    Retire => "retire", RetireMemoryEdit;
});

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StartSessionEdit {
    /// Nonempty request-local name of at most 256 UTF-8 bytes, unique across all
    /// new objects in this batch. Runtime IDs (e/o/m/s/r-32 hex digits) are reserved.
    pub client_ref: String,
    pub goal: String,
    /// Explicitly selected pending unbound inputs; never import a whole backlog.
    #[serde(default)]
    #[schemars(length(max = 32))]
    pub selected_unbound_event_ids: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UseSessionEdit {
    pub session_id: String,
    pub expected_revision: i64,
    pub goal: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResumeSessionEdit {
    pub session_id: String,
    pub expected_revision: i64,
    /// Explicitly selected pending unbound inputs to attach to this lesson.
    #[serde(default)]
    #[schemars(length(max = 32))]
    pub selected_unbound_event_ids: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PauseSessionEdit {
    pub session_id: String,
    pub expected_revision: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompleteSessionEdit {
    pub session_id: String,
    pub expected_revision: i64,
}
tagged_protocol!(SessionEdit, operation {
    Start => "start", StartSessionEdit;
    Use => "use", UseSessionEdit;
    Resume => "resume", ResumeSessionEdit;
    Pause => "pause", PauseSessionEdit;
    Complete => "complete", CompleteSessionEdit;
});

impl SessionEdit {
    /// The only lifecycle operations that can admit independently verified new
    /// input while inactive. Adapter/store validation and generated descriptions
    /// share this vocabulary; a capture reference itself never chooses one.
    pub const CAPTURE_OPERATIONS: [&'static str; 2] = ["start", "resume"];

    pub fn accepts_capture_operation(operation: Option<&str>) -> bool {
        operation.is_some_and(|operation| Self::CAPTURE_OPERATIONS.contains(&operation))
    }
}

/// Preserve the selected wire form: an integer or the literal "after_edit".
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryRevision {
    Known(i64),
    AfterEdit,
}
const AFTER_EDIT: &str = "after_edit";
impl Serialize for MemoryRevision {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        match self {
            Self::Known(revision) => serializer.serialize_i64(*revision),
            Self::AfterEdit => serializer.serialize_str(AFTER_EDIT),
        }
    }
}
impl<'de> Deserialize<'de> for MemoryRevision {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct RevisionVisitor;
        impl serde::de::Visitor<'_> for RevisionVisitor {
            type Value = MemoryRevision;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an integer revision or after_edit")
            }
            fn visit_i64<E: serde::de::Error>(
                self,
                revision: i64,
            ) -> std::result::Result<Self::Value, E> {
                Ok(MemoryRevision::Known(revision))
            }
            fn visit_u64<E: serde::de::Error>(
                self,
                revision: u64,
            ) -> std::result::Result<Self::Value, E> {
                i64::try_from(revision)
                    .map(MemoryRevision::Known)
                    .map_err(|_| E::custom("revision exceeds signed integer range"))
            }
            fn visit_str<E: serde::de::Error>(
                self,
                value: &str,
            ) -> std::result::Result<Self::Value, E> {
                if value == AFTER_EDIT {
                    Ok(MemoryRevision::AfterEdit)
                } else {
                    Err(E::invalid_value(serde::de::Unexpected::Str(value), &self))
                }
            }
        }
        deserializer.deserialize_any(RevisionVisitor)
    }
}
impl schemars::JsonSchema for MemoryRevision {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "MemoryRevision".into()
    }
    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        // The sentinel schema value comes from the runtime serializer.
        let sentinel = serde_json::to_value(Self::AfterEdit).expect("revision sentinel");
        let integer = generator.subschema_for::<i64>();
        schemars::json_schema!({"anyOf":[integer,{"type":"string","const":sentinel}]})
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryRevisionRef {
    pub memory_id: String,
    pub revision: MemoryRevision,
}
/// Event-backed task proposal, with delivery and help recorded separately.
/// A planned task without a captured proposal belongs in checkpoint.next_action
/// or plan_text. This object is not a free-form task draft or delivery claim.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PendingTask {
    /// Required actual captured event ID containing the task proposal. Never
    /// invent an ID or use a learner request as a tutor proposal. A proposal
    /// reference alone does not show that the learner received the task.
    pub proposal_event_ref: String,
    /// Known task instructions and constraints; state unknown conditions.
    pub conditions: String,
    #[serde(default)]
    /// Captured event IDs of actual observed delivery, not planned future
    /// delivery. An empty array means no known observed delivery.
    pub observed_delivery_refs: Vec<String>,
    #[serde(default)]
    /// Captured event IDs of help actually supplied. Proposed future help
    /// belongs in next_action or plan_text until it has an actual event.
    pub help_refs: Vec<String>,
    /// Optional captured event ID of the reference answer; do not invent one.
    pub reference_answer_ref: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    pub current_problem: String,
    pub working_diagnosis: String,
    /// Intended next teaching step, including a specific planned task when no
    /// captured proposal event exists yet. Preserve the meaningful task here.
    pub next_action: String,
    /// Optional concise plan, including planned task wording, conditions and
    /// future help. Planning does not establish proposal, delivery or ability.
    pub plan_text: Option<String>,
    #[serde(default)]
    pub memory_refs: Vec<MemoryRevisionRef>,
    /// Existing event-backed task. Before a proposal event exists, retain the
    /// plan in next_action/plan_text and use null only if no useful current task
    /// needs preserving. Keep actual task refs until deliberately replaced or
    /// skipped; null in this complete checkpoint replacement clears the task.
    /// Complete-captured may retain an identical existing task or clear it, but
    /// cannot introduce or materially change one.
    pub pending_task: Option<PendingTask>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EventDisposition {
    /// Existing event ID; supply at most one disposition for this event per batch.
    pub event_id: String,
    /// processed or no_change; this object has no reason field.
    pub outcome: Disposition,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateReviewEdit {
    /// Nonempty request-local name of at most 256 UTF-8 bytes, unique across all
    /// new objects in this batch. Runtime IDs (e/o/m/s/r-32 hex digits) are reserved.
    pub client_ref: String,
    pub memory_id: String,
    pub purpose: String,
    pub reason: String,
    pub due_at_ms: Option<i64>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateReviewEdit {
    pub review_id: String,
    pub expected_revision: i64,
    pub status: ReviewStatus,
    pub activity_event_id: Option<String>,
}
tagged_protocol!(ReviewEdit {
    Create => "create", CreateReviewEdit;
    Update => "update", UpdateReviewEdit;
});

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(
    deny_unknown_fields,
    bound(deserialize = "M: Deserialize<'de>, S: Deserialize<'de>, R: Deserialize<'de>")
)]
pub struct UpdateRequest<M = MemoryEdit, S = SessionEdit, R = ReviewEdit> {
    #[serde(default)]
    pub native_capture: Option<NativeCaptureRequest>,
    pub schema: SchemaVersion,
    pub action: UpdateAction,
    #[serde(default)]
    pub mode: UpdateMode,
    pub request_id: String,
    /// Existing learner input used as context/support, never a reassignment.
    /// To associate pending unbound work, select it explicitly in session.
    pub turn_ref: Option<String>,
    /// Required whenever checkpoint is supplied. Existing session writes require
    /// session_id and its current expected_revision (independent of memory revisions).
    pub session: Option<S>,
    #[serde(default)]
    pub reported_events: Vec<RawEvent>,
    #[serde(default)]
    pub observations: Vec<ObservationInput>,
    #[serde(default)]
    pub memory_edits: Vec<M>,
    /// Complete lesson checkpoint replacement. When non-null, session is REQUIRED:
    /// use {operation:"use",session_id,expected_revision} from context.status or
    /// the last successful session receipt, or an explicit start/resume operation.
    /// Preserve checkpoint on errors and repair the session guard instead of dropping it.
    pub checkpoint: Option<Checkpoint>,
    #[serde(default)]
    pub event_dispositions: Vec<EventDisposition>,
    #[serde(default)]
    pub review_updates: Vec<R>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UpdateResult {
    pub host_kind: String,
    pub host_thread_id: String,
    pub schema: String,
    pub workspace_id: String,
    pub request_id: String,
    pub captured_event_ids: Vec<String>,
    pub capture_status: String,
    pub update_status: String,
    pub index_status: String,
    pub references: std::collections::BTreeMap<String, String>,
    pub revisions: std::collections::BTreeMap<String, i64>,
}

/// Bounded provenance/context only; no alternate learner profile in event metadata.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EventMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery: Option<Delivery>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub help_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    /// Typed source references; do not add a metadata.source field.
    pub source_refs: Vec<SourceRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    /// Describe image/file provenance here, not in a metadata.image_path field.
    pub attachments: Vec<AttachmentRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage: Option<String>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AttachmentRef {
    pub path: Option<String>,
    pub uri: Option<String>,
    pub media_type: Option<String>,
    pub available_form: String,
    pub content_length: Option<u64>,
}

/// Transport proof of a new native learner message on an explicit start or
/// resume, not evidence of understanding or an instruction to choose a session.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeCaptureRequest {
    pub ticket: String,
    pub text: String,
}

#[cfg(test)]
mod protocol_tests {
    use super::*;
    use serde_json::json;

    fn advertised_choices_decode<
        T: serde::de::DeserializeOwned + schemars::JsonSchema + Serialize,
    >() {
        let schema = serde_json::to_value(schemars::schema_for!(T)).unwrap();
        let choices = schema
            .get("enum")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_else(|| vec![schema["const"].clone()]);
        assert!(!choices.is_empty());
        for choice in choices {
            let decoded: T = serde_json::from_value(choice.clone()).unwrap();
            assert_eq!(serde_json::to_value(decoded).unwrap(), choice);
        }
        assert!(serde_json::from_value::<T>(json!("unsupported")).is_err());
    }

    #[test]
    fn every_advertised_enum_value_uses_the_actual_decoder() {
        advertised_choices_decode::<SchemaVersion>();
        advertised_choices_decode::<UpdateAction>();
        advertised_choices_decode::<UpdateMode>();
        advertised_choices_decode::<Speaker>();
        advertised_choices_decode::<ObservationKind>();
        advertised_choices_decode::<ObservationRole>();
        advertised_choices_decode::<ChangeKind>();
        advertised_choices_decode::<Disposition>();
        advertised_choices_decode::<ReviewStatus>();
        advertised_choices_decode::<Delivery>();
    }

    #[test]
    fn decoder_rejects_invalid_closed_semantic_choices() {
        assert!(
            serde_json::from_value::<ObservationEvent>(json!({"event_id":"e","role":"answer"}))
                .is_err()
        );
        assert!(
            serde_json::from_value::<EventDisposition>(json!({"event_id":"e","outcome":"ignored"}))
                .is_err()
        );
        assert!(
            serde_json::from_value::<ObservationInput>(
                json!({"client_ref":"o","kind":"answer","description":"work","events":[]})
            )
            .is_err()
        );
    }

    #[test]
    fn current_basis_rejects_challenge_event_during_decode() {
        let record = json!({"title":"title","scope":"scope","summary":"summary","state_text":"state","current_basis":[{"event_id":"e"}]});
        assert!(serde_json::from_value::<MemoryRecord>(record.clone()).is_err());
        let edit = json!({"operation":"retire","memory_id":"m","expected_revision":1,"reason":"challenge","change_basis":[{"event_id":"e"}]});
        assert!(serde_json::from_value::<MemoryEdit>(edit).is_ok());
    }

    #[test]
    fn checkpoint_revision_only_accepts_known_or_after_edit() {
        for revision in [json!(1), json!("after_edit")] {
            assert!(
                serde_json::from_value::<MemoryRevisionRef>(
                    json!({"memory_id":"m","revision":revision})
                )
                .is_ok()
            );
        }
        for revision in [
            json!("latest"),
            json!(1.5),
            json!({"revision":1}),
            json!(null),
        ] {
            assert!(
                serde_json::from_value::<MemoryRevisionRef>(
                    json!({"memory_id":"m","revision":revision})
                )
                .is_err()
            );
        }
    }

    #[test]
    fn invalid_optional_mode_retains_independently_permitted_native_capture() {
        let path = std::env::temp_dir().join(format!(
            "magnesium-typed-{}.sqlite",
            super::super::store::new_id("test").unwrap()
        ));
        let store = super::super::LearnerStore::initialize(&path, "typed-workspace").unwrap();
        let host = HostContext {
            host_kind: "test".into(),
            host_thread_id: "typed".into(),
        };
        let native = RawEvent {
            external_event_key: "native".into(),
            speaker: Speaker::Learner,
            kind: "message".into(),
            text: "x = 2".into(),
            metadata: Some(EventMetadata::default()),
            occurred_at_ms: None,
        };
        let request = json!({"schema":SCHEMA,"action":"update","mode":"unsupported","request_id":"invalid-mode","session":{"operation":"start","client_ref":"lesson","goal":"algebra"},"native_capture":{"ticket":"adapter-verified","text":native.text}});
        let error = store
            .update_with_native_capture(&host, request, Some(&native))
            .unwrap_err();
        assert_eq!(error.recovery.as_ref().unwrap()["field_path"], "mode");
        assert_eq!(error.captured_event_ids.len(), 1);
        let text: String = store
            .conn
            .query_row("SELECT text FROM learning_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(text, native.text);
        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn invalid_role_retains_raw_capture_and_exact_nested_error_path() {
        let path = std::env::temp_dir().join(format!(
            "magnesium-typed-{}.sqlite",
            super::super::store::new_id("test").unwrap()
        ));
        let store = super::super::LearnerStore::initialize(&path, "typed-workspace").unwrap();
        let host = HostContext {
            host_kind: "test".into(),
            host_thread_id: "typed".into(),
        };
        let request = json!({"schema":SCHEMA,"action":"update","request_id":"invalid-role","session":{"operation":"start","client_ref":"lesson","goal":"algebra"},"reported_events":[{"external_event_key":"raw","speaker":"learner","kind":"message","text":"x = 2"}],"observations":[{"client_ref":"o","kind":"learner_work","description":"work","events":[{"event_id":"irrelevant","role":"answer"}]}]});
        let error = store.update(&host, request).unwrap_err();
        assert_eq!(
            error.recovery.as_ref().unwrap()["field_path"],
            "observations[0].events[0].role"
        );
        assert_eq!(error.captured_event_ids.len(), 1);
        let text: String = store
            .conn
            .query_row("SELECT text FROM learning_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(text, "x = 2");
        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn oversized_unknown_key_keeps_error_and_recovery_bounded() {
        let host = HostContext {
            host_kind: "test".into(),
            host_thread_id: "typed".into(),
        };
        let mut request = json!({"schema":SCHEMA,"action":"update","request_id":"bounded"});
        request["🧩".repeat(4000)] = json!(true);
        let error = super::super::update::decode_update_request(&request, &host).unwrap_err();
        let path = error.recovery.as_ref().unwrap()["field_path"]
            .as_str()
            .unwrap();
        assert!(path.len() <= 512);
        assert!(path.ends_with("[path truncated]"));
        assert!(serde_json::to_vec(&error).unwrap().len() < 4096);
    }

    #[test]
    fn tagged_memory_fields_keep_exact_nested_paths() {
        let host = HostContext {
            host_kind: "test".into(),
            host_thread_id: "typed".into(),
        };
        let request = json!({"schema":SCHEMA,"action":"update","request_id":"bad-title","memory_edits":[{"operation":"create","client_ref":"m","record":{"title":17,"scope":"scope","summary":"summary","state_text":"state","current_basis":[{"new_observation":"o"}]},"reason":"reason","change_basis":[{"new_observation":"o"}]}]});
        let error = super::super::update::decode_update_request(&request, &host).unwrap_err();
        assert_eq!(
            error.recovery.unwrap()["field_path"],
            "memory_edits[0].record.title"
        );
        let mut request = request;
        request["memory_edits"][0]["record"]["title"] = json!("title");
        request["memory_edits"][0]["record"]["current_basis"] = json!([{"observation_id":17}]);
        let error = super::super::update::decode_update_request(&request, &host).unwrap_err();
        assert_eq!(
            error.recovery.unwrap()["field_path"],
            "memory_edits[0].record.current_basis[0].observation_id"
        );
    }

    #[test]
    fn unsupported_disposition_reason_reports_complete_nested_path() {
        let path = std::env::temp_dir().join(format!(
            "magnesium-typed-{}.sqlite",
            super::super::store::new_id("test").unwrap()
        ));
        let store = super::super::LearnerStore::initialize(&path, "typed-workspace").unwrap();
        let host = HostContext {
            host_kind: "test".into(),
            host_thread_id: "typed".into(),
        };
        let request = json!({"schema":SCHEMA,"action":"update","request_id":"invalid-reason","session":{"operation":"start","client_ref":"lesson","goal":"algebra"},"event_dispositions":[{"event_id":"e","outcome":"no_change","reason":"extra"}]});
        let error = store.update(&host, request).unwrap_err();
        assert_eq!(
            error.recovery.as_ref().unwrap()["field_path"],
            "event_dispositions[0].reason"
        );
        drop(store);
        let _ = std::fs::remove_file(path);
    }
}
