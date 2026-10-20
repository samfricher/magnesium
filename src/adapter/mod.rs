pub(crate) mod control_admission;
pub(crate) mod control_safety;
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
const CONTEXT_SHAPES: &str = r#"context takes either an instruction load {"action":"instructions","host":"claude"|"codex"|"opencode","stage":"orient_and_probe"|"plan_and_route"|"teach_and_practice"|"interpret_and_repair"|"review_and_transfer"|"consolidate_and_resume"|"papers"|"papers_loop"} with optional "operation":"state_and_evidence"|"source_use"|"tool_recipes", or a learner-state read {"schema":"learner-model/v2","action":"status"|"prepare"|"session_choices"|"pending_events",...} without host or stage"#;
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
    use serde_json::json;
    if operation == "learning_step"
        && serde_json::to_vec(&request)
            .map_err(|e| e.to_string())?
            .len()
            > 1024 * 1024
    {
        return Err("field_too_large: semantic request exceeds 1 MiB".into());
    }
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
    let (_, resolved) = match resolve_workspace(cwd) {
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
    let mut notice = hook_notice(&store, &host, None, NoticeHook::SessionStart)?;
    if host.host_kind == "claude"
        && !store
            .conn
            .query_row("SELECT EXISTS(SELECT 1 FROM learning_sessions)", [], |r| {
                r.get::<_, bool>(0)
            })
            .map_err(|e| e.to_string())?
    {
        notice.push_str(CLAUDE_NEW_LEARNER_GUIDANCE);
    }
    Ok(Some(notice))
}
/// The hook a notice answers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NoticeHook {
    SessionStart,
    UserPromptSubmit,
}
impl NoticeHook {
    /// The `additionalContextLimit` that the Claude Code plugin's hooks.json
    /// gives this hook.
    const fn limit(self) -> usize {
        match self {
            Self::SessionStart => 1024,
            Self::UserPromptSubmit => 512,
        }
    }
}
/// Clauses after a notice's state line, declared in the order they give way
/// when the notice is over its hook's limit. `Required` clauses never do.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Clause {
    /// SessionStart repeats it on resume and compaction, when it applies.
    ReloadAfterCompaction,
    TeachingReminder,
    ReplyFeedback,
    Required,
}
fn hook_notice(
    store: &crate::learner::LearnerStore,
    host: &crate::learner::HostContext,
    event: Option<&str>,
    hook: NoticeHook,
) -> Result<String, String> {
    let (_, active, recording) = store.binding(host).map_err(|e| e.to_string())?;
    let (generation,pending,unbound):(i64,i64,i64)=store.conn.query_row("SELECT memory_generation,(SELECT count(*) FROM learning_events WHERE host_kind=?1 AND host_thread_id=?2 AND processing='pending'),(SELECT count(*) FROM learning_events WHERE session_id IS NULL AND processing='pending') FROM workspace_meta",rusqlite::params![host.host_kind,host.host_thread_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
    let state = format!(
        "Magnesium learner-model/v2; generation {generation}; pending {pending}; unbound {unbound}; learning {active}; recording {recording}; current event {}. Read context.prepare on new conversation, compaction, changed goal or contradiction; learning_step revises memory without an assessment gate.",
        event.unwrap_or("unavailable (use labeled reported events)")
    );
    let claude = host.host_kind == "claude";
    let mut clauses: Vec<(Clause, Cow<'static, str>)> = Vec::new();
    // Plain next-step guidance for Claude Code, whose tutor can lose the
    // Magnesium skill's instructions to compaction and otherwise sees only
    // state fields. Other hosts keep their established notices.
    match (claude, active, recording) {
        (true, true, true) => {
            clauses.push((Clause::Required, CLAUDE_LESSON_GUIDANCE.into()));
            clauses.push((Clause::ReloadAfterCompaction, CLAUDE_RELOAD_GUIDANCE.into()));
        }
        (true, false, true) => clauses.push((Clause::Required, CLAUDE_START_GUIDANCE.into())),
        _ => {}
    }
    if unbound > 0 {
        clauses.push((
            Clause::Required,
            " Unbound pending work: context.pending_work with unbound_only:true.".into(),
        ));
    }
    if claude && active && hook == NoticeHook::UserPromptSubmit {
        clauses.push((Clause::TeachingReminder, reply_check::REMINDER.into()));
        if let Some(feedback) = event.and_then(|e| reply_check::feedback(&store.conn, host, e)) {
            clauses.push((Clause::ReplyFeedback, feedback.into()));
        }
    }
    Ok(fit(state, clauses, hook.limit()))
}
/// The state line, which carries the current event id, and the clauses in
/// order, after whole clauses give way in `Clause` order until the text fits.
fn fit(state: String, mut clauses: Vec<(Clause, Cow<'static, str>)>, limit: usize) -> String {
    while state.len() + clauses.iter().map(|(_, text)| text.len()).sum::<usize>() > limit {
        let Some(index) = (0..clauses.len())
            .filter(|&index| clauses[index].0 != Clause::Required)
            .min_by_key(|&index| clauses[index].0)
        else {
            break;
        };
        clauses.remove(index);
    }
    clauses
        .iter()
        .fold(state, |notice, (_, text)| notice + text.as_ref())
}
const CLAUDE_LESSON_GUIDANCE: &str =
    " A lesson is active: interpret this learner turn with learning_step.";
const CLAUDE_RELOAD_GUIDANCE: &str =
    " After a new conversation or compaction, load the learn skill and its instructions first.";
const CLAUDE_START_GUIDANCE: &str = " Magnesium is set up in this folder: when the learner asks to learn, be taught, practise, be quizzed or review, load the learn skill and start or resume a session.";
/// Only the Claude Code plugin ships `/magnesium:help`, so other hosts never get this line.
const CLAUDE_NEW_LEARNER_GUIDANCE: &str = " No learning session is recorded in this study folder yet, so the learner is probably new to Magnesium: when it fits, tell them \"New here? Type /magnesium:help.\"";
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
                return Ok(Some(notice));
            }
            Err(_) => {
                return Ok(Some(format!(
                    "{} Native ticket unavailable; use labeled model-reported fallback.",
                    hook_notice(&store, &context, None, NoticeHook::UserPromptSubmit)?
                )));
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
    let context = host_context(host);
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
    fn fixture() -> (
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
    fn claude_lesson() -> (
        std::path::PathBuf,
        crate::learner::LearnerStore,
        HostIdentity,
    ) {
        let (root, store, _) = fixture();
        let host = HostIdentity::claude("teaching-reminder").unwrap();
        store.update(&host_context(&host),json!({"schema":"learner-model/v2","action":"update","request_id":"start","session":{"operation":"start","client_ref":"s","goal":"Quadratic coefficients"}})).unwrap();
        (root, store, host)
    }
    fn claude_prompt(root: &Path, host: &HostIdentity, turn: &str) -> String {
        host_user_prompt_submit(root, host, Some(turn), None, "Is b another unknown?")
            .unwrap()
            .unwrap()
    }
    fn claude_stop(
        root: &Path,
        host: &HostIdentity,
        turn: &str,
        reply: &str,
    ) -> Result<(), String> {
        host_response_stop_with_delivery(
            root,
            host,
            &host.stored_turn_id(turn).unwrap(),
            Some(reply),
        )
    }
    #[test]
    fn claude_prompt_notice_reports_only_the_reply_to_the_previous_prompt() {
        let (root, store, host) = claude_lesson();
        let first = claude_prompt(&root, &host, "one");
        assert!(first.contains(reply_check::REMINDER), "{first}");
        assert!(first.contains(CLAUDE_RELOAD_GUIDANCE), "{first}");
        assert!(!first.contains("Previous reply"), "{first}");

        claude_stop(&root, &host, "one", &"word ".repeat(850)).unwrap();
        assert!(
            claude_stop(&root, &host, "one", "Short now. What is b?").is_err(),
            "a repeated Stop with new text keeps the first reply"
        );
        let second = claude_prompt(&root, &host, "two");
        assert!(
            second.contains(" Previous reply: 850 words, no task."),
            "{second}"
        );
        assert!(second.contains("current event e-"), "{second}");
        assert!(second.contains(CLAUDE_LESSON_GUIDANCE), "{second}");
        assert!(second.len() <= 512, "{} bytes: {second}", second.len());
        assert_eq!(
            claude_prompt(&root, &host, "two"),
            second,
            "a retried prompt hook"
        );

        let after_missed_stop = claude_prompt(&root, &host, "three");
        assert!(
            !after_missed_stop.contains("Previous reply"),
            "{after_missed_stop}"
        );
        claude_stop(
            &root,
            &host,
            "three",
            "b is fixed. Your turn: name the coefficient of x.",
        )
        .unwrap();
        let after_good_reply = claude_prompt(&root, &host, "four");
        assert!(
            !after_good_reply.contains("Previous reply"),
            "{after_good_reply}"
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn claude_prompt_notice_keeps_required_clauses_within_512_bytes_at_extreme_counts() {
        let (root, store, host) = claude_lesson();
        store
            .conn
            .execute("UPDATE workspace_meta SET memory_generation=?1", [i64::MAX])
            .unwrap();
        let earlier = host_context(&HostIdentity::claude("earlier-conversation").unwrap());
        let unbound = crate::learner::RawEvent {
            external_event_key: "unbound".into(),
            speaker: crate::learner::Speaker::Learner,
            kind: "message".into(),
            text: "An answer before any lesson".into(),
            metadata: None,
            occurred_at_ms: None,
        };
        store
            .capture(&earlier, &[unbound], "host_observed", true)
            .unwrap();
        claude_prompt(&root, &host, "one");
        claude_stop(&root, &host, "one", &"w ".repeat(123_456)).unwrap();
        let notice = claude_prompt(&root, &host, "two");
        assert!(notice.len() <= 512, "{} bytes: {notice}", notice.len());
        for required in [
            "current event e-",
            CLAUDE_LESSON_GUIDANCE,
            "context.pending_work with unbound_only:true",
            " Previous reply: 123456 words, no task.",
        ] {
            assert!(notice.contains(required), "{required}: {notice}");
        }
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn notice_clauses_give_way_in_order_and_required_ones_stay() {
        let clauses = vec![
            (Clause::Required, " lesson".into()),
            (Clause::ReloadAfterCompaction, " reload".into()),
            (Clause::TeachingReminder, " reminder".into()),
            (Clause::ReplyFeedback, " feedback".into()),
        ];
        let fit_to = |limit| fit("state".into(), clauses.clone(), limit);
        assert_eq!(fit_to(512), "state lesson reload reminder feedback");
        assert_eq!(fit_to(35), "state lesson reminder feedback");
        assert_eq!(fit_to(27), "state lesson feedback");
        assert_eq!(fit_to(20), "state lesson");
        assert_eq!(fit_to(1), "state lesson");
    }
    #[test]
    fn notice_limits_match_the_claude_plugin_hooks() {
        let hooks: serde_json::Value =
            serde_json::from_str(include_str!("../../claude-plugin/hooks/hooks.json")).unwrap();
        for (event, hook) in [
            ("SessionStart", NoticeHook::SessionStart),
            ("UserPromptSubmit", NoticeHook::UserPromptSubmit),
        ] {
            assert_eq!(
                hooks["hooks"][event][0]["hooks"][0]["additionalContextLimit"],
                hook.limit(),
                "{event}"
            );
        }
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
