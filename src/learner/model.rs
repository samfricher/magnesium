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
protocol_enum!(MessageIntent { Talk => "talk" });
protocol_enum!(FirstTry { Right => "right", Partly => "partly", Wrong => "wrong", Declined => "declined" });
protocol_enum!(Confidence { Sure => "sure", Unsure => "unsure", Guess => "guess" });
protocol_enum!(GoalStepStatus { Todo => "todo", Doing => "doing", Done => "done" });
protocol_enum!(AttemptHelp { None => "none", Hint => "hint", Step => "step", Solution => "solution" });
protocol_enum!(GapType { K => "K", S => "S", N => "N", A => "A", R => "R" });

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
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(
    description = "Exactly one object key identifies change support. Direct event references\nmay describe a learner challenge; current state still requires observations."
)]
pub enum ChangeBasisRef {
    #[serde(rename = "observation_id")]
    Observation(String),
    #[serde(rename = "new_observation")]
    NewObservation(String),
    #[serde(rename = "event_id")]
    Event(String),
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(
    description = "Only observations can support the current descriptive state. Externally\ntagged newtypes retain the selected single-key object wire form and paths."
)]
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
    #[schemars(
        description = "One of original, excerpt, model_reported or unavailable. A missing or\nunknown value is saved as model_reported."
    )]
    #[serde(default)]
    pub available_form: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObservationInput {
    #[schemars(
        description = "Nonempty request-local name of at most 256 UTF-8 bytes, unique across all\nnew objects in this batch. Runtime IDs (e/o/m/s/r-32 hex digits) are reserved.\nRefer to it with {\"new_observation\":\"this-name\"}."
    )]
    pub client_ref: String,
    #[schemars(
        description = "Use self_report for an explicitly stated preference; learner_preference is not a kind."
    )]
    pub kind: ObservationKind,
    #[schemars(
        description = "Concrete observed work and its limited implications; not a generic mastery score."
    )]
    pub description: String,
    #[schemars(
        description = "Existing captured event IDs with each event's evidentiary role. Not event_ids."
    )]
    #[schemars(length(min = 1, max = 16))]
    pub events: Vec<ObservationEvent>,
    #[serde(default)]
    #[schemars(
        description = "Conditions under which the work occurred; state unknowns explicitly."
    )]
    pub conditions_text: String,
    #[serde(default)]
    #[schemars(
        description = "Help actually supplied before the work, distinguishing unknown from no help."
    )]
    pub assistance_text: String,
    #[serde(default)]
    pub source_refs: Vec<SourceRef>,
    #[schemars(
        description = "Give this when the observation is the learner's first try at a task you\nset, or their decline of a quick check. The runtime stores it as one\nrow per answer. A wrong or partly right first try schedules a quick\ncheck on its label for the next day; a later right, cold first try on\na due check passes it. A right first try reached through a full\nsolution is asked for cold the next day. The runtime works out every\npass, status and next date. Needs a learner_response event."
    )]
    #[serde(default)]
    pub attempt: Option<Attempt>,
    #[schemars(
        description = "Instead of attempt, for a marked paper: one attempt per answered\nquestion or part, each with paper. At most 40."
    )]
    #[serde(default)]
    #[schemars(length(max = 40))]
    pub attempts: Vec<Attempt>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(
    description = "One marked answer. The runtime stores it as a row code can count, and\nschedules quick checks from chat answers."
)]
#[serde(deny_unknown_fields)]
pub struct Attempt {
    #[schemars(
        description = "A short name, 1–120 bytes, for the skill or misunderstanding in words,\nsuch as \"sign of b in the quadratic formula\", or the paper's tag. Never\nthe problem: no equation and no question number. Reuse an open quick\ncheck's label exactly for the same skill, and when recording its answer\nor decline."
    )]
    pub label: String,
    #[schemars(
        description = "The question as you set it, such as \"Solve 2x² − 5x − 3 = 0\". On a\npaper, a line of the question is enough; paper says which it was.\nLonger than 1,000 bytes is cut."
    )]
    pub question: String,
    #[schemars(
        description = "The mark for the first try before any correction: right, partly or\nwrong. declined when the learner declines a quick check, or asks for\nthe full solution before giving any answer: record that with help\nsolution, and the step is asked for cold the next day."
    )]
    pub first_try: FirstTry,
    #[schemars(
        description = "The most help given on this question before the first try: none,\nhint, step (a worked step of this question) or solution. A worked or\npartly worked example on another problem is not help on this one, so\nthe changed version or full problem that follows it is none unless\nyou helped with that question itself."
    )]
    pub help: AttemptHelp,
    #[schemars(
        description = "true only when the answer was unaided, from a blank page, on a later\nday than the teaching. Never true with help other than none, nor when\nthis label already has an answer earlier the same day."
    )]
    pub cold: bool,
    #[schemars(
        description = "The learner's confidence letter, when they gave one: sure (S), unsure\n(U) or guess (G)."
    )]
    #[serde(default)]
    pub confidence: Option<Confidence>,
    #[schemars(description = "Seconds the answer took, only where a clock ran.")]
    #[serde(default)]
    pub seconds: Option<u32>,
    #[schemars(
        description = "For an answer on a practice paper: which paper and question, and its\nmarks. first_try is right for full marks, wrong for none, else partly."
    )]
    #[serde(default)]
    pub paper: Option<PaperMark>,
    #[schemars(
        description = "true when the answer was right but over the time allowed. A Type S\nrow never passes on a slow answer."
    )]
    #[serde(default)]
    pub slow: bool,
    #[schemars(
        description = "The gap-log rows, such as \"midterm-1#3\", or quick-check labels this\nanswer is a redo of. The runtime counts it for each: a pass when it is\ndue and the answer is right, cold and unaided; a fail when it is due\nand the answer is not. On a fail, name only the rows whose tag holds\nthe first error. At most 12."
    )]
    #[serde(default)]
    #[schemars(length(max = super::limits::COUNTS_FOR_PER_ATTEMPT))]
    pub counts_for: Vec<String>,
    #[schemars(
        description = "On a paper answer that lost marks, or a full-mark answer that was too\nslow (type S): the gap-log row it makes. The runtime numbers the row,\nwrites it into the plan's gap-log.md and dates its redos once you\nreport its repair. Needed on every lost mark unless counts_for names\nthe row it fails or paper.stretch is true."
    )]
    #[serde(default)]
    pub gap: Option<GapReport>,
    #[schemars(
        description = "The next question to pose cold on the item this answer makes or\nredoes, with its answer key. Write it when the item is made and again\nafter each pass."
    )]
    #[serde(default)]
    pub next: Option<NextQuestion>,
    #[schemars(
        description = "The kind of problem: problems that share one method, such as\n\"discrete expected value\", 1–120 bytes. With a course graph it is the\ntag. Reuse a name from known_kinds in an earlier result for the same\nmethod. Give it when the learner has no starting move (no_start), on\nthe changed version or full problem that follows an example, and on\neach later problem, cold redo or check of that kind. The runtime keeps\nthe kind's help level from these: a first try that is not right\nwithout help moves it back one step, and the result's help_levels\nstates it. An example given on another problem is not help on this one."
    )]
    #[serde(default)]
    pub kind: Option<String>,
    #[schemars(
        description = "true when the learner has no starting move on a new problem of this\nkind, with first_try declined (they wrote nothing) or wrong. Needs\nkind. The runtime moves the help level forward and the result's\nhelp_levels says which help to give now. Never for the same problem\nor its changed version."
    )]
    #[serde(default)]
    pub no_start: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(
    description = "A gap-log row, as the tutor reports it. Dates, passes and status are the\nruntime's."
)]
#[serde(deny_unknown_fields)]
pub struct GapReport {
    #[schemars(
        description = "K did not know, S slow, N notation or missing step, A arithmetic, R\nmisread."
    )]
    #[serde(rename = "type")]
    pub gap_type: GapType,
    #[schemars(
        description = "What was written, and what the mark scheme needed. Over 1,000 bytes\nis cut."
    )]
    pub what_went_wrong: String,
    #[schemars(
        description = "The plan's folder under Study/, such as \"midterm-1\". Needed only when\nthe study folder has more than one plan."
    )]
    #[serde(default)]
    pub plan: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NextQuestion {
    #[schemars(
        description = "A new problem the learner has not seen, posed as is, that needs the\nskill unprompted. It asks for a result and never names or points at\nthe skill, tag, method, form of answer, step or earlier mistake:\n\"Solve …\", not \"Solve … by <method>\" or \"Write … in the form …\". Each\nearlier miss gives it a different answer from the right one."
    )]
    pub question: String,
    #[schemars(
        description = "Its answer and how to mark it. Held back until the learner answers."
    )]
    pub answer_key: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(
    description = "A change to one gap-log row or quick check. Name it as the gap log or the\nsitting opener does: \"midterm-1#3\" for a row, or a quick check's label."
)]
#[serde(deny_unknown_fields)]
pub struct PracticeEdit {
    pub item: String,
    #[schemars(
        description = "The repair is done: what it was, such as \"taught the slope; two\nchecks\". The first redo falls due the next day, never the same day.\nReport it again after a failed redo's new repair."
    )]
    #[serde(default)]
    pub repaired: Option<String>,
    #[serde(default)]
    pub next: Option<NextQuestion>,
    #[schemars(description = "Correct a row's type.")]
    #[serde(default, rename = "type")]
    pub gap_type: Option<GapType>,
    #[schemars(description = "Correct a row's description.")]
    #[serde(default)]
    pub what_went_wrong: Option<String>,
    #[schemars(
        description = "Take the item out, when it was logged in error: why. Its attempts stay."
    )]
    #[serde(default)]
    pub dropped: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(
    description = "Correct a stored attempt, such as when the learner shows a mark was wrong.\nGive only the fields that change. The runtime keeps the attempt as it was,\nand works out every item it counts for again."
)]
#[serde(deny_unknown_fields)]
pub struct AttemptCorrection {
    #[schemars(
        description = "From practice.attempts in the result that stored it, or `magnesium\nattempts --json`."
    )]
    pub attempt_id: String,
    #[schemars(description = "Why, in a sentence.")]
    pub reason: String,
    #[serde(default)]
    pub first_try: Option<FirstTry>,
    #[schemars(description = "A paper answer's marks. first_try follows them.")]
    #[serde(default)]
    pub marks: Option<f64>,
    #[serde(default)]
    pub help: Option<AttemptHelp>,
    #[serde(default)]
    pub cold: Option<bool>,
    #[serde(default)]
    pub slow: Option<bool>,
    #[schemars(description = "Replaces the rows and checks it counts for.")]
    #[serde(default)]
    pub counts_for: Option<Vec<String>>,
    #[schemars(description = "Replaces the kind of problem; an empty string takes it away.")]
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub no_start: Option<bool>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PaperMark {
    #[schemars(description = "The paper's name as the gap log writes it, such as \"MockA\".")]
    pub name: String,
    #[schemars(description = "The question and part, such as \"3b\".")]
    pub question: String,
    pub marks: f64,
    pub out_of: f64,
    #[schemars(
        description = "true for a question beyond the plan's scope: it makes no gap-log row."
    )]
    #[serde(default)]
    pub stretch: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(description = "One step of the lesson's current goal.")]
