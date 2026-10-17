//! Small typed host-adapter boundary (LEARN-200).
//!
//!
//! Trust means supported local capture under the host's permission boundary,
//! not resistance to arbitrary malicious code running as the same OS user.
//! Capabilities come from the installed adapter plus negotiated host support,
//! never from a model argument. Tool connectivity alone never grants capture.

use std::path::{Path, PathBuf};

use crate::workspace::Workspace;

pub(crate) const EXPLICIT_ROOT_ENV: &str = "MAGNESIUM_WORKSPACE_ROOT";
pub(crate) const MAX_NATIVE_ID_BYTES: usize = 512;

/// Native process session identity exported by each host to hooks and stdio
/// MCP servers. Claude Code sends no conversation identity in MCP requests,
/// so `CLAUDE_CODE_SESSION_ID` is its only route. A Claude MCP server keeps
/// the ID it was spawned with, so it can lag the hooks after `/clear`.
pub(crate) const HOST_SESSION_ENV: [(HostKind, &str); 3] = [
    (HostKind::Codex, "CODEX_THREAD_ID"),
    (HostKind::Claude, "CLAUDE_CODE_SESSION_ID"),
    (HostKind::OpenCode, "OPENCODE_SESSION_ID"),
];

/// Stable native identity framing. Host kind is also stored separately in
/// the learner database; no identity is taken from a model tool argument.
pub(crate) const STORED_HOST_ID_PREFIX: &str = "magnesium-h1:";

/// Supported host kinds. Unknown kinds are rejected as unavailable capture;
/// they must not silently inherit another host's identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HostKind {
    Codex,
    Claude,
    OpenCode,
}

impl HostKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::OpenCode => "opencode",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "codex" => Ok(Self::Codex),
            "claude" => Ok(Self::Claude),
            "opencode" => Ok(Self::OpenCode),
            _ => Err("unsupported host kind".to_owned()),
        }
    }

    pub(crate) fn is_codex(self) -> bool {
        matches!(self, Self::Codex)
    }
}

/// Typed host identity supplied by an installed native adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HostIdentity {
    kind: HostKind,
    native_session_id: String,
}

impl HostIdentity {
    pub(crate) fn new(kind: HostKind, native_session_id: &str) -> Result<Self, String> {
        validate_native_id("native session ID", native_session_id)?;
        Ok(Self {
            kind,
            native_session_id: native_session_id.to_owned(),
        })
    }

    pub(crate) fn codex(native_session_id: &str) -> Result<Self, String> {
        Self::new(HostKind::Codex, native_session_id)
    }

    pub(crate) fn claude(native_session_id: &str) -> Result<Self, String> {
        Self::new(HostKind::Claude, native_session_id)
    }

    pub(crate) fn opencode(native_session_id: &str) -> Result<Self, String> {
        Self::new(HostKind::OpenCode, native_session_id)
    }

    pub(crate) fn kind(&self) -> HostKind {
        self.kind
    }

    /// Opaque host-thread key. Native identity does not restrict which
    /// workspace-wide memories may be read or revised.
    pub(crate) fn stored_session_id(&self) -> String {
        match self.kind {
            HostKind::Codex => self.native_session_id.clone(),
            _ => framed_host_id(self.kind, &self.native_session_id),
        }
    }

    /// Stable native event key used for capture deduplication.
    pub(crate) fn stored_turn_id(&self, native_turn_id: &str) -> Result<String, String> {
        validate_native_id("native turn ID", native_turn_id)?;
        Ok(match self.kind {
            HostKind::Codex => native_turn_id.to_owned(),
            _ => framed_host_id(self.kind, native_turn_id),
        })
    }

    /// Stored turn value for a native-local ordinal allocated at a trusted
    /// prompt hook for hosts without stable turn IDs. Newly minted
    /// ordinals are session-qualified (`ordinal-<session hash>-<20-digit
    /// ordinal>`) so two conversations of the same host cannot collide on
    /// their first ordinal within one store; historical unqualified
    /// `ordinal-…` values remain readable as opaque existing identities.
    pub(crate) fn stored_ordinal_turn_id(&self, ordinal: u64) -> String {
        let base = session_qualified_ordinal_turn_id(&self.stored_session_id(), ordinal);
        match self.kind {
            HostKind::Codex => base,
            _ => framed_host_id(self.kind, &base),
        }
    }
}

fn framed_host_id(kind: HostKind, native: &str) -> String {
    format!("{STORED_HOST_ID_PREFIX}{}:{native}", kind.as_str())
}

