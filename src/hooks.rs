use std::io::{self, Read};
use std::path::PathBuf;
use std::process::ExitCode;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::adapter::{self, HostIdentity, HostKind, InvocationIdentity};
use crate::diagnostics::{HookFailure, record_hook_error};
use crate::workspace::Workspace;

const MAX_HOOK_INPUT_BYTES: u64 = 4 * 1024 * 1024;

/// Hook events the plugin no longer registers. A plugin copy from before
/// their removal, or a Claude Code session started before the program was
/// replaced, still runs them. Exit code 2 from a PreCompact hook blocks
/// compaction, so these names must keep parsing and must succeed: they exit
/// 0 at once, with no output, without reading their input or recording
/// anything.
pub(crate) const RETIRED_EVENTS: [&str; 2] = ["pre-compact", "post-compact"];

#[derive(Clone, Copy, Debug)]
enum HookEvent {
    SessionStart,
    UserPromptSubmit,
    Stop,
    SessionEnd,
    /// After Magnesium's `reply` tool, in a session the study window starts.
    PostToolUse,
}

impl HookEvent {
    fn from_command(value: &str) -> Option<Self> {
        match value {
            "session-start" => Some(Self::SessionStart),
            "user-prompt-submit" => Some(Self::UserPromptSubmit),
            "stop" => Some(Self::Stop),
            "session-end" => Some(Self::SessionEnd),
            "post-tool-use" => Some(Self::PostToolUse),
            _ => None,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::SessionStart => "SessionStart",
            Self::UserPromptSubmit => "UserPromptSubmit",
            Self::Stop => "Stop",
            Self::SessionEnd => "SessionEnd",
            Self::PostToolUse => "PostToolUse",
        }
    }
}

#[derive(Debug, Deserialize)]
struct HookInput {
    session_id: String,
    transcript_path: Option<PathBuf>,
    cwd: PathBuf,
    hook_event_name: String,
    model: Option<String>,
    permission_mode: Option<String>,
    /// Claude Code's per-prompt UUID, shared by every hook in that turn, so
    /// the prompt and Stop hooks agree on the turn. A prompt without one gets
    /// a native-local ordinal instead.
    prompt_id: Option<String>,
    prompt: Option<String>,
    source: Option<String>,
    reason: Option<String>,
    stop_hook_active: Option<bool>,
    last_assistant_message: Option<String>,
    tool_name: Option<String>,
    tool_input: Option<Value>,
}

/// The host comes from the installed `--host-kind` hook flag, because Claude
/// Code's official event JSON carries no kind field. A missing or unknown
/// kind is unavailable capture, never a silent reinterpretation as Claude
/// Code. The caller degrades fail-open so the host stays unblocked.
fn host_kind(flag: Option<&str>) -> Result<HostKind, String> {
    let flag = flag.ok_or("unavailable_capture: the hook command names no host kind")?;
    HostKind::parse(flag).map_err(|detail| format!("unavailable_capture: {detail}"))
}

