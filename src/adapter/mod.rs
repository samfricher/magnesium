pub(crate) mod control_admission;
pub(crate) mod control_safety;
pub(crate) mod core_rules;
use crate::workspace::Workspace;
#[cfg(test)]
use control_admission::{
    LocalSuppression, control_event, control_storage_failure, recording_suppressed,
};
use std::borrow::Cow;
use std::path::Path;
pub(crate) mod cli;
pub(crate) mod helpers;
pub(crate) mod host;
pub(crate) mod native_capture;
mod reply_check;
pub(crate) mod search;
pub(crate) mod sources;
pub(crate) mod types;
pub(crate) use helpers::resolve_workspace;
pub(crate) use host::{HostIdentity, HostKind, check_workspace_binding};
pub(crate) use search::handle_search;
pub(crate) use sources::{PdfCopyError, handle_sources, read_authorized_pdf, read_bounded_bytes};
pub(crate) use types::*;
pub(crate) const SUPPORTED_PROTOCOL_VERSION: u64 = 2;
pub(crate) const INSTRUCTIONS_FILENAME: &str = "INSTRUCTIONS.md";
pub(crate) const HARNESS_INSTRUCTIONS: &str = include_str!("../workspace/instructions.md");
pub(crate) const TEACHING_POLICY_VERSION: &str = "teaching-policy/v2";
const MAX_VERSION_BYTES: usize = 128;
pub(crate) const MAX_TURN_ID_BYTES: usize = 256;
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InvocationIdentity {
    pub(crate) plugin_version: Option<String>,
    pub(crate) protocol_version: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CompatibilityError {
    pub(crate) reason: &'static str,
    pub(crate) detail: String,
}

pub(crate) fn validate(identity: &InvocationIdentity) -> Result<(), CompatibilityError> {
    let plugin_version = identity.plugin_version.as_deref().ok_or_else(|| {
        error(
            "missing_plugin_version",
            "the Magnesium plugin did not provide its version",
        )
    })?;
    validate_version(plugin_version).map_err(|detail| CompatibilityError {
        reason: "invalid_plugin_version",
        detail,
    })?;
    if !compatible_version(plugin_version) {
        return Err(error(
            "plugin_version_mismatch",
            format!(
                "plugin version {plugin_version} is not compatible with runtime {}",
                env!("CARGO_PKG_VERSION")
            ),
        ));
    }

    let protocol = identity.protocol_version.as_deref().ok_or_else(|| {
        error(
            "missing_adapter_protocol_version",
            "the Magnesium plugin did not provide an adapter protocol version",
        )
    })?;
    if protocol != SUPPORTED_PROTOCOL_VERSION.to_string() {
        return Err(error(
            "adapter_protocol_mismatch",
            format!(
                "adapter protocol {protocol} is not supported; runtime supports {}",
                SUPPORTED_PROTOCOL_VERSION
            ),
        ));
    }
    Ok(())
}

pub(crate) fn compatible_plugin_version(version: &str) -> bool {
    validate_version(version).is_ok() && compatible_version(version)
}

fn compatible_version(version: &str) -> bool {
    let (base, build) = version.split_once('+').unwrap_or((version, ""));
    base == env!("CARGO_PKG_VERSION") && (build.is_empty() || valid_cachebuster(build))
}

fn validate_version(version: &str) -> Result<(), String> {
    if version.is_empty() || version.len() > MAX_VERSION_BYTES {
        return Err(format!(
            "plugin version is outside the {MAX_VERSION_BYTES}-byte bound"
        ));
    }
    let (without_build, build) = match version.split_once('+') {
        Some((base, build)) => (base, Some(build)),
        None => (version, None),
    };
    let (core, prerelease) = match without_build.split_once('-') {
        Some((core, prerelease)) => (core, Some(prerelease)),
        None => (without_build, None),
    };
    let components = core.split('.').collect::<Vec<_>>();
    if components.len() != 3 || components.iter().any(|part| !valid_numeric(part)) {
        return Err(format!(
            "plugin version {version} is not bounded semantic version text"
        ));
    }
    if let Some(prerelease) = prerelease
        && (prerelease.is_empty() || !prerelease.split('.').all(valid_identifier))
    {
        return Err(format!(
            "plugin version {version} has malformed prerelease metadata"
        ));
    }
    if let Some(build) = build
        && (build.is_empty() || !build.split('.').all(valid_identifier))
    {
        return Err(format!(
            "plugin version {version} has malformed build metadata"
        ));
    }
    Ok(())
}

fn valid_cachebuster(build: &str) -> bool {
    let Some(token) = build.strip_prefix("codex.") else {
        return false;
    };
    !token.is_empty() && token.split('.').all(valid_identifier)
}

fn valid_numeric(value: &str) -> bool {
    !value.is_empty()
        && (value == "0" || !value.starts_with('0'))
        && value.bytes().all(|b| b.is_ascii_digit())
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn error(reason: &'static str, detail: impl Into<String>) -> CompatibilityError {
    CompatibilityError {
        reason,
        detail: detail.into(),
    }
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

// The same runtime protocol types describe the tool contract.
// Updates retain an independent raw-capture boundary before semantic decoding.
#[allow(dead_code)]
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub(crate) enum LearningStepRequest {
    Update(Box<crate::learner::UpdateRequest>),
    Control(Box<ControlRequest>),
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ControlRequest {
    pub schema: crate::learner::SchemaVersion,
    pub action: control_admission::ControlTag,
    pub request_id: String,
    pub control: control_admission::ControlAction,
    pub turn_ref: Option<String>,
    pub reported_event: Option<crate::learner::RawEvent>,
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub(crate) enum InstructionAction {
    #[serde(rename = "instructions")]
    Instructions,
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstructionRequest {
    pub action: InstructionAction,
    pub stage: crate::workspace::instruction_bundle::InstructionStage,
    pub host: crate::workspace::instruction_bundle::InstructionHost,
    pub operation: Option<crate::workspace::instruction_bundle::InstructionOperation>,
}
// Runtime contracts for instruction metadata and typed learner reads.
#[allow(dead_code)]
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub(crate) enum ContextRequest {
    Learner(Box<crate::learner::read::ContextReadRequest>),
    Instructions(InstructionRequest),
}

pub(crate) fn host_context(host: &HostIdentity) -> crate::learner::HostContext {
    crate::learner::HostContext {
        host_kind: host.kind().as_str().into(),
        host_thread_id: host.stored_session_id(),
    }
}
/// `context` serves two request shapes; a tutor that mixes them gets both in
/// full rather than one missing field per retry.
const CONTEXT_SHAPES: &str = r#"context takes either an instruction load {"action":"instructions","host":"claude"|"codex"|"opencode","stage":"orient_and_probe"|"plan_and_route"|"teach_and_practice"|"interpret_and_repair"|"review_and_transfer"|"consolidate_and_resume"|"papers"|"papers_loop"} with optional "operation":"state_and_evidence"|"source_use"|"tool_recipes", or a learner-state read {"schema":"learner-model/v2","action":"status"|"prepare"|"session_choices"|"pending_events",...} without host or stage; with no action, a stage loads instructions and a goal prepares"#;
const MEMORY_SHAPES: &str = r#"memory takes {"schema":"learner-model/v2","action":"search","goal":"<topic>","original_query":"<learner's words>"}, {"schema":"learner-model/v2","action":"read","refs":[{"kind":"current_memory"|"observation"|"event","id":"<id>"}]}, or {"schema":"learner-model/v2","action":"browse"} with optional "topic"; other actions are topics, history, reviews, observations and pending_work"#;
/// Native process environment is adapter context, not a model-supplied identity.
/// A tool-only connection gets its own ephemeral thread; it never borrows a
/// previous conversation's bound session merely because the workspace matches.
fn tool_host() -> crate::learner::HostContext {
    if let Some(host) = host::process_host_identity() {
        return host_context(&host);
    }
    static THREAD: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    crate::learner::HostContext {
        host_kind: "tool_only".into(),
        host_thread_id: THREAD
            .get_or_init(|| crate::learner::store::new_id("tool").expect("host identity entropy"))
            .clone(),
    }
}
/// Tutors often call `context` with only a stage or only a goal. A stage
/// means "load that stage's instructions" and a goal means "prepare", so fill
/// in the action, and the host or schema the runtime already knows, instead of
/// failing the call. An explicit action is never changed.
fn complete_context_request(
    mut request: serde_json::Value,
    native_host: Option<&crate::learner::HostContext>,
) -> serde_json::Value {
    let Some(fields) = request.as_object_mut() else {
        return request;
    };
    if !fields.contains_key("action") {
        if fields.contains_key("stage") {
            fields.insert("action".into(), "instructions".into());
        } else if fields.contains_key("goal") {
            fields.insert("action".into(), "prepare".into());
            fields
                .entry("schema")
                .or_insert_with(|| crate::learner::SCHEMA.into());
        }
    }
    if fields.get("action").and_then(serde_json::Value::as_str) == Some("instructions")
        && !fields.contains_key("host")
    {
        let kind = native_host
            .and_then(|host| HostKind::parse(&host.host_kind).ok())
            .or_else(|| host::process_host_identity().map(|host| host.kind()))
            .or_else(host::launch_host);
        if let Some(kind) = kind {
            fields.insert("host".into(), kind.as_str().into());
        }
    }
    request
}
fn open_store(workspace: &Workspace) -> Result<crate::learner::LearnerStore, String> {
    crate::learner::LearnerStore::open(
        &workspace.store_path(),
        &workspace.id().map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
pub(crate) fn dispatch(
    cwd: &Path,
    operation: &str,
    request: serde_json::Value,
) -> Result<serde_json::Value, String> {
    dispatch_with_host(cwd, operation, request, None)
}
/// The optional identity is supplied only by trusted native transport metadata.
/// It is never read from model-selected tool arguments.
pub(crate) fn dispatch_with_host(
    cwd: &Path,
    operation: &str,
    request: serde_json::Value,
    native_host: Option<crate::learner::HostContext>,
) -> Result<serde_json::Value, String> {
    let caller = native_host
        .clone()
        .or_else(|| host::process_host_identity().map(|host| host_context(&host)));
    let codex_thread = caller
        .as_ref()
        .filter(|host| host.host_kind == HostKind::Codex.as_str())
        .map(|host| host.host_thread_id.clone());
    if let Some(refusal) = due_checks_first(cwd, operation, &request, caller.as_ref()) {
        return Ok(refusal);
    }
    // A sittings store from before #30 moves its quick checks into the
    // learner store before an update can make a check on the same label.
    if operation == "learning_step" {
        let _ = sittings_here(cwd, false);
    }
    let update = (operation == "learning_step").then(|| request.clone());
    let paper_stage = operation == "context"
        && matches!(
            request.get("stage").and_then(serde_json::Value::as_str),
            Some("papers" | "papers_loop")
        );
    let mut result = dispatch_inner(cwd, operation, request, native_host)?;
    if let Some(thread) = codex_thread {
        deliver_owed_core_rules(cwd, &thread, &mut result);
    }
    open_sitting(
        cwd,
        update.as_ref(),
        paper_stage,
        caller.as_ref(),
        &mut result,
    );
    Ok(result)
}
/// The study folder's sittings, when this call runs in one it is bound to.
/// The store is created only for a write, so a read changes no file.
fn sittings_here(cwd: &Path, write: bool) -> Option<(Workspace, crate::sittings::Sittings)> {
    let workspace = Workspace::discover(cwd).ok()??;
    check_workspace_binding(&workspace).ok()?;
    let sittings = if write {
        crate::sittings::Sittings::open(&workspace).ok()?
    } else {
        crate::sittings::Sittings::open_existing(&workspace).ok()??
    };
    Some((workspace, sittings))
}
/// After a committed update, its learner input notes a learner message, and
/// what its attempts did to practice items goes in the result and settles
/// the sitting's checks. A practice change, or a paper stage load, brings in
/// and writes the gap logs. The first tool result of a sitting then carries
/// the opener. Last, the result states the help levels and readings the
/// update or load concerns. None of this can fail the call.
fn open_sitting(
    cwd: &Path,
    update: Option<&serde_json::Value>,
    paper_stage: bool,
    caller: Option<&crate::learner::HostContext>,
    result: &mut serde_json::Value,
) {
    if !result.is_object() {
        return;
    }
    let Ok(Some(workspace)) = Workspace::discover(cwd) else {
        return;
    };
    if check_workspace_binding(&workspace).is_err() {
        return;
    }
    let now = crate::sittings::now_ms();
    let today = crate::sittings::local_day(now);
    let committed = update.filter(|_| result["update_status"] == "committed");
    let practice_change = committed.is_some_and(touches_practice);
    // Bringing a gap log in copies the learner's rows into the learner
    // store, so it waits while this conversation records nothing.
    let records = caller.is_none_or(|host| records_here(&workspace, host));
    let mut report = crate::practice::GapLogReport::default();
    if records && (paper_stage || practice_change) {
        report = crate::practice::sync_gap_logs(&workspace, today);
    }
    let mut outcomes = Vec::new();
    if let Some(request) = committed.filter(|_| practice_change)
        && let Ok(store) = crate::practice::open_store(&workspace)
        && let Ok(items) = crate::practice::snapshot(&store.conn, workspace.root())
    {
        let effects = crate::practice::attempt_effects(&store.conn, &items, request, result);
        if let Some(summary) =
            crate::practice::update_summary(&items, &effects, request, &store.conn)
        {
            result["practice"] = summary;
        }
        let chat = |id: &str| {
            items.iter().any(|item| {
                item.facts.item_id == id
                    && item.facts.origin == crate::learner::practice::Origin::Chat
            })
        };
        for attempt in &effects {
            for (item, effect) in &attempt.effects {
                if chat(item)
                    && let Some(outcome) = crate::sittings::outcome(effect)
                {
                    outcomes.push((item.clone(), outcome));
                }
            }
        }
        if has_chat_attempt(request) {
            result["quick_checks"] = serde_json::json!({
                "open_labels": crate::practice::open_labels(&items),
                "note": "One quick check per misunderstanding. When an attempt shows the same misunderstanding as one of these, even in another form or another step, reuse its label exactly. Make a new label only for a different misunderstanding.",
            });
        }
    }
    let message =
        update.is_some_and(|request| crate::sittings::carries_learner_message(request, result));
    if let Some((workspace, mut sittings)) = sittings_here(cwd, message || !outcomes.is_empty()) {
        if message {
            let _ = sittings.note_learner_message(&workspace, now);
        }
        let _ = sittings.settle(&outcomes);
        if records && workspace.opener() != crate::workspace::OpenerMode::Off {
            let mut synced = crate::practice::GapLogReport::default();
            if let Ok(Some(opener)) = sittings.take_opener(&workspace, now, &mut synced) {
                result["sitting_opener"] = opener.into();
            }
            report.merge(synced);
        }
    }
    if let Some(gap_logs) = report.to_json() {
        result["gap_logs"] = gap_logs;
    }
    crate::readings::add_to_result(&workspace, committed, practice_change, paper_stage, result);
}
/// Whether this conversation records: no control latch denies capture, and
/// recording is on for it.
fn records_here(workspace: &Workspace, host: &crate::learner::HostContext) -> bool {
    let allowed = control_admission::Admission::new(workspace, host)
        .capture_guard()
        .is_ok_and(|guard| guard.blocked.is_none());
    allowed
        && open_store(workspace)
            .and_then(|store| store.binding(host).map_err(|e| e.to_string()))
            .is_ok_and(|(_, _, recording)| recording)
}
/// An update that records an attempt, or changes or corrects a practice
/// item.
fn touches_practice(request: &serde_json::Value) -> bool {
    let nonempty = |field: &str| {
        request[field]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    };
    nonempty("practice")
        || nonempty("attempt_corrections")
        || request["observations"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|o| {
                o.get("attempt").is_some_and(|a| !a.is_null())
                    || o["attempts"].as_array().is_some_and(|a| !a.is_empty())
            })
}
/// An answer in chat: an attempt with no paper that names no item, so it
/// makes or answers the quick check on its label.
fn has_chat_attempt(request: &serde_json::Value) -> bool {
    request["observations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|o| o.get("attempt").filter(|a| a.is_object()))
        .any(|a| {
            a.get("paper").is_none_or(serde_json::Value::is_null)
                && a["counts_for"].as_array().is_none_or(Vec::is_empty)
        })
}
/// With the opener's block on, a stage that brings new material is refused
/// while a quick check of this sitting is neither posed nor declined.
fn due_checks_first(
    cwd: &Path,
    operation: &str,
    request: &serde_json::Value,
    caller: Option<&crate::learner::HostContext>,
) -> Option<serde_json::Value> {
    let stage = request.get("stage")?.as_str()?;
    if operation != "context"
        || request
            .get("action")
            .is_some_and(|action| action != "instructions")
        || !matches!(
            stage,
            "orient_and_probe" | "plan_and_route" | "teach_and_practice"
        )
    {
        return None;
    }
    let (workspace, mut sittings) = sittings_here(cwd, false)?;
    if workspace.opener() != crate::workspace::OpenerMode::Block {
        return None;
    }
    let counts = sittings.open_counts().ok()??;
    let now = crate::sittings::now_ms();
    let mut synced = crate::practice::GapLogReport::default();
    // Claiming the opener brings gap logs in, which waits while this
    // conversation records nothing.
    let records = caller.is_none_or(|host| records_here(&workspace, host));
    let claimed = if records {
        sittings
            .take_opener(&workspace, now, &mut synced)
            .ok()
            .flatten()
    } else {
        None
    };
    let opener = claimed.or_else(|| sittings.opener_now(&workspace, now).ok().flatten());
    let mut refusal = serde_json::json!({
        "status": "unavailable",
        "recommended_stage": stage,
        "error": {
            "code": "due_checks_first",
            "message": format!(
                "This study folder opens each sitting with its quick checks before new material: due {}, posed {}. Pose each one, or record the learner's decline, then load this stage again. Stages review_and_transfer, interpret_and_repair, consolidate_and_resume, papers and papers_loop stay open.",
                counts.due, counts.posed
            ),
        },
    });
    if let Some(opener) = opener {
        refusal["sitting_opener"] = opener.into();
    }
    if let Some(gap_logs) = synced.to_json() {
        refusal["gap_logs"] = gap_logs;
    }
    Some(refusal)
}
/// After a Codex compaction, the first tool result in that conversation
/// carries the core rules. An instruction load already holds them in
/// `MAGNESIUM.md`, so it only settles the debt.
fn deliver_owed_core_rules(cwd: &Path, thread: &str, result: &mut serde_json::Value) {
    let Some(fields) = result.as_object_mut() else {
        return;
    };
    let Ok(Some(workspace)) = Workspace::discover(cwd) else {
        return;
    };
    if check_workspace_binding(&workspace).is_err()
        || !host::take_core_rules_owed(&workspace, thread).unwrap_or(false)
    {
        return;
    }
    let loaded_orchestrator = fields
        .get("files")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|files| files.iter().any(|file| file == "MAGNESIUM.md"));
    if !loaded_orchestrator {
        fields.insert("core_rules".into(), core_rules::text().into());
    }
}
fn dispatch_inner(
    cwd: &Path,
    operation: &str,
    request: serde_json::Value,
    native_host: Option<crate::learner::HostContext>,
) -> Result<serde_json::Value, String> {
    use serde_json::json;
    if operation == "learning_step"
        && serde_json::to_vec(&request)
            .map_err(|e| e.to_string())?
            .len()
            > crate::learner::limits::REQUEST_BYTES
    {
        return Err("field_too_large: semantic request exceeds 1 MiB".into());
    }
    let request = if operation == "context" {
        complete_context_request(request, native_host.as_ref())
    } else {
        request
    };
    if operation == "context" && request["action"] == "instructions" {
        let request: InstructionRequest =
            serde_json::from_value(request).map_err(|e| format!("{e}. {CONTEXT_SHAPES}"))?;
        let Some(control) = crate::workspace::instruction_bundle::resolve_authorized_control(cwd)?
        else {
            return Ok(json!({"status":"unavailable","error":{"code":"inactive_workspace"}}));
        };
        return Ok(crate::workspace::instruction_bundle::load(
            &control,
            request.stage,
            request.host,
            request.operation,
            "instruction_only",
        ));
    }
    if operation == "search_sources" {
        return handle_search(
            cwd,
            &serde_json::from_value(request).map_err(|e| e.to_string())?,
        );
    }
    if operation == "sources" {
        return handle_sources(
            cwd,
            &serde_json::from_value(request).map_err(|e| e.to_string())?,
        );
    }
    if !matches!(operation, "context" | "memory" | "learning_step") {
        return Err(format!("unknown tool: {operation}"));
    }
    let host = native_host.unwrap_or_else(tool_host);
    let resolved = match resolve_workspace(cwd) {
        Ok(value) => value,
        Err(error) => {
            return control_admission::storage_failure(cwd, &host, operation, &request, error);
        }
    };
    let Some(resolved) = resolved else {
        return Ok(json!({"schema":crate::learner::SCHEMA,"active":false,"outcome":"inactive"}));
    };
    // A Claude MCP server keeps its spawn-time session after /clear or an
    // in-process /resume; refuse rather than act on the conversation it left.
    if host::launch_host() == Some(host::HostKind::Claude)
        && host.host_kind == "claude"
        && host::session_ended(resolved.workspace(), &host.host_thread_id)?
    {
        return Err("conversation_changed: this Claude Code conversation was cleared or switched after Magnesium's tool connection started, so Magnesium will not read or record learner state for it. Start a new Claude Code session, or resume this conversation, to continue the lesson.".into());
    }
    let store = match open_store(resolved.workspace()) {
        Ok(store) => store,
        Err(error) => {
            return control_admission::storage_failure(cwd, &host, operation, &request, error);
        }
    };
    match operation {
        "context" | "memory" => {
            use crate::learner::read::Tool;
            let tool = if operation == "context" {
                Tool::Context
            } else {
                Tool::Memory
            };
            let prepared = crate::learner::read::prepare_snapshot_for_tool(
                &store.conn,
                &host.host_kind,
                &host.host_thread_id,
                request,
                tool,
            )
            .map_err(|error| match operation {
                _ if !error.starts_with("invalid_variant") => error,
                "context" => format!("{error}. {CONTEXT_SHAPES}"),
                _ => format!("{error}. {MEMORY_SHAPES}"),
            })?;
            control_admission::Admission::new(resolved.workspace(), &host)
                .trace(&store, |allowed| {
                    crate::learner::read::finish_trace(&store.conn, prepared, allowed)
                })
        }
        _ => {
            let admission = control_admission::Admission::new(resolved.workspace(), &host);
            if request["action"] == "control" {
                admission.control(&store, request)
            } else {
                admission.update(&store, request)
            }
        }
    }
}
pub(crate) fn host_session_start(
    cwd: &Path,
    host: &HostIdentity,
) -> Result<Option<String>, String> {
    let Some(workspace) = Workspace::discover(cwd).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    check_workspace_binding(&workspace)?;
    let store = open_store(&workspace)?;
    let host = host_context(host);
    let mut notice = hook_notice(
        &store,
        &host,
        None,
        NoticeHook::SessionStart {
            core_rules: workspace.core_rules_on(),
        },
        None,
        None,
    )?;
    if host.host_kind == "claude" {
        let folder_has_lessons = store
            .conn
            .query_row("SELECT EXISTS(SELECT 1 FROM learning_sessions)", [], |r| {
                r.get::<_, bool>(0)
            })
            .map_err(|e| e.to_string())?;
        if first_greeting_on_this_machine(folder_has_lessons) {
            notice.push_str(CLAUDE_NEW_LEARNER_GUIDANCE);
        }
    }
    Ok(Some(notice))
}
/// `magnesium core-rules on|off` in the current study folder.
pub(crate) fn set_core_rules(on: bool) -> std::process::ExitCode {
    let result = std::env::current_dir()
        .and_then(|cwd| Workspace::discover(&cwd))
        .map_err(|error| error.to_string())
        .and_then(|workspace| {
            workspace.ok_or_else(|| {
                "no Magnesium study folder here; run `magnesium init` first".to_owned()
            })
        })
        .and_then(|workspace| {
            workspace
                .set_core_rules(on)
                .map_err(|error| error.to_string())
        });
    match result {
        Ok(()) if on => {
            println!(
                "Core rules on: after a compaction during a lesson, Magnesium puts its core teaching rules in front of the tutor."
            );
            std::process::ExitCode::SUCCESS
        }
        Ok(()) => {
            println!(
                "Core rules off: after a compaction during a lesson, Magnesium asks the tutor to reload its instructions, as before."
            );
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("magnesium: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
/// The core rules to restate after a compaction, when this study folder
/// keeps them on and this conversation has an active lesson.
pub(crate) fn core_rules_after_compaction(
    workspace: &Workspace,
    host: &HostIdentity,
) -> Result<Option<&'static str>, String> {
    check_workspace_binding(workspace)?;
    if !workspace.core_rules_on() {
        return Ok(None);
    }
    let store = open_store(workspace)?;
    let (_, active, _) = store
        .binding(&host_context(host))
        .map_err(|e| e.to_string())?;
    Ok(active.then(core_rules::text))
}
/// "New here?" is said once per machine, not once per study folder. The
/// marker lives in Magnesium's per-user data folder, which `magnesium init`
/// creates; this never creates that folder, so without it the hint falls back
/// to checking only this study folder.
fn first_greeting_on_this_machine(folder_has_lessons: bool) -> bool {
    let Ok(paths) = crate::workspace::GlobalPaths::resolve() else {
        return !folder_has_lessons;
    };
    let marker = paths.greeted_marker();
    if marker.exists() {
        return false;
    }
    if paths.installed() {
        // A failed write only means the hint may be repeated once more.
        let _ = crate::workspace::write_private_atomic(&marker, b"");
    }
    !folder_has_lessons
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NoticeHook {
    /// `core_rules` is this study folder's switch: on, the core rules replace
    /// the line that asks the tutor to reload its instructions.
    SessionStart {
        core_rules: bool,
    },
    UserPromptSubmit,
}
impl NoticeHook {
    /// Must match `additionalContextLimit` in the Claude Code hooks file.
    const fn additional_context_limit(self) -> usize {
        match self {
            Self::SessionStart { .. } => 4096,
            Self::UserPromptSubmit => 512,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Clause {
    Required,
    ConfusionReminder,
    TaskReminder,
    ReplyFeedback,
    Sitting,
    UnboundRecovery,
}
/// Over the limit, the state line first loses its context.prepare advice;
/// only then do clauses give way, in this order. The confusion reminder goes
/// before the pointers because a turn after a long reply is when it matters
/// most. The sitting and unbound-work pointers go only when nothing else is
/// left to drop; the state line still counts unbound work.
const GIVE_WAY_ORDER: [Clause; 5] = [
    Clause::TaskReminder,
    Clause::ReplyFeedback,
    Clause::ConfusionReminder,
    Clause::Sitting,
    Clause::UnboundRecovery,
];
const STATE_ADVICE: &str = " Read context.prepare on new conversation, compaction, changed goal or contradiction; learning_step revises memory without an assessment gate.";
fn hook_notice(
    store: &crate::learner::LearnerStore,
    host: &crate::learner::HostContext,
    event: Option<&str>,
    hook: NoticeHook,
    sitting: Option<&str>,
    closing: Option<&'static str>,
) -> Result<String, String> {
    let (_, active, recording) = store.binding(host).map_err(|e| e.to_string())?;
    let (generation,pending,unbound):(i64,i64,i64)=store.conn.query_row("SELECT memory_generation,(SELECT count(*) FROM learning_events WHERE host_kind=?1 AND host_thread_id=?2 AND processing='pending'),(SELECT count(*) FROM learning_events WHERE session_id IS NULL AND processing='pending') FROM workspace_meta",rusqlite::params![host.host_kind,host.host_thread_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
    let state = format!(
        "Magnesium learner-model/v2; generation {generation}; pending {pending}; unbound {unbound}; learning {active}; recording {recording}; current event {}.",
        event.unwrap_or("unavailable (use labeled reported events)")
    );
    let claude = host.host_kind == "claude";
    let mut clauses: Vec<(Clause, Cow<'static, str>)> = Vec::new();
    match (claude, active, recording) {
        (true, true, true) => {
            clauses.push((Clause::Required, CLAUDE_LESSON_GUIDANCE.into()));
            if hook == (NoticeHook::SessionStart { core_rules: false }) {
                clauses.push((Clause::Required, CLAUDE_RELOAD_GUIDANCE.into()));
            }
        }
        (true, false, true) => clauses.push((Clause::Required, CLAUDE_START_GUIDANCE.into())),
        _ => {}
    }
    // The lesson's teaching rules hold whether or not it is recording.
    if claude && active && hook == (NoticeHook::SessionStart { core_rules: true }) {
        clauses.push((
            Clause::Required,
            format!("\n\n{}", core_rules::text()).into(),
        ));
    }
    // While a quick check of this sitting is open, every turn says so.
    if let Some(sitting) = sitting {
        clauses.push((Clause::Sitting, sitting.to_owned().into()));
    }
    if unbound > 0 {
        let recovery = if claude {
            " Unbound work: context.pending_work unbound_only:true."
        } else {
            " Unbound pending work: context.pending_work with unbound_only:true."
        };
        clauses.push((Clause::UnboundRecovery, recovery.into()));
    }
    if let (true, true, NoticeHook::UserPromptSubmit) = (claude, active, hook) {
        clauses.push((Clause::TaskReminder, reply_check::task_reminder().into()));
        clauses.push((
            Clause::ConfusionReminder,
            reply_check::CONFUSION_REMINDER.into(),
        ));
        if let Some(feedback) = event.and_then(|event| {
            reply_check::feedback(&store.conn, host, event)
                .ok()
                .flatten()
        }) {
            clauses.push((Clause::ReplyFeedback, feedback.into()));
        }
    }
    if let Some(closing) = closing {
        clauses.push((Clause::Required, closing.into()));
    }
    Ok(fit(state, clauses, hook.additional_context_limit()))
}
fn fit(mut state: String, mut clauses: Vec<(Clause, Cow<'static, str>)>, limit: usize) -> String {
    let length = |state: &str, clauses: &[(Clause, Cow<'static, str>)]| {
        state.len() + clauses.iter().map(|(_, text)| text.len()).sum::<usize>()
    };
    if length(&state, &clauses) + STATE_ADVICE.len() <= limit {
        state.push_str(STATE_ADVICE);
    }
    for droppable in GIVE_WAY_ORDER {
        if length(&state, &clauses) <= limit {
            break;
        }
        clauses.retain(|(clause, _)| *clause != droppable);
    }
    clauses
        .iter()
        .fold(state, |notice, (_, text)| notice + text.as_ref())
}
const CLAUDE_LESSON_GUIDANCE: &str =
    " A lesson is active: interpret this learner turn with learning_step.";
const CLAUDE_RELOAD_GUIDANCE: &str =
    " After a new conversation or compaction, load the learn skill and its instructions first.";
const CLAUDE_START_GUIDANCE: &str = " Magnesium is set up in this folder: when the learner asks to learn, be taught, practise, be quizzed, review, plan their study or catch up, load the learn skill and start or resume a session; when they name an exam, midterm or final to prepare for, load the papers skill instead.";
const NATIVE_TICKET_FALLBACK: &str =
    " Native ticket unavailable; use labeled model-reported fallback.";
/// Only the Claude Code plugin ships `/magnesium:help`, so other hosts never get this line.
const CLAUDE_NEW_LEARNER_GUIDANCE: &str = " No learning session is recorded in this study folder, and no study folder on this machine has greeted the learner yet, so they are probably new to Magnesium: when it fits, tell them \"New here? Type /magnesium:help.\"";
pub(crate) fn host_user_prompt_submit(
    cwd: &Path,
    host: &HostIdentity,
    turn: Option<&str>,
    ordinal: Option<u64>,
    text: &str,
) -> Result<Option<String>, String> {
    let Some(workspace) = Workspace::discover(cwd).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    check_workspace_binding(&workspace)?;
    let context = host_context(host);
    let safety = control_admission::Admission::new(&workspace, &context).capture_guard()?;
    let store = open_store(&workspace)?;
    if safety.blocked.is_some() {
        let _ = native_capture::invalidate(&workspace, &context);
        return Ok(Some(if safety.blocked == Some(control_admission::CaptureBlock::SharedDeny) {
            "Magnesium learner-model/v2: capture denied by a shared control safety latch. Explicit recording_on or learning resume, as appropriate, is required."
        } else {
            "Magnesium learner-model/v2: capture locally suppressed after control storage failed; cross-process protection is unavailable. After storage recovers, explicitly use recording_on or session.start/session.resume in this same live tool connection."
        }.into()));
    }
    let key = match (turn, ordinal) {
        (Some(t), None) => host.stored_turn_id(t)?,
        (None, Some(n)) => host.stored_ordinal_turn_id(n),
        _ => return Err("unavailable_capture: ambiguous current turn".into()),
    };
    let event = crate::learner::RawEvent {
        external_event_key: native_capture::event_key(&key),
        speaker: crate::learner::Speaker::Learner,
        kind: "message".into(),
        text: native_capture::learner_text(&context, text).into_owned(),
        metadata: Some(crate::learner::EventMetadata::default()),
        occurred_at_ms: None,
    };
    let (_, active, recording) = store.binding(&context).map_err(|e| e.to_string())?;
    // A learner message in a conversation that records may begin a sitting.
    // Only its time is kept, never its text.
    if recording && let Ok(mut sittings) = crate::sittings::Sittings::open(&workspace) {
        let _ = sittings.note_learner_message(&workspace, crate::sittings::now_ms());
    }
    let sitting = crate::sittings::notice_clause(&workspace);
    if !active && recording {
        match native_capture::issue(
            &workspace,
            &context,
            &key,
            text,
            crate::learner::store::now_ms(),
        ) {
            Ok(ticket) => {
                let unbound:i64=store.conn.query_row("SELECT count(*) FROM learning_events WHERE session_id IS NULL AND processing='pending'",[],|r|r.get(0)).map_err(|e|e.to_string())?;
                let mut notice = native_ticket_notice(&ticket, unbound);
                if context.host_kind == "claude" {
                    notice.push_str(CLAUDE_START_GUIDANCE);
                }
                if let Some(sitting) = sitting.as_deref()
                    && notice.len() + sitting.len()
                        <= NoticeHook::UserPromptSubmit.additional_context_limit()
                {
                    notice.push_str(sitting);
                }
                return Ok(Some(notice));
            }
            Err(_) => {
                return Ok(Some(hook_notice(
                    &store,
                    &context,
                    None,
                    NoticeHook::UserPromptSubmit,
                    sitting.as_deref(),
                    Some(NATIVE_TICKET_FALLBACK),
                )?));
            }
        }
    }
    native_capture::invalidate(&workspace, &context)?;
    let ids = if active && recording {
        store
            .capture(&context, &[event], "host_observed", false)
            .map_err(|e| e.to_string())?
    } else {
        vec![]
    };
    Ok(Some(hook_notice(
        &store,
        &context,
        ids.first().map(String::as_str),
        NoticeHook::UserPromptSubmit,
        sitting.as_deref(),
        None,
    )?))
}
fn native_ticket_notice(ticket: &str, unbound: i64) -> String {
    // The complete signed ticket stays exact. A count beyond the compact
    // preview is explicitly labeled and can be read through pending_work.
    let count = if unbound > 9999 {
        "9999+".to_owned()
    } else {
        unbound.to_string()
    };
    let recovery = if unbound > 0 {
        "; context.pending_work unbound_only:true"
    } else {
        ""
    };
    let notice = format!(
        "learner-model/v2 native_capture.ticket={ticket}; {}:exact text; compaction:context.prepare; unbound:{count}{recovery}.",
        crate::learner::SessionEdit::CAPTURE_OPERATIONS.join("/")
    );
    debug_assert!(notice.len() <= 512);
    notice
}

pub(crate) fn host_response_stop_with_delivery(
    cwd: &Path,
    host: &HostIdentity,
    turn: &str,
    text: Option<&str>,
) -> Result<(), String> {
    let Some(text) = text else { return Ok(()) };
    let Some(workspace) = Workspace::discover(cwd).map_err(|e| e.to_string())? else {
        return Ok(());
    };
    check_workspace_binding(&workspace)?;
    let context = host_context(host);
    let safety = control_admission::Admission::new(&workspace, &context).capture_guard()?;
    if safety.blocked.is_some() {
        return Ok(());
    }
    // A reply that says "Quick check 2" poses that check. Only the fact is
    // kept, never the reply.
    if let Ok(Some(mut sittings)) = crate::sittings::Sittings::open_existing(&workspace) {
        let _ = sittings.note_reply(text);
    }
    let store = open_store(&workspace)?;
    let event = crate::learner::RawEvent {
        external_event_key: turn.into(),
        speaker: crate::learner::Speaker::Tutor,
        kind: "delivery".into(),
        text: text.into(),
        metadata: Some(crate::learner::EventMetadata {
            delivery: Some(crate::learner::Delivery::Observed),
            ..Default::default()
        }),
        occurred_at_ms: None,
    };
    let (_, active, recording) = store.binding(&context).map_err(|e| e.to_string())?;
    if active && recording {
        store
            .capture(&context, &[event], "host_observed", false)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    pub(super) fn fixture() -> (
        std::path::PathBuf,
        crate::learner::LearnerStore,
        HostIdentity,
    ) {
        let root = std::env::temp_dir().join(crate::learner::store::new_id("host-test").unwrap());
        std::fs::create_dir_all(root.join(crate::workspace::CONTROL_DIR)).unwrap();
        std::fs::write(
            root.join(crate::workspace::CONTROL_DIR).join("config.toml"),
            "schema_version = 1\nworkspace_id = \"host-test\"\n",
        )
        .unwrap();
        let store = crate::learner::LearnerStore::initialize(
            &root
                .join(crate::workspace::CONTROL_DIR)
                .join("state.sqlite"),
            "host-test",
        )
        .unwrap();
        (
            root,
            store,
            HostIdentity::codex("native-host-test").unwrap(),
        )
    }
    #[test]
    fn native_hooks_capture_only_active_work_deduplicate_turns_and_keep_raw_on_failed_interpretation()
     {
        let (root, store, host) = fixture();
        let context = host_context(&host);
        let notice =
            host_user_prompt_submit(&root, &host, Some("ordinary"), None, "private coding text")
                .unwrap()
                .unwrap();
        assert!(notice.contains("native_capture.ticket="));
        assert!(notice.len() <= 512);
        assert_eq!(
            store
                .conn
                .query_row("SELECT count(*) FROM learning_events", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        store.update(&context,json!({"schema":"learner-model/v2","action":"update","request_id":"start","session":{"operation":"start","client_ref":"s","goal":"Quadratic coefficients"}})).unwrap();
        let notice = host_user_prompt_submit(
            &root,
            &host,
            Some("one"),
            None,
            "I think b is another unknown.",
        )
        .unwrap()
        .unwrap();
        assert!(notice.contains("pending 1"));
        assert!(notice.len() <= 512);
        host_user_prompt_submit(
            &root,
            &host,
            Some("one"),
            None,
            "I think b is another unknown.",
        )
        .unwrap();
        host_user_prompt_submit(
            &root,
            &host,
            Some("two"),
            None,
            "I think b is another unknown.",
        )
        .unwrap();
        assert_eq!(
            store
                .conn
                .query_row(
                    "SELECT count(*) FROM learning_events WHERE origin='host_observed'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            2
        );
        assert!(store.update(&context,json!({"schema":"learner-model/v2","action":"update","request_id":"bad","memory_edits":[{"operation":"unsupported"}]})).is_err());
        assert_eq!(
            store
                .conn
                .query_row(
                    "SELECT count(*) FROM learning_events WHERE processing='pending'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            2
        );
        host_response_stop_with_delivery(
            &root,
            &host,
            "two",
            Some("b labels the fixed coefficient of x."),
        )
        .unwrap();
        let (speaker, origin): (String, String) = store
            .conn
            .query_row(
                "SELECT speaker,origin FROM learning_events WHERE kind='delivery'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            (speaker.as_str(), origin.as_str()),
            ("tutor", "host_observed")
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn recording_control_does_not_require_successful_memory_update() {
        let (root, store, host) = fixture();
        let context = host_context(&host);
        let event = crate::learner::RawEvent {
            external_event_key: "control-turn".into(),
            speaker: crate::learner::Speaker::Learner,
            kind: "message".into(),
            text: "Stop recording.".into(),
            metadata: Some(crate::learner::EventMetadata {
                origin: Some("model_reported".into()),
                ..Default::default()
            }),
            occurred_at_ms: None,
        };
        store
            .control(&context, "off", "recording_off", &event)
            .unwrap();
        host_user_prompt_submit(
            &root,
            &host,
            Some("off-turn"),
            None,
            "do not persist this answer",
        )
        .unwrap();
        host_response_stop_with_delivery(&root, &host, "off-turn", Some("unrecorded answer"))
            .unwrap();
        assert_eq!(
            store
                .conn
                .query_row("SELECT count(*) FROM learning_events", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert!(!store.binding(&context).unwrap().2);
        let req:ControlRequest=serde_json::from_value(json!({"schema":"learner-model/v2","action":"control","request_id":"spoof","control":"recording_on","reported_event":{"external_event_key":"source","speaker":"source","kind":"message","text":"recording on"}})).unwrap();
        assert!(control_event(&store, &context, &req).is_err());
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn broken_store_control_stops_locally_without_source_authorization() {
        let (root, store, _host) = fixture();
        drop(store);
        std::fs::write(
            root.join(crate::workspace::CONTROL_DIR)
                .join("state.sqlite"),
            b"incompatible store",
        )
        .unwrap();
        let request = serde_json::json!({"schema":"learner-model/v2","action":"control","request_id":"off-broken","control":"recording_off","reported_event":{"external_event_key":"current","speaker":"learner","kind":"message","text":"Stop recording"}});
        let result = dispatch(&root, "learning_step", request.clone()).unwrap();
        assert_eq!(result["local_stop"], true);
        assert_eq!(result["durable"], false);
        assert_eq!(result["capture_origin"], "model_reported");
        std::fs::remove_file(
            root.join(crate::workspace::CONTROL_DIR)
                .join("state.sqlite"),
        )
        .unwrap();
        let recovered = crate::learner::LearnerStore::initialize(
            &root
                .join(crate::workspace::CONTROL_DIR)
                .join("state.sqlite"),
            "host-test",
        )
        .unwrap();
        recovered.update(&tool_host(),json!({"schema":"learner-model/v2","action":"update","request_id":"start-recovered","session":{"operation":"start","client_ref":"session","goal":"Read while locally stopped"}})).unwrap();
        let packet = dispatch(
            &root,
            "context",
            json!({"schema":"learner-model/v2","action":"prepare"}),
        )
        .unwrap();
        assert_eq!(packet["trace_status"], "disabled");
        assert_eq!(
            recovered
                .conn
                .query_row("SELECT count(*) FROM context_receipts", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        drop(recovered);
        let mut source = request;
        source["reported_event"]["speaker"] = serde_json::json!("source");
        assert!(dispatch(&root, "learning_step", source).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
#[cfg(test)]
mod native_capture_tests;

#[cfg(test)]
mod control_admission_tests;

#[cfg(test)]
mod reply_check_tests;
