use std::io::{self, Read};
use std::path::PathBuf;
use std::process::ExitCode;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::adapter::{self, HostIdentity, HostKind, InvocationIdentity};
use crate::diagnostics::{HookFailure, record_hook_error};
use crate::workspace::Workspace;

const MAX_HOOK_INPUT_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
enum HookEvent {
    SessionStart,
    UserPromptSubmit,
    Stop,
    PreCompact,
    PostCompact,
    SessionEnd,
}

impl HookEvent {
    fn from_command(value: &str) -> Option<Self> {
        match value {
            "session-start" => Some(Self::SessionStart),
            "user-prompt-submit" => Some(Self::UserPromptSubmit),
            "stop" => Some(Self::Stop),
            "pre-compact" => Some(Self::PreCompact),
            "post-compact" => Some(Self::PostCompact),
            "session-end" => Some(Self::SessionEnd),
            _ => None,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::SessionStart => "SessionStart",
            Self::UserPromptSubmit => "UserPromptSubmit",
            Self::Stop => "Stop",
            Self::PreCompact => "PreCompact",
            Self::PostCompact => "PostCompact",
            Self::SessionEnd => "SessionEnd",
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
    turn_id: Option<String>,
    /// Optional host kind for the common adapter boundary. Absent means
    /// Codex. Unknown kinds are rejected as unavailable capture;
    /// they never inherit Codex trust. Hosts whose native event JSON carries
    /// no kind field (Claude Code) receive it from the installed
    /// `--host-kind` hook flag instead; a flag that disagrees with a payload
    /// kind is ambiguous and means unavailable capture.
    host_kind: Option<String>,
    /// Claude Code's per-prompt UUID, shared by every hook in that turn.
    prompt_id: Option<String>,
    prompt: Option<String>,
    source: Option<String>,
    trigger: Option<String>,
    reason: Option<String>,
    stop_hook_active: Option<bool>,
    last_assistant_message: Option<String>,
}

fn host_identity(input: &HookInput) -> Result<HostIdentity, String> {
    let kind = match input.host_kind.as_deref() {
        None => HostKind::Codex,
        Some(value) => {
            HostKind::parse(value).map_err(|detail| format!("unavailable_capture: {detail}"))?
        }
    };
    match kind {
        HostKind::Codex => HostIdentity::codex(&input.session_id),
        HostKind::Claude => HostIdentity::claude(&input.session_id),
        HostKind::OpenCode => HostIdentity::opencode(&input.session_id),
    }
    .map_err(|detail| format!("unavailable_capture: {detail}"))
}

/// Merge the installed `--host-kind` hook flag into the parsed event. Claude
/// Code's official event JSON carries no kind field, so the installed Claude
/// command line supplies it; adapters whose payload already carries a kind
/// never use the flag. An unknown flag kind, or a flag that disagrees with
/// the payload kind, is ambiguous input: unavailable capture, never a silent
/// reinterpretation as another host. The caller degrades fail-open so the
/// host stays unblocked.
fn apply_host_kind_flag(input: &mut HookInput, flag: Option<&str>) -> Result<(), String> {
    let Some(flag) = flag else {
        return Ok(());
    };
    let kind = HostKind::parse(flag).map_err(|detail| format!("unavailable_capture: {detail}"))?;
    if let Some(payload) = input.host_kind.as_deref()
        && payload != kind.as_str()
    {
        return Err("turn-scoped hook is ambiguous".to_owned());
    }
    input.host_kind = Some(kind.as_str().to_owned());
    Ok(())
}

/// Claude Code has no `turn_id`, but its common `prompt_id` names the user
/// prompt a hook belongs to, so the prompt and Stop hooks agree on the turn.
/// Events without one keep the ordinal fallback at prompt time.
fn adopt_claude_prompt_id(input: &mut HookInput) {
    if input.host_kind.as_deref() == Some("claude") && input.turn_id.is_none() {
        input.turn_id = input.prompt_id.take();
    }
}

pub(crate) fn run(
    command_event: &str,
    plugin_version: Option<String>,
    protocol_version: Option<String>,
    host_kind_flag: Option<String>,
) -> ExitCode {
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

    let mut input = match read_input() {
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
    if apply_host_kind_flag(&mut input, host_kind_flag.as_deref()).is_err() {
        record_if_active(
            current_workspace.as_ref(),
            event,
            HookFailure::InputValidation,
        );
        return ExitCode::SUCCESS;
    }
    adopt_claude_prompt_id(&mut input);
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

    let output = match workspace {
        Some(workspace) => match handle(event, &input, &workspace) {
            Ok(output) => output,
            Err(_) => {
                record_hook_error(&workspace, event.name(), HookFailure::HookProcessing);
                return ExitCode::SUCCESS;
            }
        },
        None => inactive_output(event),
    };

    if let Some(output) = output
        && serde_json::to_writer(io::stdout().lock(), &output).is_err()
    {
        record_if_active(
            current_workspace.as_ref(),
            event,
            HookFailure::OutputSerialization,
        );
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
    if let Some(kind) = input.host_kind.as_deref()
        && HostKind::parse(kind).is_err()
    {
        return Err("hook input contains an unsupported host_kind".to_owned());
    }
    // Exactly one turn identity: a stable native turn ID or a native-local
    // ordinal. Both present is ambiguous and means unavailable capture.
    // Both missing is allocatable only for ordinal-capable hosts at prompt
    // time (see `validate_turn_for`); all other cases are unavailable.

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
        HookEvent::PreCompact => {
            validate_turn_for(input, false)?;
            validate_choice("trigger", input.trigger.as_deref(), &["manual", "auto"])
        }
        HookEvent::PostCompact => {
            // Post-compaction is session-scoped: the OpenCode adapter reports
            // it without a turn because no turn is in flight when the host
            // emits the event. The marker itself is keyed by host session, so
            // no turn identity is consumed downstream.
            validate_post_compact_turn(input)?;
            validate_choice("trigger", input.trigger.as_deref(), &["manual", "auto"])
        }
        // Codex registers only "other"; Claude also registers the reasons
        // after which its process continues as a different conversation.
        HookEvent::SessionEnd => validate_choice(
            "reason",
            input.reason.as_deref(),
            &["other", "clear", "resume"],
        ),
    }
}

fn validate_turn_for(input: &HookInput, allocatable: bool) -> Result<(), String> {
    match input.turn_id.as_deref() {
        Some(turn_id) => validate_turn_identifier(turn_id),
        None if allocatable && input.host_kind.as_deref() == Some("claude") => Ok(()),
        None => Err("turn-scoped hook is missing turn_id".to_owned()),
    }
}

fn validate_post_compact_turn(input: &HookInput) -> Result<(), String> {
    input
        .turn_id
        .as_deref()
        .map_or(Ok(()), validate_turn_identifier)
}

fn validate_turn_identifier(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > crate::adapter::MAX_TURN_ID_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(format!(
            "turn_id must contain 1-{} bytes without control characters",
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
    workspace: &Workspace,
) -> Result<Option<Value>, String> {
    // Hook cwd must resolve to the same nearest boundary as the workspace
    // under operation. A mismatch (nested/moved workspace, changed route,
    // simultaneous windows, or a bound-root violation) rejects learning
    // writes here; ordinary host work continues because hooks always exit
    // successfully with degraded status instead of blocking.
    adapter::check_workspace_binding(workspace)?;
    let host = host_identity(input)?;
    match event {
        HookEvent::SessionStart => {
            if host.kind() == HostKind::Claude {
                adapter::host::clear_session_ended(workspace, &host)?;
            }
            // All native hosts share the same workspace-local capture owner.
            let context = adapter::host_session_start(&input.cwd, &host)?;
            Ok(context.map(|c| additional_context(event, c)))
        }
        HookEvent::UserPromptSubmit => {
            let prompt = input.prompt.as_deref().expect("validated prompt");
            let context = if input.turn_id.is_none() {
                // Ordinal-capable host without a stable turn ID: allocate a
                // native-local ordinal at this trusted prompt hook. The
                // session/order mapping is unambiguous (per-session monotonic
                // counter); retries reuse the MCP idempotency key.
                let Some(workspace) =
                    Workspace::discover(&input.cwd).map_err(|error| error.to_string())?
                else {
                    return Ok(None);
                };
                let ordinal =
                    adapter::host::allocate_native_ordinal(&workspace, &host.stored_session_id())?;
                adapter::host_user_prompt_submit(&input.cwd, &host, None, Some(ordinal), prompt)?
            } else {
                adapter::host_user_prompt_submit(
                    &input.cwd,
                    &host,
                    input.turn_id.as_deref(),
                    None,
                    prompt,
                )?
            };
            Ok(context.map(|c| additional_context(event, c)))
        }
        HookEvent::Stop => {
            let native = input
                .turn_id
                .as_deref()
                .ok_or("turn-scoped hook is missing turn_id")?;
            let stored_turn = host
                .stored_turn_id(native)
                .map_err(|detail| format!("unavailable_capture: {detail}"))?;
            adapter::host_response_stop_with_delivery(
                &input.cwd,
                &host,
                &stored_turn,
                input.last_assistant_message.as_deref(),
            )?;
            Ok(Some(json!({})))
        }
        // Compaction needs no capture work: the prompt notice already tells
        // the tutor to reread context.prepare after a compaction.
        HookEvent::PreCompact | HookEvent::PostCompact => Ok(Some(json!({}))),
        HookEvent::SessionEnd => {
            if host.kind() == HostKind::Claude
                && matches!(input.reason.as_deref(), Some("clear" | "resume"))
            {
                adapter::host::mark_session_ended(workspace, &host)?;
            }
            Ok(None)
        }
    }
}

fn inactive_output(_event: HookEvent) -> Option<Value> {
    None
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

    use super::{MAX_HOOK_INPUT_BYTES, read_bounded_input, validate_turn_identifier};

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