pub(crate) fn run(
    command_event: &str,
    plugin_version: Option<String>,
    protocol_version: Option<String>,
    host_kind_flag: Option<String>,
) -> ExitCode {
    if RETIRED_EVENTS.contains(&command_event) {
        return ExitCode::SUCCESS;
    }
    let event = HookEvent::from_command(command_event).expect("clap validates hook events");
    let current_workspace = std::env::current_dir()
        .ok()
        .and_then(|cwd| Workspace::discover(&cwd).ok().flatten());

    if adapter::validate(&InvocationIdentity {
        plugin_version,
        protocol_version,
    })
    .is_err()
    {
        record_if_active(
            current_workspace.as_ref(),
            event,
            HookFailure::AdapterCompatibility,
        );
        return ExitCode::SUCCESS;
    }

    let input = match read_input() {
        Ok(input) => input,
        Err(_) => {
            record_if_active(
                current_workspace.as_ref(),
                event,
                HookFailure::InputReadDecode,
            );
            return ExitCode::SUCCESS;
        }
    };
    let kind = match host_kind(host_kind_flag.as_deref()) {
        Ok(kind) => kind,
        Err(_) => {
            record_if_active(
                current_workspace.as_ref(),
                event,
                HookFailure::InputValidation,
            );
            return ExitCode::SUCCESS;
        }
    };
    let workspace = match Workspace::discover(&input.cwd) {
        Ok(workspace) => workspace,
        Err(_) => {
            record_if_active(
                current_workspace.as_ref(),
                event,
                HookFailure::WorkspaceDiscovery,
            );
            return ExitCode::SUCCESS;
        }
    };

    if validate_input(event, &input).is_err() {
        record_if_active(workspace.as_ref(), event, HookFailure::InputValidation);
        return ExitCode::SUCCESS;
    }

    let output = match &workspace {
        Some(workspace) => match handle(event, &input, kind, workspace) {
            Ok(output) => output,
            Err(_) => {
                record_hook_error(workspace, event.name(), HookFailure::HookProcessing);
                return ExitCode::SUCCESS;
            }
        },
        None => None,
    };

    if let Some(output) = output
        && serde_json::to_writer(io::stdout().lock(), &output).is_err()
    {
        record_if_active(workspace.as_ref(), event, HookFailure::OutputSerialization);
    }
    ExitCode::SUCCESS
}

fn read_input() -> Result<HookInput, String> {
    read_bounded_input(io::stdin())
}

fn read_bounded_input(input: impl Read) -> Result<HookInput, String> {
    let mut bytes = Vec::new();
    input
        .take(MAX_HOOK_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("could not read hook input: {error}"))?;
    if bytes.len() as u64 > MAX_HOOK_INPUT_BYTES {
        return Err(format!(
            "hook input exceeds the {MAX_HOOK_INPUT_BYTES}-byte limit"
        ));
    }
    serde_json::from_slice(&bytes).map_err(|error| format!("invalid hook JSON: {error}"))
}

fn validate_input(event: HookEvent, input: &HookInput) -> Result<(), String> {
    if input.hook_event_name != event.name() {
        return Err("hook_event_name does not match the invoked handler".to_owned());
    }
    validate_identifier("session_id", &input.session_id)?;
    // A turn-scoped hook needs the native turn ID. Without one, only a
    // prompt is accepted, and `handle` then allocates a native-local ordinal
    // (see `validate_turn_for`).

    // Never scrape transcript files. Available finalized assistant text is
    // captured as observed delivery only while learning recording is active.
    // Provider model/permission metadata remains unpersisted.
    let _unpersisted_documented_fields =
        (&input.transcript_path, &input.model, &input.permission_mode);

    match event {
        HookEvent::SessionStart => validate_choice(
            "source",
            input.source.as_deref(),
            &["startup", "resume", "clear", "compact"],
        ),
        HookEvent::UserPromptSubmit => {
            validate_turn_for(input, true)?;
            input
                .prompt
                .as_ref()
                .ok_or_else(|| "UserPromptSubmit is missing prompt".to_owned())?;
            Ok(())
        }
        HookEvent::Stop => {
            validate_turn_for(input, false)?;
            input
                .stop_hook_active
                .ok_or_else(|| "Stop is missing stop_hook_active".to_owned())?;
            Ok(())
        }
        // "clear" and "resume" are the reasons after which Claude Code's
        // process continues as a different conversation.
        HookEvent::SessionEnd => validate_choice(
            "reason",
            input.reason.as_deref(),
            &["other", "clear", "resume"],
        ),
        HookEvent::PostToolUse => input
            .tool_name
            .as_deref()
            .map(|_| ())
            .ok_or_else(|| "PostToolUse is missing tool_name".to_owned()),
    }
}