/// The host whose plugin launched this process, when its command line says so.
/// Only that host's session variable is trusted, so a Claude Code session
/// started from a Codex shell cannot borrow the inherited Codex thread.
static LAUNCH_HOST: std::sync::OnceLock<HostKind> = std::sync::OnceLock::new();

pub(crate) fn set_launch_host(kind: HostKind) {
    let _ = LAUNCH_HOST.set(kind);
}

pub(crate) fn launch_host() -> Option<HostKind> {
    LAUNCH_HOST.get().copied()
}

const ENDED_SESSIONS: &str = "cache/ended-sessions";

fn ended_marker(workspace: &Workspace, stored_session_id: &str) -> Result<PathBuf, String> {
    let directory = workspace
        .private_dir(Path::new(ENDED_SESSIONS))
        .map_err(|error| error.to_string())?;
    let path = directory.join(format!("{}.ended", stored_session_hash(stored_session_id)));
    crate::workspace::reject_symlink_components(&path, workspace.control())
        .map_err(|error| error.to_string())?;
    Ok(path)
}

/// Claude Code keeps a stdio MCP server, and its spawn-time session ID,
/// across /clear and in-process /resume. Its hooks mark the conversation they
/// leave in those cases, and unmark a conversation when it starts again.
pub(crate) fn mark_session_ended(workspace: &Workspace, host: &HostIdentity) -> Result<(), String> {
    crate::workspace::write_private_atomic(
        &ended_marker(workspace, &host.stored_session_id())?,
        b"",
    )
    .map_err(|error| error.to_string())
}

