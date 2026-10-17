//! One owner for whole-item admission, continuation and exact packet bytes.
use super::{
    records::*,
    requests::{NextCall, ReadRef, Request},
};
use crate::learner::model::SchemaVersion;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub(super) fn size<T: Serialize>(value: &T) -> usize {
    serde_json::to_vec(value)
        .map(|v| v.len())
        .unwrap_or(usize::MAX)
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "consistency", rename_all = "snake_case", deny_unknown_fields)]
pub enum Dependencies {
    Snapshot {
        #[serde(skip_serializing_if = "Option::is_none")]
        memory_generation: Option<i64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        observation_generation: Option<i64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        review_generation: Option<i64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        event_generation: Option<i64>,
    },
    Live,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PageKey {
    Number(i64),
    Text(String),
    Pending(PendingKey),
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PendingKey {
    pub event_no: i64,
    pub host_kind: String,
    pub host_thread_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Cursor {
    pub schema: SchemaVersion,
    pub workspace_id: String,
    pub filter_hash: String,
    pub dependencies: Dependencies,
    pub last: PageKey,
    pub checksum: String,
}
#[derive(Clone)]
pub(super) struct CursorScope {
    request: Request,
    workspace: String,
    filter: String,
    dependencies: Dependencies,
}
impl CursorScope {
    pub fn new(request: &Request, workspace: &str, dependencies: Dependencies) -> Self {
        Self {
            request: request.clone(),
            workspace: workspace.into(),
            filter: hex(&Sha256::digest(
                request.normalized_identity().to_string().as_bytes(),
            )),
            dependencies,
        }
    }
    pub fn retarget(&self, request: &Request) -> Self {
        Self::new(request, &self.workspace, self.dependencies.clone())
    }
    fn next_call(&self, cursor: Cursor) -> NextCall {
        self.request.next_call(cursor)
    }
    fn checksum(workspace: &str, filter: &str, deps: &Dependencies, last: &PageKey) -> String {
        hex(&Sha256::digest(
            serde_json::to_vec(&(crate::learner::SCHEMA, workspace, filter, deps, last)).unwrap(),
        ))
    }
    pub fn cursor(&self, last: PageKey) -> Cursor {
        Cursor {
            schema: SchemaVersion::V2,
            workspace_id: self.workspace.clone(),
            filter_hash: self.filter.clone(),
            checksum: Self::checksum(&self.workspace, &self.filter, &self.dependencies, &last),
            dependencies: self.dependencies.clone(),
            last,
        }
    }
    pub fn after(&self, request: &Request) -> Result<Option<PageKey>, String> {
        let Some(cursor) = request.cursor() else {
            return Ok(None);
        };
        if cursor.workspace_id != self.workspace
            || cursor.filter_hash != self.filter
            || cursor.checksum
                != Self::checksum(
                    &cursor.workspace_id,
                    &cursor.filter_hash,
                    &cursor.dependencies,
                    &cursor.last,
                )
        {
            return Err("invalid_cursor: workspace/action/filter mismatch".into());
        }
        if cursor.dependencies != self.dependencies {
            return Err("cursor_stale: restart this action without cursor".into());
        }
        Ok(Some(cursor.last.clone()))
    }
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct Coverage {
    pub active_memory_count: i64,
    pub catalog_complete: bool,
    pub index_current: bool,
    pub candidate_lists_truncated: bool,
    pub ranked_retrieval_used: bool,
    pub omitted_card_count: usize,
    pub semantic_completeness: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_execution_complete: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_queue_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_queue_complete: Option<bool>,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct Packet {
    pub schema: SchemaVersion,
    pub workspace_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_generation: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation_generation: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_generation: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_generation: Option<i64>,
    pub selection_mode: String,
    pub session: Option<Session>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint_basis: Option<CheckpointBasis>,
    pub session_choices: Vec<SessionChoice>,
    pub revision_notices: Vec<RevisionNotice>,
    pub memories: Vec<MemoryCard>,
    pub coverage: Coverage,
    pub continuations: Vec<NextCall>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub omitted_memory_refs: Vec<ReadRef>,
    pub hints: Vec<String>,
    pub pending_event_refs: Vec<PendingRef>,
    pub warnings: Vec<String>,
    pub next_cursor: Option<Cursor>,
    pub next_call: Option<NextCall>,
    pub trace_id: Option<String>,
    pub trace_status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_consistency: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_evidence: Option<PendingNotice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_choices_more: Option<NextCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_events_more: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_events_continuation: Option<NextCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_wording_intent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub records: Option<Vec<Record>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topics: Option<Vec<Topic>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history: Option<Vec<HistoricalChange>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_ref: Option<CurrentMemoryRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reviews: Option<Vec<Review>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observations: Option<Vec<Record>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub support: Option<Vec<Record>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_work: Option<Vec<PendingWork>>,
    #[serde(flatten)]
    pub status: Option<Status>,
}
impl Packet {
    pub fn new(
        state: &WorkspaceState,
        action: &str,
        active: i64,
        index_current: bool,
        trace: String,
        live: bool,
    ) -> Self {
        Self {
            schema: SchemaVersion::V2,
            workspace_id: state.workspace_id.clone(),
            memory_generation: (!live).then_some(state.memory_generation),
            observation_generation: (!live).then_some(state.observation_generation),
            review_generation: (!live).then_some(state.review_generation),
            event_generation: (!live).then_some(state.event_generation),
            selection_mode: action.into(),
            session: None,
            checkpoint_basis: None,
            session_choices: vec![],
            revision_notices: vec![],
            memories: vec![],
            coverage: Coverage {
                active_memory_count: active,
                catalog_complete: false,
                index_current,
                candidate_lists_truncated: false,
                ranked_retrieval_used: false,
                omitted_card_count: 0,
                semantic_completeness: "not_claimed",
                query_execution_complete: None,
                review_queue_count: None,
                review_queue_complete: None,
            },
            continuations: vec![],
            omitted_memory_refs: vec![],
            hints: vec![],
            pending_event_refs: vec![],
            warnings: vec![],
            next_cursor: None,
            next_call: None,
            trace_id: Some(trace),
            trace_status: "unavailable",
            page_consistency: live.then_some("live"),
            pending_evidence: None,
            session_choices_more: None,
            pending_events_more: None,
            pending_events_continuation: None,
            original_wording_intent: None,
            page_count: None,
            records: None,
            topics: None,
            history: None,
            current_ref: None,
            reviews: None,
            observations: None,
            support: None,
            status: None,
            pending_work: None,
        }
    }
}
#[derive(Clone)]
pub(super) enum Item {
    Memory(MemoryCard),
    Topic(Topic),
    History(HistoricalChange),
    Review(Review),
    Observation(Record),
    Support(Record),
    SessionChoice(SessionChoice),
    Pending(PendingRef),
    PendingWork(PendingWork),
}
#[derive(Clone)]
pub(super) struct PageItem {
    pub value: Item,
    pub after: PageKey,
}
pub(super) enum PageCoverage {
    None,
    Catalog(bool),
    Review(bool),
    Pending,
}
pub(super) enum Admission {
    Added,
    FullBeforeItem,
}
#[derive(Clone)]
pub(super) struct Builder {
    pub packet: Packet,
    pub max: usize,
}
impl Builder {
    pub fn new(packet: Packet, max: usize) -> Self {
        Self { packet, max }
    }
    pub fn required(&mut self, edit: impl FnOnce(&mut Packet)) -> Result<(), String> {
        let mut next = self.packet.clone();
        edit(&mut next);
        if size(&next) > self.max {
            return Err("budget_too_small: required whole records and packet metadata do not fit; increase max_bytes or narrow the read".into());
        }
        self.packet = next;
        Ok(())
    }
    pub fn optional(&mut self, edit: impl FnOnce(&mut Packet)) -> bool {
        let mut next = self.packet.clone();
        edit(&mut next);
        if size(&next) > self.max {
            return false;
        }
        self.packet = next;
        true
    }
    fn append_page_item(
        next: &mut Packet,
        item: PageItem,
        more_after_item: bool,
        scope: &CursorScope,
        coverage: PageCoverage,
    ) {
        let count = match item.value {
            Item::Memory(v) => {
                if !next.memories.iter().any(|c| c.memory_id == v.memory_id) {
                    next.memories.push(v);
                }
                next.page_count.unwrap_or(0) + 1
            }
            Item::Topic(v) => {
                let rows = next.topics.get_or_insert_default();
                rows.push(v);
                rows.len()
            }
            Item::History(v) => {
                let rows = next.history.get_or_insert_default();
                rows.push(v);
                rows.len()
            }
            Item::Review(v) => {
                let rows = next.reviews.get_or_insert_default();
                rows.push(v);
                rows.len()
            }
            Item::Observation(v) => {
                let rows = next.observations.get_or_insert_default();
                rows.push(v);
                rows.len()
            }
            Item::Support(v) => {
                let rows = next.support.get_or_insert_default();
                rows.push(v);
                rows.len()
            }
            Item::SessionChoice(v) => {
                next.session_choices.push(v);
                next.session_choices.len()
            }
            Item::PendingWork(v) => {
                let rows = next.pending_work.get_or_insert_default();
                rows.push(v);
                rows.len()
            }
            Item::Pending(v) => {
                next.pending_event_refs.push(v);
                next.pending_event_refs.len()
            }
        };
        next.page_count = Some(count);
        next.next_cursor = more_after_item.then(|| scope.cursor(item.after));
        next.next_call = next
            .next_cursor
            .clone()
            .map(|cursor| scope.next_call(cursor));
        match coverage {
            PageCoverage::Catalog(from_start) => {
                next.coverage.catalog_complete = from_start && !more_after_item
            }
            PageCoverage::Review(from_start) => {
                next.coverage.review_queue_complete = Some(from_start && !more_after_item)
            }
            PageCoverage::Pending => next.pending_events_more = Some(more_after_item),
            PageCoverage::None => {}
        }
    }
    pub fn try_push(
        &mut self,
        item: PageItem,
        more_after_item: bool,
        scope: &CursorScope,
        coverage: PageCoverage,
    ) -> Admission {
        let mut next = self.packet.clone();
        Self::append_page_item(&mut next, item, more_after_item, scope, coverage);
        if size(&next) > self.max {
            Admission::FullBeforeItem
        } else {
            self.packet = next;
            Admission::Added
        }
    }
    pub fn whole_page(
        &mut self,
        rows: Vec<PageItem>,
        limit: usize,
        scope: &CursorScope,
        coverage: impl Fn() -> PageCoverage,
    ) -> Result<(), String> {
        let total = rows.len();
        // A known final page is admitted atomically with its actual null cursor.
        // Intermediate cursor overhead must not reject an exactly fitting final
        // packet containing all remaining whole items.
        if total <= limit {
            let mut complete = self.packet.clone();
            for (index, row) in rows.iter().cloned().enumerate() {
                Self::append_page_item(&mut complete, row, total > index + 1, scope, coverage());
            }
            if size(&complete) <= self.max {
                self.packet = complete;
                return Ok(());
            }
        }
        let mut emitted = 0;
        for (index, row) in rows.into_iter().take(limit).enumerate() {
            match self.try_push(row,total>index+1,scope,coverage()) {Admission::Added=>emitted+=1,Admission::FullBeforeItem if emitted==0=>return Err("budget_too_small: next whole item and continuation do not fit; increase max_bytes".into()),Admission::FullBeforeItem=>break}
        }
        // The prior admission reserved its continuation whenever any row remained,
        // including a later row that fails admission. No rejected key is committed.
        self.check()
    }
    pub fn preview_choices(
        &mut self,
        rows: Vec<SessionChoice>,
        scope: &CursorScope,
    ) -> Result<(), String> {
        if rows.is_empty() {
            return Ok(());
        }
        if rows.len() <= 3
            && self.optional(|p| {
                p.session_choices = rows.clone();
                p.session_choices_more = None;
            })
        {
            return Ok(());
        }
        self.required(|p| p.session_choices_more = Some(NextCall::session_choices(None)))?;
        let total = rows.len();
        for (index, row) in rows.into_iter().take(3).enumerate() {
            let more = total > index + 1;
            let after = PageKey::Text(row.session_id.clone());
            if !self.optional(|p| {
                p.session_choices.push(row);
                p.session_choices_more =
                    more.then(|| NextCall::session_choices(Some(scope.cursor(after))));
            }) {
                break;
            }
        }
        Ok(())
    }
    pub fn preview_pending(&mut self, events: Vec<String>, session: &str) -> Result<(), String> {
        if events.len() <= 32
            && self.optional(|p| {
                p.pending_event_refs = events.iter().cloned().map(PendingRef::Id).collect();
                p.pending_events_more = Some(false);
                p.pending_events_continuation = None;
            })
        {
            return Ok(());
        }
        self.required(|p| {
            p.pending_events_more = Some(!events.is_empty());
            p.pending_events_continuation =
                (!events.is_empty()).then(|| NextCall::pending_events(session));
        })?;
        let total = events.len();
        for (index, id) in events.into_iter().take(32).enumerate() {
            let more = total > index + 1;
            if !self.optional(|p| {
                p.pending_event_refs.push(PendingRef::Id(id));
                p.pending_events_more = Some(more);
                if !more {
                    p.pending_events_continuation = None;
                }
            }) {
                break;
            }
        }
        Ok(())
    }
    pub fn check(&self) -> Result<(), String> {
        if size(&self.packet) > self.max {
            Err(
                "budget_too_small: required packet envelope does not fit; increase max_bytes"
                    .into(),
            )
        } else {
            Ok(())
        }
    }
    pub fn finish(self) -> Result<Packet, String> {
        self.check()?;
        Ok(self.packet)
    }
}