fn validate_turn_for(input: &HookInput, allocatable: bool) -> Result<(), String> {
    match input.prompt_id.as_deref() {
        Some(prompt_id) => validate_turn_identifier(prompt_id),
        None if allocatable => Ok(()),
        None => Err("turn-scoped hook is missing prompt_id".to_owned()),
    }
}

fn validate_turn_identifier(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > crate::adapter::MAX_TURN_ID_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(format!(
            "prompt_id must contain 1-{} bytes without control characters",
            crate::adapter::MAX_TURN_ID_BYTES
        ));
    }
    Ok(())
}

fn validate_identifier(field: &str, value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
        return Err(format!(
            "{field} must contain 1-512 characters without control characters"
        ));
    }
    Ok(())
}

fn validate_choice(field: &str, value: Option<&str>, choices: &[&str]) -> Result<(), String> {
    let value = value.ok_or_else(|| format!("hook input is missing {field}"))?;
    if choices.contains(&value) {
        Ok(())
    } else {
        Err(format!("hook input contains an unsupported {field}"))
    }
}

fn handle(
    event: HookEvent,
    input: &HookInput,
    kind: HostKind,
    workspace: &Workspace,
) -> Result<Option<Value>, String> {
    // Hook cwd must resolve to the same nearest boundary as the workspace
    // under operation. A mismatch (nested/moved workspace, changed route,
    // simultaneous windows, or a bound-root violation) rejects learning
    // writes here; ordinary host work continues because hooks always exit
    // successfully with degraded status instead of blocking.
    adapter::check_workspace_binding(workspace)?;
    let host = &HostIdentity::new(kind, &input.session_id)
        .map_err(|detail| format!("unavailable_capture: {detail}"))?;
    if let HookEvent::PostToolUse = event {
        let tool = input.tool_name.as_deref();
        let ends = match tool_kind(tool) {
            Some(MagnesiumTool::Reply) => !save_after(input.tool_input.as_ref()),
            // The notes the tutor sends after its reply are the turn's last
            // write. In a terminal no reply tool exists, so this never fires.
            Some(MagnesiumTool::LearningStep) => adapter::replied_this_turn(&input.cwd, host)?,
            None => false,
        };
        return Ok(ends.then(end_turn));
    }
    match event {
        HookEvent::SessionStart => {
            adapter::host::clear_session_ended(workspace, host)?;
            let context = adapter::host_session_start(&input.cwd, host)?;
            Ok(context.map(|c| additional_context(event, c)))
        }
        HookEvent::UserPromptSubmit => {
            let prompt = input.prompt.as_deref().expect("validated prompt");
            let context = if input.prompt_id.is_none() {
                // A prompt without a stable turn ID: allocate a native-local
                // ordinal at this trusted prompt hook. The session/order
                // mapping is unambiguous (per-session monotonic counter);
                // retries reuse the MCP idempotency key.
                let ordinal =
                    adapter::host::allocate_native_ordinal(workspace, &host.stored_session_id())?;
                adapter::host_user_prompt_submit(&input.cwd, host, None, Some(ordinal), prompt)?
            } else {
                adapter::host_user_prompt_submit(
                    &input.cwd,
                    host,
                    input.prompt_id.as_deref(),
                    None,
                    prompt,
                )?
            };
            Ok(context.map(|c| additional_context(event, c)))
        }
        HookEvent::Stop => {
            let native = input
                .prompt_id
                .as_deref()
                .ok_or("turn-scoped hook is missing prompt_id")?;
            let stored_turn = host
                .stored_turn_id(native)
                .map_err(|detail| format!("unavailable_capture: {detail}"))?;
            adapter::host_response_stop_with_delivery(
                &input.cwd,
                host,
                &stored_turn,
                input.last_assistant_message.as_deref(),
            )?;
            Ok(Some(json!({})))
        }
        HookEvent::SessionEnd => {
            if matches!(input.reason.as_deref(), Some("clear" | "resume")) {
                adapter::host::mark_session_ended(workspace, host)?;
            }
            Ok(None)
        }
        HookEvent::PostToolUse => Ok(None),
    }
}