pub(crate) fn clear_session_ended(
    workspace: &Workspace,
    host: &HostIdentity,
) -> Result<(), String> {
    match std::fs::remove_file(ended_marker(workspace, &host.stored_session_id())?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

pub(crate) fn session_ended(
    workspace: &Workspace,
    stored_session_id: &str,
) -> Result<bool, String> {
    ended_marker(workspace, stored_session_id)?
        .try_exists()
        .map_err(|error| error.to_string())
}

/// Resolve a validated native process identity using the established host precedence.
/// Environment values are native IDs; framing happens only through HostIdentity.
/// No match grants neither a native identity nor permission to publish learner text.
pub(crate) fn process_host_identity() -> Option<HostIdentity> {
    let launched = launch_host();
    HOST_SESSION_ENV
        .into_iter()
        .filter(|(kind, _)| launched.is_none_or(|launched| launched == *kind))
        .find_map(|(kind, key)| {
            std::env::var(key)
                .ok()
                .and_then(|id| HostIdentity::new(kind, &id).ok())
        })
}

/// Native process workspace binding; tool arguments cannot change it.
pub(crate) fn explicit_root() -> Option<PathBuf> {
    std::env::var_os(EXPLICIT_ROOT_ENV)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

/// Verify that a discovered workspace matches the explicit binding.
/// Mismatch rejects learning writes; ordinary host work continues.
pub(crate) fn check_workspace_binding(workspace: &Workspace) -> Result<(), String> {
    let bound = explicit_root();
    check_workspace_binding_against(workspace, bound.as_deref())
}

pub(crate) fn check_workspace_binding_against(
    workspace: &Workspace,
    bound: Option<&Path>,
) -> Result<(), String> {
    let Some(bound) = bound else {
        return Ok(());
    };
    if !binding_matches(bound, workspace.root()) {
        return Err(
            "workspace binding mismatch: hook/MCP cwd resolved to a different workspace than the bound root"
                .to_owned(),
        );
    }
    Ok(())
}

/// Canonicalize-and-compare helper for tests and the MCP boundary.
pub(crate) fn binding_matches(bound_root: &Path, workspace_root: &Path) -> bool {
    let (Ok(bound), Ok(root)) = (
        std::fs::canonicalize(bound_root),
        std::fs::canonicalize(workspace_root),
    ) else {
        return false;
    };
    bound == root
}

/// Session-qualified ordinal fragment for newly minted ordinal turn IDs.
/// The SHA-256 of the exact stored host-session value namespaces the
/// counter so two conversations of one host allocate distinct ordinal
/// identities within one Magnesium store.
pub(crate) fn session_qualified_ordinal_turn_id(stored_session_id: &str, ordinal: u64) -> String {
    let hash = stored_session_hash(stored_session_id);
    format!("ordinal-{hash}-{ordinal:020}")
}

fn stored_session_hash(stored_session_id: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(stored_session_id.as_bytes()) {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// Allocate the next native-local ordinal for a host session. The counter
/// is scoped to the exact stored session value so two providers with
/// identical native strings allocate in independent sequences. The
/// counter is read and updated under a private OS lock; a missing file is
/// zero, while a corrupt or unreadable file is `unavailable_capture`
/// rather than a silent reset to zero. The update is checked and
/// persisted atomically; the lock is released on return. Order comes from
/// the monotonic ordinal; retries reuse the MCP idempotency key, not a
/// new ordinal. A raced duplicate ordinal fails safe at the existing
/// `UNIQUE(scope_id, turn_id)` write, never as a silent redirect.
pub(crate) fn allocate_native_ordinal(
    workspace: &Workspace,
    stored_session_id: &str,
) -> Result<u64, String> {
    if stored_session_id.is_empty() {
        return Err("unavailable_capture: session identity is missing".to_owned());
    }
    let hash = stored_session_hash(stored_session_id);
    let directory = workspace
        .private_dir(Path::new("cache/host-ordinals"))
        .map_err(|error| error.to_string())?;
    let counter_path = directory.join(format!("{hash}.counter"));
    let lock_path = directory.join(format!("{hash}.lock"));
    for path in [&counter_path, &lock_path] {
        crate::workspace::reject_symlink_components(path, workspace.control())
            .map_err(|error| error.to_string())?;
    }
    let lock =
        crate::workspace::open_private_lock(&lock_path).map_err(|error| error.to_string())?;
    acquire_native_ordinal_lock(&lock)?;
    let current = match std::fs::read_to_string(&counter_path) {
        Ok(text) => text
            .trim()
            .parse::<u64>()
            .map_err(|_| "unavailable_capture: host ordinal counter is corrupt".to_owned())?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => {
            return Err(format!(
                "unavailable_capture: host ordinal counter is unavailable: {error}"
            ));
        }
    };
    let next = current
        .checked_add(1)
        .ok_or_else(|| "unavailable_capture: host ordinal counter overflow".to_owned())?;
    crate::workspace::write_private_atomic(&counter_path, next.to_string().as_bytes())
        .map_err(|error| error.to_string())?;
    // Dropping `lock` releases the OS advisory lock.
    Ok(next)
}

/// Serialize ordinal access per session with a bounded private OS lock.
/// The lock has no TTL: process termination releases it.
fn acquire_native_ordinal_lock(lock: &std::fs::File) -> Result<(), String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match lock.try_lock() {
            Ok(()) => return Ok(()),
            Err(error) => {
                let source: std::io::Error = error.into();
                if source.kind() != std::io::ErrorKind::WouldBlock {
                    return Err(source.to_string());
                }
                if std::time::Instant::now() >= deadline {
                    return Err("unavailable_capture: host ordinal counter lock is busy".to_owned());
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    }
}

fn validate_native_id(field: &str, value: &str) -> Result<(), String> {
    if !is_valid_native_id(value) {
        return Err(format!(
            "{field} must contain 1-{MAX_NATIVE_ID_BYTES} bytes without control characters or ':'"
        ));
    }
    Ok(())
}

fn is_valid_native_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_NATIVE_ID_BYTES
        && !value.contains(':')
        && !value.chars().any(char::is_control)
}

#[cfg(all(test, unix))]
mod boundary_tests {
    use super::*;
    #[test]
    fn ordinal_files_cannot_follow_foreign_symlinks() {
        use std::os::unix::fs::symlink;
        let root =
            std::env::temp_dir().join(crate::learner::store::new_id("ordinal-boundary").unwrap());
        std::fs::create_dir_all(
            root.join(crate::workspace::CONTROL_DIR)
                .join("cache/host-ordinals"),
        )
        .unwrap();
        let workspace = Workspace::discover(&root).unwrap().unwrap();
        let foreign = root.join("foreign");
        std::fs::write(&foreign, b"PRIVATE").unwrap();
        let hash = stored_session_hash("thread");
        for extension in ["lock", "counter"] {
            let path = root
                .join(crate::workspace::CONTROL_DIR)
                .join(format!("cache/host-ordinals/{hash}.{extension}"));
            symlink(&foreign, &path).unwrap();
            assert!(allocate_native_ordinal(&workspace, "thread").is_err());
            assert_eq!(std::fs::read(&foreign).unwrap(), b"PRIVATE");
            std::fs::remove_file(path).unwrap();
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