#[serde(deny_unknown_fields)]
pub struct GoalStep {
    pub step: String,
    #[schemars(description = "todo, doing or done.")]
    pub status: GoalStepStatus,
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
    #[schemars(
        description = "Select 1–8 observation objects supporting the complete current description.\nThis is not an append-only history; prior support remains in memory revisions.\nUse {\"observation_id\":\"id\"} or {\"new_observation\":\"client_ref\"}; event_id is only allowed in change_basis."
    )]
    #[schemars(length(min = 1, max = 8))]
    pub current_basis: Vec<CurrentBasisRef>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateMemoryEdit {
    #[schemars(
        description = "Nonempty request-local name of at most 256 UTF-8 bytes, unique across all\nnew objects in this batch. Runtime IDs (e/o/m/s/r-32 hex digits) are reserved."
    )]
    pub client_ref: String,
    #[schemars(
        description = "Complete memory description nested under record; title/state_text/current_basis are not edit-level fields."
    )]
    pub record: MemoryRecord,
    pub reason: String,
    #[schemars(
        description = "Evidence objects explaining this change; required separately from record.current_basis."
    )]
    #[schemars(length(min = 1, max = 16))]
    pub change_basis: Vec<ChangeBasisRef>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviseMemoryEdit {
    pub memory_id: String,
    pub expected_revision: i64,
    #[schemars(
        description = "Complete memory description nested under record; title/state_text/current_basis are not edit-level fields."
    )]
    pub record: MemoryRecord,
    pub change_kind: ChangeKind,
    pub reason: String,
    #[schemars(
        description = "Evidence objects explaining this change; required separately from record.current_basis."
    )]
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
    #[schemars(
        description = "Evidence objects explaining this change; required separately from record.current_basis."
    )]
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
    #[schemars(
        description = "Evidence objects explaining this change; required separately from record.current_basis."
    )]
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
    #[schemars(
        description = "Nonempty request-local name of at most 256 UTF-8 bytes, unique across all\nnew objects in this batch. Runtime IDs (e/o/m/s/r-32 hex digits) are reserved."
    )]
    pub client_ref: String,
    pub goal: String,
    #[schemars(
        description = "Explicitly selected pending unbound inputs; never import a whole backlog."
    )]
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
    #[schemars(
        description = "Explicitly selected pending unbound inputs to attach to this lesson."
    )]
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
    pub const CAPTURE_OPERATIONS: [&'static str; 2] = ["start", "resume"];

    pub fn accepts_capture_operation(operation: Option<&str>) -> bool {
        operation.is_some_and(|operation| Self::CAPTURE_OPERATIONS.contains(&operation))
    }
}

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
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(
    description = "Event-backed task proposal, with delivery and help recorded separately.\nA planned task without a captured proposal belongs in checkpoint.next_action\nor plan_text. This object is not a free-form task draft or delivery claim."
)]
#[serde(deny_unknown_fields)]
pub struct PendingTask {
    #[schemars(
        description = "Required actual captured event ID containing the task proposal. Never\ninvent an ID or use a learner request as a tutor proposal. A proposal\nreference alone does not show that the learner received the task."
    )]
    pub proposal_event_ref: String,
    #[schemars(
        description = "The task's own words, copied exactly from the reply that posed it:\nthe question and what it states, not the feedback or teaching before\nit. The study window marks them as the task. A task's words restate\nevery number, name and condition its answer needs, so these stand\nalone. Other constraints go in plan_text."
    )]
    pub conditions: String,
    #[serde(default)]
    #[schemars(
        description = "Captured event IDs of actual observed delivery, not planned future\ndelivery. An empty array means no known observed delivery."
    )]
    pub observed_delivery_refs: Vec<String>,
    #[serde(default)]
    #[schemars(
        description = "Captured event IDs of help actually supplied. Proposed future help\nbelongs in next_action or plan_text until it has an actual event."
    )]
    pub help_refs: Vec<String>,
    #[schemars(
        description = "Optional captured event ID of the reference answer; do not invent one."
    )]
    pub reference_answer_ref: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    pub current_problem: String,
    pub working_diagnosis: String,
    #[schemars(
        description = "Intended next teaching step, including a specific planned task when no\ncaptured proposal event exists yet. Preserve the meaningful task here."
    )]
    pub next_action: String,
    #[schemars(
        description = "Optional concise plan, including planned task wording, conditions and\nfuture help. Planning does not establish proposal, delivery or ability."
    )]
    pub plan_text: Option<String>,
    #[serde(default)]
    pub memory_refs: Vec<MemoryRevisionRef>,
    #[schemars(
        description = "Existing event-backed task. Before a proposal event exists, retain the\nplan in next_action/plan_text and use null only if no useful current task\nneeds preserving. Keep actual task refs until deliberately replaced or\nskipped; null in this complete checkpoint replacement clears the task.\nComplete-captured may retain an identical existing task or clear it, but\ncannot introduce or materially change one."
    )]
    pub pending_task: Option<PendingTask>,
    #[schemars(
        description = "Optional ordered steps of the current goal, at most 12. A checkpoint\nsaved before this field existed has none."
    )]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(max = 12))]
    pub goal_steps: Vec<GoalStep>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EventDisposition {
    #[schemars(
        description = "Existing event ID; supply at most one disposition for this event per batch."
    )]
    pub event_id: String,
    #[schemars(description = "processed or no_change; this object has no reason field.")]
    pub outcome: Disposition,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateReviewEdit {
    #[schemars(
        description = "Nonempty request-local name of at most 256 UTF-8 bytes, unique across all\nnew objects in this batch. Runtime IDs (e/o/m/s/r-32 hex digits) are reserved."
    )]
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
    #[schemars(
        description = "Required. An ID you make up for this request, new to this study folder:\nevery conversation here shares its IDs, so end it with something only\nthis turn has, such as the last characters of its ticket or event ID.\nSend an ID again only to replay the identical request after an unknown\noutcome."
    )]
    pub request_id: String,
    #[schemars(
        description = "Existing learner input used as context/support, never a reassignment.\nTo associate pending unbound work, select it explicitly in session."
    )]
    pub turn_ref: Option<String>,
    #[schemars(
        description = "Required whenever checkpoint is supplied. Existing session writes require\nsession_id and its current expected_revision (independent of memory revisions)."
    )]
    pub session: Option<S>,
    #[serde(default)]
    pub reported_events: Vec<RawEvent>,
    #[serde(default)]
    pub observations: Vec<ObservationInput>,
    #[serde(default)]
    pub memory_edits: Vec<M>,
    #[schemars(
        description = "Complete lesson checkpoint replacement. When non-null, session is REQUIRED:\nuse {operation:\"use\",session_id,expected_revision} from context.status or\nthe last successful session receipt, or an explicit start/resume operation.\nPreserve checkpoint on errors and repair the session guard instead of dropping it."
    )]
    pub checkpoint: Option<Checkpoint>,
    #[serde(default)]
    pub event_dispositions: Vec<EventDisposition>,
    #[serde(default)]
    pub review_updates: Vec<R>,
    #[schemars(
        description = "Report a repair, the next question, or a correction, on gap-log rows\nand quick checks. Never a date, pass or status: the runtime works those\nout. At most 40."
    )]
    #[serde(default)]
    #[schemars(length(max = super::limits::PRACTICE_EDITS_PER_BATCH))]
    pub practice: Vec<PracticeEdit>,
    #[schemars(description = "At most 8.")]
    #[serde(default)]
    #[schemars(length(max = super::limits::ATTEMPT_CORRECTIONS_PER_BATCH))]
    pub attempt_corrections: Vec<AttemptCorrection>,
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
    #[schemars(
        description = "What the runtime cut or relabeled to save this request instead of\nrefusing it. Absent when nothing was changed."
    )]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shortened: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(
    description = "Bounded provenance/context only; no alternate learner profile in event metadata."
)]
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
    #[schemars(description = "Typed source references; do not add a metadata.source field.")]
    pub source_refs: Vec<SourceRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(
        description = "Describe image/file provenance here, not in a metadata.image_path field."
    )]
    pub attachments: Vec<AttachmentRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage: Option<String>,
    #[schemars(
        description = "Set by the host alone, from how the learner sent the message: talk is\nnever an answer, so no attempt may cite it. Absent from the tool\nschema, and refused on a model-reported event."
    )]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(skip)]
    pub intent: Option<MessageIntent>,
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

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(
    description = "Transport proof of a new native learner message on an explicit start or\nresume, not evidence of understanding or an instruction to choose a session."
)]
#[serde(deny_unknown_fields)]
pub struct NativeCaptureRequest {
    pub ticket: String,
    pub text: String,
}

#[cfg(test)]
mod protocol_tests;