/// The turn ends at its last write in the window: at the reply, or, when a
/// memory edit or checkpoint is due, at the one learning_step after it. A
/// tutor asked to write nothing more still wrote a closing line in most turns
/// of the live runs of 2026-11-11 ("Your question is above"), which the
/// window would show as a second message, and which cost seconds. Other
/// tools, and every terminal session, are left alone.
fn end_turn() -> Value {
    json!({"continue": false, "suppressOutput": true})
}

#[derive(Debug, PartialEq)]
enum MagnesiumTool {
    Reply,
    LearningStep,
}

fn tool_kind(tool: Option<&str>) -> Option<MagnesiumTool> {
    let tool = tool?;
    if !(tool.starts_with("mcp__") && tool.contains("magnesium")) {
        return None;
    }
    if tool.ends_with(&format!("__{}", crate::mcp::catalog::REPLY_TOOL)) {
        Some(MagnesiumTool::Reply)
    } else if tool.ends_with("__learning_step") {
        Some(MagnesiumTool::LearningStep)
    } else {
        None
    }
}

fn save_after(input: Option<&Value>) -> bool {
    input.and_then(|input| input["save_after"].as_bool()) == Some(true)
}

fn additional_context(event: HookEvent, context: String) -> Value {
    debug_assert!(matches!(
        event,
        HookEvent::SessionStart | HookEvent::UserPromptSubmit
    ));
    json!({
        "hookSpecificOutput": {
            "hookEventName": event.name(),
            "additionalContext": context,
        }
    })
}

fn record_if_active(workspace: Option<&Workspace>, event: HookEvent, failure: HookFailure) {
    if let Some(workspace) = workspace {
        record_hook_error(workspace, event.name(), failure);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{
        MAX_HOOK_INPUT_BYTES, MagnesiumTool, end_turn, read_bounded_input, save_after, tool_kind,
        validate_turn_identifier,
    };

    #[test]
    fn only_magnesiums_reply_and_learning_step_can_end_the_turn() {
        assert_eq!(end_turn()["continue"], false);
        assert_eq!(
            tool_kind(Some("mcp__plugin_magnesium_magnesium__reply")),
            Some(MagnesiumTool::Reply)
        );
        assert_eq!(
            tool_kind(Some("mcp__plugin_magnesium_magnesium__learning_step")),
            Some(MagnesiumTool::LearningStep)
        );
        for other in [
            "mcp__other__reply",
            "Write",
            "reply",
            "mcp__plugin_magnesium_magnesium__context",
        ] {
            assert_eq!(tool_kind(Some(other)), None, "{other}");
        }
        assert_eq!(tool_kind(None), None);
        assert!(save_after(Some(
            &serde_json::json!({"text": "x", "save_after": true})
        )));
        assert!(!save_after(Some(&serde_json::json!({"text": "x"}))));
        assert!(!save_after(None));
    }

    #[test]
    fn hook_input_accepts_exact_limit_and_rejects_one_byte_over() {
        let prefix =
            br#"{"session_id":"fixture","cwd":"/tmp","hook_event_name":"UserPromptSubmit"}"#;
        let mut exact = prefix.to_vec();
        exact.resize(MAX_HOOK_INPUT_BYTES as usize, b' ');
        assert!(read_bounded_input(Cursor::new(exact)).is_ok());

        let mut oversized = prefix.to_vec();
        oversized.resize(MAX_HOOK_INPUT_BYTES as usize + 1, b' ');
        let error = read_bounded_input(Cursor::new(oversized)).expect_err("oversized hook input");
        assert!(error.contains("exceeds the 4194304-byte limit"));
    }

    #[test]
    fn turn_identifier_bound_matches_context_assembly_limit() {
        assert!(validate_turn_identifier(&"t".repeat(256)).is_ok());
        assert!(validate_turn_identifier(&"t".repeat(257)).is_err());
    }
}
