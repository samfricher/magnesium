//! The decoded read protocol. Each action accepts only its own fields.
use super::pages::Cursor;
use crate::learner::{
    model::SchemaVersion,
    search::{self, Intent, Query},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use unicode_normalization::UnicodeNormalization;
// Each action's payload is declared once. The two actual wire decoders and
// shared internal dispatch enum are generated from that ownership list.
pub(super) trait ReadProtocol: Sized {
    fn decode(raw: &serde_json::Value) -> Result<Self, String>;
}
macro_rules! read_actions {
    (context { $( $context:ident($cwire:literal,$cty:ty) ),* $(,)? }
     memory { $( $memory:ident($mwire:literal,$mty:ty) ),* $(,)? }
     shared { $( $shared:ident($swire:literal,$sty:ty) ),* $(,)? }) => {
        #[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
        #[serde(tag="action",deny_unknown_fields)]
        pub enum ContextReadRequest { $( #[serde(rename=$cwire)] $context($cty), )* $( #[serde(rename=$swire)] $shared($sty), )* }
        #[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
        #[serde(tag="action",deny_unknown_fields)]
        pub enum MemoryRequest { $( #[serde(rename=$mwire)] $memory($mty), )* $( #[serde(rename=$swire)] $shared($sty), )* }
        #[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
        #[serde(tag="action",deny_unknown_fields)]
        pub enum Request { $( #[serde(rename=$cwire)] $context($cty), )* $( #[serde(rename=$mwire)] $memory($mty), )* $( #[serde(rename=$swire)] $shared($sty), )* }
        impl ReadProtocol for ContextReadRequest {
            fn decode(raw:&serde_json::Value)->Result<Self,String> {
                let (action,fields)=super::action_fields(raw)?;
                match action {
                    $( $cwire=>super::decode_fields::<$cty>(&fields).map(Self::$context), )*
                    $( $swire=>super::decode_fields::<$sty>(&fields).map(Self::$shared), )*
                    _=>Err(super::shape_error("action",concat!("Supported context actions: ",$( $cwire," ", )* $( $swire," ", )*))),
                }
            }
        }
        impl ReadProtocol for MemoryRequest {
            fn decode(raw:&serde_json::Value)->Result<Self,String> {
                let (action,fields)=super::action_fields(raw)?;
                match action {
                    $( $mwire=>super::decode_fields::<$mty>(&fields).map(Self::$memory), )*
                    $( $swire=>super::decode_fields::<$sty>(&fields).map(Self::$shared), )*
                    _=>Err(super::shape_error("action",concat!("Supported memory actions: ",$( $mwire," ", )* $( $swire," ", )*))),
                }
            }
        }
        #[cfg(test)]
        impl super::Tool {
            /// Tests name only the action; this picks the native tool that owns it.
            pub(super) fn for_action(action:&str)->Self {
                match action { $( $cwire=>Self::Context, )* _=>Self::Memory }
            }
        }
        impl Request {
            fn into_next_call(self) -> NextCall {
                match self {
                    $( Self::$context(arguments) => NextCall::Context { arguments: Box::new(ContextReadRequest::$context(arguments)) }, )*
                    $( Self::$memory(arguments) => NextCall::Memory { arguments: Box::new(MemoryRequest::$memory(arguments)) }, )*
                    $( Self::$shared(arguments) => NextCall::Memory { arguments: Box::new(MemoryRequest::$shared(arguments)) }, )*
                }
            }
        }
        impl Request {pub fn action(&self)->&'static str {match self {$( Self::$context(_)=>$cwire, )* $( Self::$memory(_)=>$mwire, )* $( Self::$shared(_)=>$swire, )* }}}
        impl From<ContextReadRequest> for Request {
            fn from(request:ContextReadRequest)->Self {match request { $( ContextReadRequest::$context(value)=>Self::$context(value), )* $( ContextReadRequest::$shared(value)=>Self::$shared(value), )* }}
        }
        impl From<MemoryRequest> for Request {
            fn from(request:MemoryRequest)->Self {match request { $( MemoryRequest::$memory(value)=>Self::$memory(value), )* $( MemoryRequest::$shared(value)=>Self::$shared(value), )* }}
        }
    }
}
read_actions! {
    context {Prepare("prepare",PrepareRequest),Status("status",StatusRequest),SessionChoices("session_choices",SessionChoicesRequest),PendingEvents("pending_events",PendingEventsRequest)}
    memory {Search("search",PrepareRequest),Browse("browse",BrowseRequest),Topics("topics",TopicsRequest),Read("read",ReadRequest),History("history",HistoryRequest),Reviews("reviews",ReviewsRequest),Observations("observations",ObservationsRequest)}
    shared {PendingWork("pending_work",PendingWorkRequest)}
}
/// A complete executable tool call, serialized from the same action types as
/// the strict native tool boundary. Explanatory advice belongs outside this type.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "tool", rename_all = "snake_case", deny_unknown_fields)]
pub enum NextCall {
    Memory { arguments: Box<MemoryRequest> },
    Context { arguments: Box<ContextReadRequest> },
}
impl NextCall {
    pub(super) fn selected_read(request: &ReadRequest, remaining: &[ReadRef]) -> Self {
        Self::Memory {
            arguments: Box::new(MemoryRequest::Read(ReadRequest {
                schema: request.schema,
                max_bytes: request.max_bytes,
                refs: remaining.to_vec(),
            })),
        }
    }
    pub fn current_memory(id: &str) -> Self {
        Self::Memory {
            arguments: Box::new(MemoryRequest::Read(ReadRequest {
                schema: SchemaVersion::V2,
                max_bytes: Some(65536),
                refs: vec![ReadRef::CurrentMemory {
                    id: id.into(),
                    expected_revision: None,
                }],
            })),
        }
    }
    pub fn session_status(id: &str) -> Self {
        Self::current_status(Some(id))
    }
    pub fn current_status(session_id: Option<&str>) -> Self {
        Self::Context {
            arguments: Box::new(ContextReadRequest::Status(StatusRequest {
                schema: SchemaVersion::V2,
                max_bytes: Some(65536),
                session_id: session_id.map(str::to_owned),
            })),
        }
    }
    pub fn reviews_for_memory(id: &str) -> Self {
        Self::Memory {
            arguments: Box::new(MemoryRequest::Reviews(ReviewsRequest {
                schema: SchemaVersion::V2,
                max_bytes: Some(65536),
                limit: None,
                cursor: None,
                memory_id: Some(id.into()),
            })),
        }
    }
    pub(super) fn browse(topic: Option<String>) -> Self {
        Self::Memory {
            arguments: Box::new(MemoryRequest::Browse(BrowseRequest {
                schema: SchemaVersion::V2,
                max_bytes: None,
                limit: None,
                cursor: None,
                topic,
            })),
        }
    }
    pub(super) fn topics() -> Self {
        Self::Memory {
            arguments: Box::new(MemoryRequest::Topics(TopicsRequest {
                schema: SchemaVersion::V2,
                max_bytes: None,
                limit: None,
                cursor: None,
            })),
        }
    }
    pub(super) fn session_choices(cursor: Option<Cursor>) -> Self {
        Self::Context {
            arguments: Box::new(ContextReadRequest::SessionChoices(SessionChoicesRequest {
                schema: SchemaVersion::V2,
                max_bytes: None,
                limit: Some(3),
                cursor,
            })),
        }
    }
    pub(crate) fn pending_events(session_id: &str) -> Self {
        Self::Context {
            arguments: Box::new(ContextReadRequest::PendingEvents(PendingEventsRequest {
                schema: SchemaVersion::V2,
                max_bytes: Some(65536),
                limit: None,
                cursor: None,
                session_id: Some(session_id.into()),
            })),
        }
    }
    pub(super) fn pending_work(unbound_only: bool) -> Self {
        Self::Memory {
            arguments: Box::new(MemoryRequest::PendingWork(PendingWorkRequest {
                schema: SchemaVersion::V2,
                max_bytes: Some(65536),
                limit: None,
                cursor: None,
                host_kind: None,
                host_thread_id: None,
                session_id: None,
                unbound_only,
            })),
        }
    }
    pub(super) fn support(
        kind: ObservationKind,
        observation_id: Option<String>,
        event_id: Option<String>,
    ) -> Self {
        Self::Memory {
            arguments: Box::new(MemoryRequest::Observations(ObservationsRequest {
                schema: SchemaVersion::V2,
                max_bytes: Some(65536),
                limit: None,
                cursor: None,
                kind,
                memory_id: None,
                session_id: None,
                observation_id,
                event_id,
                query: None,
                literal_terms: vec![],
            })),
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KnownRevision {
    pub memory_id: String,
    pub revision: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReadRef {
    CurrentMemory {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_revision: Option<i64>,
    },
    Observation {
        id: String,
    },
    Event {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        start_byte: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        end_byte: Option<usize>,
    },
}
impl ReadRef {
    pub fn id(&self) -> &str {
        match self {
            Self::CurrentMemory { id, .. } | Self::Observation { id } | Self::Event { id, .. } => {
                id
            }
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Auto,
    Focused,
    Overview,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ObservationKind {
    #[default]
    Observation,
    Event,
    MemoryRefs,
    EventRefs,
    EventMemoryRefs,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PrepareRequest {
    pub schema: SchemaVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 65536))]
    pub max_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 32))]
    pub limit: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<Mode>,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub original_query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default)]
    #[schemars(length(max = 8))]
    pub memory_ids: Vec<String>,
    #[serde(default)]
    #[schemars(length(max = 16))]
    pub known_revisions: Vec<KnownRevision>,
    #[serde(default)]
    #[schemars(length(max = 4))]
    pub intents: Vec<Intent>,
    #[serde(default)]
    #[schemars(length(max = 4))]
    pub literal_terms: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub literal_intent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrowseRequest {
    pub schema: SchemaVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 65536))]
    pub max_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 32))]
    pub limit: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TopicsRequest {
    pub schema: SchemaVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 65536))]
    pub max_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 32))]
    pub limit: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadRequest {
    pub schema: SchemaVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 65536))]
    pub max_bytes: Option<usize>,
    /// Selected complete records, returned in order with an executable next_call
    /// when the packet budget requires another batch. Revisions stay with refs.
    #[schemars(length(min = 1, max = 32))]
    pub refs: Vec<ReadRef>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HistoryRequest {
    pub schema: SchemaVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 65536))]
    pub max_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 32))]
    pub limit: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
    pub memory_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewsRequest {
    pub schema: SchemaVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 65536))]
    pub max_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 32))]
    pub limit: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_id: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObservationsRequest {
    pub schema: SchemaVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 65536))]
    pub max_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 32))]
    pub limit: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
    #[serde(default)]
    pub kind: ObservationKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<Query>,
    #[serde(default)]
    #[schemars(length(max = 4))]
    pub literal_terms: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionChoicesRequest {
    pub schema: SchemaVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 65536))]
    pub max_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 32))]
    pub limit: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PendingEventsRequest {
    pub schema: SchemaVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 65536))]
    pub max_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 32))]
    pub limit: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StatusRequest {
    pub schema: SchemaVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 65536))]
    pub max_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PendingWorkRequest {
    pub schema: SchemaVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 65536))]
    pub max_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 32))]
    pub limit: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default)]
    pub unbound_only: bool,
}
impl Request {
    pub(super) fn next_call(&self, cursor: Cursor) -> NextCall {
        let mut request = self.clone();
        match &mut request {
            Self::Prepare(r) | Self::Search(r) => r.cursor = Some(cursor),
            Self::Browse(r) => r.cursor = Some(cursor),
            Self::Topics(r) => r.cursor = Some(cursor),
            Self::History(r) => r.cursor = Some(cursor),
            Self::Reviews(r) => r.cursor = Some(cursor),
            Self::Observations(r) => r.cursor = Some(cursor),
            Self::SessionChoices(r) => r.cursor = Some(cursor),
            Self::PendingEvents(r) => r.cursor = Some(cursor),
            Self::PendingWork(r) => r.cursor = Some(cursor),
            Self::Read(_) | Self::Status(_) => unreachable!("unpaged action has no cursor"),
        }
        request.into_next_call()
    }
    pub fn max_bytes(&self) -> usize {
        match self {
            Self::Prepare(r) => r.max_bytes,
            Self::Search(r) => r.max_bytes,
            Self::Browse(r) => r.max_bytes,
            Self::Topics(r) => r.max_bytes,
            Self::Read(r) => r.max_bytes,
            Self::History(r) => r.max_bytes,
            Self::Reviews(r) => r.max_bytes,
            Self::Observations(r) => r.max_bytes,
            Self::SessionChoices(r) => r.max_bytes,
            Self::PendingEvents(r) => r.max_bytes,
            Self::Status(r) => r.max_bytes,
            Self::PendingWork(r) => r.max_bytes,
        }
        .unwrap_or(crate::learner::limits::DEFAULT_PACKET_BYTES)
    }
    pub fn cursor(&self) -> Option<&Cursor> {
        match self {
            Self::Prepare(r) | Self::Search(r) => r.cursor.as_ref(),
            Self::Browse(r) => r.cursor.as_ref(),
            Self::Topics(r) => r.cursor.as_ref(),
            Self::History(r) => r.cursor.as_ref(),
            Self::Reviews(r) => r.cursor.as_ref(),
            Self::Observations(r) => r.cursor.as_ref(),
            Self::SessionChoices(r) => r.cursor.as_ref(),
            Self::PendingEvents(r) => r.cursor.as_ref(),
            Self::PendingWork(r) => r.cursor.as_ref(),
            _ => None,
        }
    }
    pub fn limit(&self) -> usize {
        match self {
            Self::Prepare(r) | Self::Search(r) => r.limit,
            Self::Browse(r) => r.limit,
            Self::Topics(r) => r.limit,
            Self::History(r) => r.limit,
            Self::Reviews(r) => r.limit,
            Self::Observations(r) => r.limit,
            Self::SessionChoices(r) => r.limit,
            Self::PendingEvents(r) => r.limit,
            Self::PendingWork(r) => r.limit,
            _ => None,
        }
        .unwrap_or(32)
    }
    pub fn session_id(&self) -> Option<&str> {
        match self {
            Self::Prepare(r) | Self::Search(r) => r.session_id.as_deref(),
            Self::Observations(r) => r.session_id.as_deref(),
            Self::PendingEvents(r) => r.session_id.as_deref(),
            Self::Status(r) => r.session_id.as_deref(),
            Self::PendingWork(r) => r.session_id.as_deref(),
            _ => None,
        }
    }
    pub fn memory_id(&self) -> Option<&str> {
        match self {
            Self::History(r) => Some(&r.memory_id),
            Self::Reviews(r) => r.memory_id.as_deref(),
            Self::Observations(r) => r.memory_id.as_deref(),
            _ => None,
        }
    }
    pub fn normalized_identity(&self) -> serde_json::Value {
        use serde_json::json;
        match self {
            Self::Prepare(r) | Self::Search(r) => {
                json!([self.action(), r.topic.as_deref().map(search::normalize)])
            }
            Self::Browse(r) => json!([self.action(), r.topic.as_deref().map(search::normalize)]),
            Self::History(r) => json!([self.action(), r.memory_id]),
            Self::Reviews(r) => json!([self.action(), r.memory_id]),
            Self::Observations(r) => json!([
                self.action(),
                r.kind,
                r.memory_id,
                r.session_id,
                r.observation_id,
                r.event_id,
                r.query.as_ref().and_then(|q| search::compile(q).ok()),
                r.literal_terms
            ]),
            Self::PendingEvents(r) => json!([self.action(), r.session_id]),
            Self::PendingWork(r) => json!([
                self.action(),
                r.host_kind,
                r.host_thread_id,
                r.session_id,
                r.unbound_only
            ]),
            _ => json!([self.action()]),
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        let fail = |detail: &str| Err(format!("field_too_large: {detail}"));
        if self.max_bytes() == 0
            || self.max_bytes() > 65536
            || self.limit() == 0
            || self.limit() > 32
        {
            return fail("max_bytes/limit");
        }
        if self.session_id().is_some_and(|id| id.len() > 256)
            || self.memory_id().is_some_and(|id| id.len() > 256)
        {
            return fail("record/session ID exceeds 256 bytes");
        }
        match self {
            Self::Prepare(r) | Self::Search(r) => {
                if r.cursor.is_some() && r.mode != Some(Mode::Overview) {
                    return Err("invalid_variant: cursor applies to overview mode; use browse for directory continuation".into());
                }
                let distinct = r
                    .memory_ids
                    .iter()
                    .chain(r.known_revisions.iter().map(|k| &k.memory_id))
                    .collect::<BTreeSet<_>>();
                if r.memory_ids.len() > 8
                    || r.known_revisions.len() > 16
                    || distinct.len() > 16
                    || distinct.iter().any(|id| id.len() > 256)
                    || r.goal.len() > 2048
                    || r.original_query.len() > 2048
                    || r.literal_terms.len() > 4
                    || r.literal_terms.iter().any(|s| s.len() > 256)
                    || r.intents.len() > 4
                {
                    return fail("prepare field bounds");
                }
                if r.known_revisions.iter().any(|k| k.revision < 1) {
                    return Err(
                        "invalid_variant: known_revisions requires positive revisions".into(),
                    );
                }
                if let Some(target) = &r.literal_intent {
                    let implicit = if r
                        .original_query
                        .nfc()
                        .collect::<String>()
                        .split(|c: char| !c.is_alphanumeric())
                        .any(|word| word.chars().count() >= 2)
                    {
                        Some("original-wording")
                    } else if !r.literal_terms.is_empty() {
                        Some("literal")
                    } else {
                        None
                    };
                    if !(r.intents.iter().any(|intent| &intent.id == target)
                        || (r.intents.is_empty() && implicit == Some(target.as_str())))
                    {
                        return Err("invalid_variant: literal_intent".into());
                    }
                }
                let mut ids = BTreeSet::new();
                for intent in &r.intents {
                    if intent.id.is_empty()
                        || intent.id.len() > 128
                        || intent.purpose.len() > 2048
                        || intent.queries.len() > 2
                        || !ids.insert(&intent.id)
                    {
                        return Err("invalid_variant: intent bounds or duplicate ID".into());
                    }
                    for q in &intent.queries {
                        if let Err(e) = search::compile(q)
                            && e != "no_search_terms"
                        {
                            return Err(e);
                        }
                    }
                }
            }
            Self::Read(r) => {
                if r.refs.is_empty()
                    || r.refs.len() > 32
                    || r.refs.iter().any(|rf| rf.id().len() > 256)
                {
                    return fail("refs must contain 1..32 bounded records");
                }
                if r.refs.iter().any(
                    |rf| matches!(rf,ReadRef::CurrentMemory{expected_revision:Some(n),..} if *n<1),
                ) {
                    return Err("invalid_variant: expected_revision must be positive".into());
                }
            }
            Self::Observations(r) => {
                if r.observation_id.as_ref().is_some_and(|id| id.len() > 256)
                    || r.event_id.as_ref().is_some_and(|id| id.len() > 256)
                    || r.literal_terms.len() > 4
                    || r.literal_terms.iter().any(|s| s.len() > 256)
                {
                    return fail("observations field bounds");
                }
                if let Some(query) = &r.query {
                    search::compile(query)?;
                }
                let valid = match r.kind {
                    ObservationKind::Observation => {
                        r.observation_id.is_none()
                            && r.event_id.is_none()
                            && r.literal_terms.is_empty()
                    }
                    ObservationKind::Event => {
                        r.observation_id.is_none()
                            && r.event_id.is_none()
                            && r.query.is_none()
                            && !r.literal_terms.is_empty()
                    }
                    ObservationKind::MemoryRefs | ObservationKind::EventRefs => {
                        r.observation_id.is_some()
                            && r.event_id.is_none()
                            && r.memory_id.is_none()
                            && r.session_id.is_none()
                            && r.query.is_none()
                            && r.literal_terms.is_empty()
                    }
                    ObservationKind::EventMemoryRefs => {
                        r.event_id.is_some()
                            && r.observation_id.is_none()
                            && r.memory_id.is_none()
                            && r.session_id.is_none()
                            && r.query.is_none()
                            && r.literal_terms.is_empty()
                    }
                };
                if !valid {
                    return Err("invalid_variant: observations fields do not apply to kind".into());
                }
            }
            Self::SessionChoices(r) if r.limit.is_some_and(|n| n > 3) => {
                return fail("session_choices limit must be 1..3");
            }
            Self::PendingWork(r) => {
                if r.host_kind.as_ref().is_some_and(|s| s.len() > 256)
                    || r.host_thread_id.as_ref().is_some_and(|s| s.len() > 256)
                {
                    return fail("pending work host identity");
                }
                if r.unbound_only && r.session_id.is_some() {
                    return Err("invalid_variant: unbound_only conflicts with session_id".into());
                }
            }
            _ => {}
        }
        Ok(())
    }
}
