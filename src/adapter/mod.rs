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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NoticeHook<'a> {
    SessionStart,
    UserPromptSubmit {
        prompt: &'a str,
        turns: Option<&'a reply_check::PromptTurns>,
    },
}
impl NoticeHook<'_> {
    const fn additional_context_limit(self) -> usize {
        match self {
            Self::SessionStart => 1024,
            Self::UserPromptSubmit { .. } => 512,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Clause {
    Required,
    TeachingReminder,
    ReplyFeedback,
    AskWhyConfused,
}
const GIVE_WAY_ORDER: [Clause; 3] = [
    Clause::TeachingReminder,
    Clause::ReplyFeedback,
    Clause::AskWhyConfused,
];
fn hook_notice(
    store: &crate::learner::LearnerStore,
    host: &crate::learner::HostContext,
    event: Option<&str>,
    hook: NoticeHook<'_>,
) -> Result<String, String> {
    let (_, active, recording) = store.binding(host).map_err(|e| e.to_string())?;
    let (generation,pending,unbound):(i64,i64,i64)=store.conn.query_row("SELECT memory_generation,(SELECT count(*) FROM learning_events WHERE host_kind=?1 AND host_thread_id=?2 AND processing='pending'),(SELECT count(*) FROM learning_events WHERE session_id IS NULL AND processing='pending') FROM workspace_meta",rusqlite::params![host.host_kind,host.host_thread_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
    let state = format!(
        "Magnesium learner-model/v2; generation {generation}; pending {pending}; unbound {unbound}; learning {active}; recording {recording}; current event {}. Read context.prepare on new conversation, compaction, changed goal or contradiction; learning_step revises memory without an assessment gate.",
        event.unwrap_or("unavailable (use labeled reported events)")
    );
    let claude = host.host_kind == "claude";
    let mut clauses: Vec<(Clause, Cow<'static, str>)> = Vec::new();
    match (claude, active, recording) {
        (true, true, true) => {
            clauses.push((Clause::Required, CLAUDE_LESSON_GUIDANCE.into()));
            if hook == NoticeHook::SessionStart {
                clauses.push((Clause::Required, CLAUDE_RELOAD_GUIDANCE.into()));
            }
        }
        (true, false, true) => clauses.push((Clause::Required, CLAUDE_START_GUIDANCE.into())),
        _ => {}
    }
    if unbound > 0 {
        let recovery = if claude {
            " Unbound work: context.pending_work unbound_only:true."
        } else {
            " Unbound pending work: context.pending_work with unbound_only:true."
        };
        clauses.push((Clause::Required, recovery.into()));
    }
    if let (true, true, NoticeHook::UserPromptSubmit { prompt, turns }) = (claude, active, hook) {
        clauses.push((Clause::TeachingReminder, reply_check::reminder().into()));
        if let Some(feedback) = turns.and_then(|turns| {
            reply_check::feedback(&store.conn, host, turns)
                .ok()
                .flatten()
        }) {
            clauses.push((Clause::ReplyFeedback, feedback.into()));
        }
        if reply_check::wants_the_confusion_question(prompt) {
            clauses.push((Clause::AskWhyConfused, reply_check::ASK_WHY_CONFUSED.into()));
        }
    }
    Ok(fit(state, clauses, hook.additional_context_limit()))
}
fn fit(state: String, mut clauses: Vec<(Clause, Cow<'static, str>)>, limit: usize) -> String {
    for droppable in GIVE_WAY_ORDER {
        if state.len() + clauses.iter().map(|(_, text)| text.len()).sum::<usize>() <= limit {
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
    let claude = context.host_kind == "claude";
    if safety.blocked.is_some() {
        let _ = native_capture::invalidate(&workspace, &context);
        if claude {
            reply_check::forget_prompts(&workspace, &context);
        }
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
    let turns = match (claude, recording) {
        (true, true) => reply_check::remember_prompt(&workspace, &context, &key),
        (true, false) => {
            reply_check::forget_prompts(&workspace, &context);
            None
        }
        (false, _) => None,
    };
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
                if claude {
                    notice.push_str(CLAUDE_START_GUIDANCE);
                }
                return Ok(Some(notice));
            }
            Err(_) => {
                return Ok(Some(format!(
                    "{} Native ticket unavailable; use labeled model-reported fallback.",
                    hook_notice(
                        &store,
                        &context,
                        None,
                        NoticeHook::UserPromptSubmit {
                            prompt: text,
                            turns: None,
                        },
                    )?
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
        NoticeHook::UserPromptSubmit {
            prompt: text,
            turns: turns.as_ref(),
        },
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
    fn claude_feedback_says_no_task_after_an_offer_a_check_in_or_a_remark() {
        let (root, store, host) = claude_lesson();
        claude_prompt(&root, &host, "0");
        for (turn, reply) in [
            "b scales x, so it stays fixed. Would you like me to explain it differently?",
            "b scales x, so it stays fixed. Shall I write the next step?",
            "b scales x, so it stays fixed. Should I solve it?",
            "Spot on! b is the coefficient of x.",
            "Sort of… b is fixed, but x varies.",
        ]
        .into_iter()
        .enumerate()
        {
            claude_stop(&root, &host, &turn.to_string(), reply).unwrap();
            let next = claude_prompt(&root, &host, &(turn + 1).to_string());
            assert!(
                next.contains(" Previous reply: no task."),
                "{reply}: {next}"
            );
        }
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn claude_prompt_notice_reports_only_the_reply_to_the_previous_prompt() {
        let (root, store, host) = claude_lesson();
        let first = claude_prompt(&root, &host, "one");
        assert!(first.contains(&reply_check::reminder()), "{first}");
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
    fn claude_feedback_never_judges_a_reply_across_an_unrecorded_turn() {
        let (root, store, host) = claude_lesson();
        let context = host_context(&host);
        let control = |request: &str, action: &str| {
            let event = crate::learner::RawEvent {
                external_event_key: request.into(),
                speaker: crate::learner::Speaker::Learner,
                kind: "message".into(),
                text: action.into(),
                metadata: Some(crate::learner::EventMetadata {
                    origin: Some("model_reported".into()),
                    ..Default::default()
                }),
                occurred_at_ms: None,
            };
            store.control(&context, request, action, &event).unwrap();
        };
        claude_prompt(&root, &host, "one");
        control("off", "recording_off");
        claude_stop(&root, &host, "one", &"unrecorded ".repeat(300)).unwrap();
        claude_prompt(&root, &host, "two");
        control("on", "recording_on");
        claude_stop(&root, &host, "two", &"recorded ".repeat(300)).unwrap();
        let after_gap = claude_prompt(&root, &host, "three");
        assert!(!after_gap.contains("Previous reply"), "{after_gap}");
        claude_stop(&root, &host, "three", &"recorded ".repeat(300)).unwrap();
        let resumed = claude_prompt(&root, &host, "four");
        assert!(
            resumed.contains(" Previous reply: 300 words, no task."),
            "{resumed}"
        );

        claude_stop(&root, &host, "four", &"recorded ".repeat(280)).unwrap();
        control("off-between-turns", "recording_off");
        claude_prompt(&root, &host, "five");
        claude_stop(&root, &host, "five", "Sure. What is b?").unwrap();
        control("on-between-turns", "recording_on");
        let after_toggles = claude_prompt(&root, &host, "six");
        assert!(!after_toggles.contains("Previous reply"), "{after_toggles}");
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    fn prompt_turns_file(root: &Path, host: &HostIdentity) -> std::path::PathBuf {
        host::conversation_file(
            &Workspace::discover(root).unwrap().unwrap(),
            "cache/prompt-turns",
            &host.stored_session_id(),
            "turns",
        )
        .unwrap()
    }
    fn long_reply() -> String {
        "This is the idea and it is what you need. ".repeat(30)
    }
    #[test]
    fn claude_feedback_never_judges_an_older_reply_from_a_stale_turns_file() {
        let (root, store, host) = claude_lesson();
        claude_prompt(&root, &host, "one");
        claude_stop(&root, &host, "one", &long_reply()).unwrap();
        let file = prompt_turns_file(&root, &host);
        let after_one = std::fs::read(&file).unwrap();
        claude_prompt(&root, &host, "two");
        std::fs::write(&file, after_one).unwrap();
        claude_stop(&root, &host, "two", "Good. Now solve 2x = 6.").unwrap();
        let after_stale = claude_prompt(&root, &host, "three");
        assert!(!after_stale.contains("Previous reply"), "{after_stale}");
        claude_stop(&root, &host, "three", &long_reply()).unwrap();
        let healed = claude_prompt(&root, &host, "four");
        assert!(
            healed.contains(" Previous reply: 300 words, no task."),
            "{healed}"
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn claude_feedback_never_judges_an_older_reply_when_the_turns_file_is_immutable() {
        let (root, store, host) = claude_lesson();
        claude_prompt(&root, &host, "one");
        claude_stop(&root, &host, "one", &long_reply()).unwrap();
        let file = prompt_turns_file(&root, &host);
        let flags = |flag: &str| {
            assert!(
                std::process::Command::new("chflags")
                    .arg(flag)
                    .arg(&file)
                    .status()
                    .unwrap()
                    .success()
            )
        };
        flags("uchg");
        let unwritable = claude_prompt(&root, &host, "two");
        claude_stop(&root, &host, "two", "Good. Now solve 2x = 6.").unwrap();
        let after = claude_prompt(&root, &host, "three");
        flags("nouchg");
        assert!(!unwritable.contains("Previous reply"), "{unwritable}");
        assert!(!after.contains("Previous reply"), "{after}");
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn claude_feedback_never_judges_an_older_reply_when_the_turns_file_cannot_be_written() {
        let (root, store, host) = claude_lesson();
        claude_prompt(&root, &host, "one");
        claude_stop(&root, &host, "one", &long_reply()).unwrap();
        let file = prompt_turns_file(&root, &host);
        std::fs::remove_file(&file).unwrap();
        std::fs::create_dir(&file).unwrap();
        let unwritable = claude_prompt(&root, &host, "two");
        assert!(file.is_dir(), "the blocked path stays a directory");
        assert!(!unwritable.contains("Previous reply"), "{unwritable}");
        claude_stop(&root, &host, "two", "Good. Now solve 2x = 6.").unwrap();
        let after = claude_prompt(&root, &host, "three");
        assert!(!after.contains("Previous reply"), "{after}");
        std::fs::remove_dir(&file).unwrap();
        claude_prompt(&root, &host, "four");
        claude_stop(&root, &host, "four", &long_reply()).unwrap();
        let healed = claude_prompt(&root, &host, "five");
        assert!(
            healed.contains(" Previous reply: 300 words, no task."),
            "{healed}"
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn claude_feedback_skips_a_turn_after_a_prompt_hook_that_exited_early() {
        let (root, store, host) = claude_lesson();
        claude_prompt(&root, &host, "one");
        claude_stop(&root, &host, "one", &long_reply()).unwrap();
        claude_stop(&root, &host, "two", "Good. Now solve 2x = 6.").unwrap();
        let after = claude_prompt(&root, &host, "three");
        assert!(!after.contains("Previous reply"), "{after}");
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn claude_turns_file_heals_when_malformed_or_its_folder_is_deleted() {
        let (root, store, host) = claude_lesson();
        claude_prompt(&root, &host, "one");
        let file = prompt_turns_file(&root, &host);
        std::fs::write(&file, "a\nb\nc").unwrap();
        claude_stop(&root, &host, "one", &long_reply()).unwrap();
        assert!(!claude_prompt(&root, &host, "two").contains("Previous reply"));
        claude_stop(&root, &host, "two", &long_reply()).unwrap();
        let healed = claude_prompt(&root, &host, "three");
        assert!(
            healed.contains(" Previous reply: 300 words, no task."),
            "{healed}"
        );
        std::fs::remove_dir_all(file.parent().unwrap()).unwrap();
        claude_stop(&root, &host, "three", &long_reply()).unwrap();
        assert!(!claude_prompt(&root, &host, "four").contains("Previous reply"));
        claude_stop(&root, &host, "four", &long_reply()).unwrap();
        let regrown = claude_prompt(&root, &host, "five");
        assert!(
            regrown.contains(" Previous reply: 300 words, no task."),
            "{regrown}"
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn claude_latch_block_deletes_the_turns_file() {
        let (root, store, host) = claude_lesson();
        claude_prompt(&root, &host, "one");
        let file = prompt_turns_file(&root, &host);
        assert!(file.exists());
        let context = host_context(&host);
        use sha2::Digest as _;
        let key: String = sha2::Sha256::digest(
            serde_json::to_vec(&[&context.host_kind, &context.host_thread_id]).unwrap(),
        )
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
        let deny = root
            .join(crate::workspace::CONTROL_DIR)
            .join("control-safety")
            .join(format!("{key}.deny"));
        std::fs::create_dir_all(deny.parent().unwrap()).unwrap();
        crate::workspace::write_private_atomic(&deny, &[1, 1]).unwrap();
        let blocked = claude_prompt(&root, &host, "two");
        assert!(blocked.contains("shared control safety latch"), "{blocked}");
        assert!(!file.exists());
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn claude_conversations_keep_their_own_feedback() {
        let (root, store, first) = claude_lesson();
        let second = HostIdentity::claude("teaching-reminder-second").unwrap();
        store.update(&host_context(&second),json!({"schema":"learner-model/v2","action":"update","request_id":"start-second","session":{"operation":"start","client_ref":"s2","goal":"Quadratic roots"}})).unwrap();
        claude_prompt(&root, &first, "a-1");
        claude_prompt(&root, &second, "b-1");
        claude_stop(&root, &first, "a-1", &long_reply()).unwrap();
        claude_stop(&root, &second, "b-1", "Good. Now solve 2x = 6.").unwrap();
        let first_note = claude_prompt(&root, &first, "a-2");
        let second_note = claude_prompt(&root, &second, "b-2");
        assert!(
            first_note.contains(" Previous reply: 300 words, no task."),
            "{first_note}"
        );
        assert!(!second_note.contains("Previous reply"), "{second_note}");
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn claude_reminder_fits_with_unbound_work_pending() {
        let (root, store, host) = claude_lesson();
        store
            .conn
            .execute("UPDATE workspace_meta SET memory_generation=?1", [9999])
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
        let notice = claude_prompt(&root, &host, "one");
        assert!(notice.contains("unbound 1"), "{notice}");
        assert!(
            notice.contains("context.pending_work unbound_only:true"),
            "{notice}"
        );
        assert!(notice.contains(&reply_check::reminder()), "{notice}");
        assert!(notice.len() <= 512, "{} bytes: {notice}", notice.len());
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn claude_confused_prompt_gets_the_confusion_clause_within_512_bytes() {
        let (root, store, host) = claude_lesson();
        let calm = claude_prompt(&root, &host, "one");
        assert!(!calm.contains(reply_check::ASK_WHY_CONFUSED), "{calm}");
        claude_stop(&root, &host, "one", &"word ".repeat(850)).unwrap();
        let confused = host_user_prompt_submit(
            &root,
            &host,
            Some("two"),
            None,
            "I'm confused, why is b fixed?",
        )
        .unwrap()
        .unwrap();
        for kept in [
            reply_check::ASK_WHY_CONFUSED,
            " Previous reply: 850 words, no task.",
            CLAUDE_LESSON_GUIDANCE,
            "current event e-",
        ] {
            assert!(confused.contains(kept), "{kept}: {confused}");
        }
        assert!(
            confused.len() <= 512,
            "{} bytes: {confused}",
            confused.len()
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
            "context.pending_work unbound_only:true",
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
            (Clause::TeachingReminder, " reminder".into()),
            (Clause::ReplyFeedback, " feedback".into()),
            (Clause::AskWhyConfused, " confused".into()),
        ];
        let fit_to = |limit| fit("state".into(), clauses.clone(), limit);
        assert_eq!(fit_to(512), "state lesson reminder feedback confused");
        assert_eq!(fit_to(35), "state lesson feedback confused");
        assert_eq!(fit_to(25), "state lesson confused");
        assert_eq!(fit_to(15), "state lesson");
        assert_eq!(fit_to(1), "state lesson");
    }
    #[test]
    fn notice_limits_match_the_claude_plugin_hooks() {
        let hooks: serde_json::Value =
            serde_json::from_str(include_str!("../../claude-plugin/hooks/hooks.json")).unwrap();
        for (event, hook) in [
            ("SessionStart", NoticeHook::SessionStart),
            (
                "UserPromptSubmit",
                NoticeHook::UserPromptSubmit {
                    prompt: "",
                    turns: None,
                },
            ),
        ] {
            assert_eq!(
                hooks["hooks"][event][0]["hooks"][0]["additionalContextLimit"],
                hook.additional_context_limit(),
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
