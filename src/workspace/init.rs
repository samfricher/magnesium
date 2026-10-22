use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::learner::store::LearnerStore;

use super::config::{ConfigError, WorkspaceConfig};
use super::{
    CONTROL_DIR, GlobalPaths, create_private_dir_all, os_random_hex, restrict_private_path,
    write_private_atomic,
};

const CONTROL_DIRS: &[&str] = &["index", "cache", "scratch", "backups"];

const MARKETPLACE_MANIFEST: &str = include_str!("../../.agents/plugins/marketplace.json");
const PLUGIN_MANIFEST: &str = include_str!("../../codex-plugin/.codex-plugin/plugin.json");
const MCP_MANIFEST: &str = include_str!("../../codex-plugin/.mcp.json");
const HOOKS_MANIFEST: &str = include_str!("../../codex-plugin/hooks/hooks.json");
const SKILL_MD: &str = include_str!("../../codex-plugin/skills/learn/SKILL.md");
const OPENCODE_PLUGIN_JS: &str = include_str!("../../opencode-plugin/magnesium.js");
const OPENCODE_SKILL_MD: &str = include_str!("../../opencode-plugin/skills/learn/SKILL.md");
const OPENCODE_SCHEMA: &str = "https://opencode.ai/config.json";
const OPENCODE_ADAPTER_PROTOCOL_VERSION: &str = "2";
const AGENTS_MD_BEGIN: &str = "<!-- magnesium:begin -->";
const AGENTS_MD_END: &str = "<!-- magnesium:end -->";

fn agents_md_block(newline: &str) -> String {
    format!(
        "{AGENTS_MD_BEGIN}{newline}\
When the nearest initialized {CONTROL_DIR} workspace is active, read its{newline}\
{CONTROL_DIR}/INSTRUCTIONS.md and use the Magnesium adapter API rather than editing {CONTROL_DIR}.{newline}\
{AGENTS_MD_END}{newline}"
    )
}

pub(crate) fn run_init(
    requested_path: Option<&Path>,
    adapter: Option<&str>,
    upgrade_instructions: bool,
) -> ExitCode {
    match initialize(requested_path, adapter, upgrade_instructions) {
        Ok(report) => {
            report.print();
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("magnesium init failed: {error}");
            eprintln!(
                "No existing workspace files were replaced. Fix the reported path and rerun `magnesium init` to resume."
            );
            ExitCode::from(1)
        }
    }
}

pub(crate) fn run_init_json(
    requested_path: Option<&Path>,
    adapter: Option<&str>,
    upgrade_instructions: bool,
) -> ExitCode {
    match init_report_json(requested_path, adapter, upgrade_instructions) {
        Ok(mut value) => {
            value["ok"] = json!(true);
            println!(
                "{}",
                serde_json::to_string(&value).expect("init report is JSON-serializable")
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            let value = json!({"ok": false, "error": error});
            println!("{value}");
            eprintln!("magnesium init failed: {error}");
            ExitCode::from(1)
        }
    }
}

pub(crate) fn init_report_json(
    requested_path: Option<&Path>,
    adapter: Option<&str>,
    upgrade_instructions: bool,
) -> Result<Value, String> {
    initialize(requested_path, adapter, upgrade_instructions)
        .map(|report| report.to_json())
        .map_err(|error| error.to_string())
}

fn initialize(
    requested_path: Option<&Path>,
    adapter: Option<&str>,
    upgrade_instructions: bool,
) -> Result<InitReport, InitError> {
    let supplied = requested_path.unwrap_or_else(|| Path::new("."));
    let root = fs::canonicalize(supplied).map_err(|source| InitError::Io {
        action: "resolve workspace path",
        path: supplied.to_path_buf(),
        source,
    })?;
    if !root.is_dir() {
        return Err(InitError::Invalid(format!(
            "workspace path {} is not a directory",
            root.display()
        )));
    }
    if let Some(adapter_name) = adapter {
        match adapter_name {
            "agents-md" => {}
            other => {
                return Err(InitError::Invalid(format!(
                    "unsupported adapter type `{other}`; supported values: `agents-md`"
                )));
            }
        }
    }

    let control = root.join(CONTROL_DIR);
    let mut report = InitReport::new(root.clone());
    ensure_private_dir(&control, &mut report)?;
    for directory in CONTROL_DIRS {
        ensure_private_dir(&control.join(directory), &mut report)?;
    }
    reject_managed_file_symlinks(&control)?;

    let config_path = control.join("config.toml");
    let config_existed = config_path.try_exists().map_err(|source| InitError::Io {
        action: "inspect Magnesium config",
        path: config_path.clone(),
        source,
    })?;
    let config = if config_existed {
        let config = WorkspaceConfig::load(&config_path)?;
        report.verified(&config_path);
        config
    } else {
        let workspace_id = generate_workspace_id()?;
        let mut config = WorkspaceConfig::new(config_path.clone(), &workspace_id)?;
        config.save()?;
        report.created(&config_path);
        config
    };

    let global_paths = GlobalPaths::resolve().map_err(|source| InitError::Io {
        action: "resolve platform data directory",
        path: root.clone(),
        source,
    })?;
    let state_path = control.join("state.sqlite");
    report.learner_state = if state_path.try_exists().map_err(|source| InitError::Io {
        action: "inspect canonical store",
        path: state_path.clone(),
        source,
    })? {
        LearnerStore::open(&state_path, config.workspace_id())
            .map_err(|error| InitError::Invalid(error.to_string()))?;
        report.verified(&state_path);
        LearnerState::Preserved
    } else {
        let connection = LearnerStore::initialize(&state_path, config.workspace_id())
            .map_err(|error| InitError::Invalid(error.to_string()))?;
        drop(connection);
        report.created(&state_path);
        report_sqlite_sidecars(&state_path, &mut report)?;
        LearnerState::Empty
    };

    // Native-start tickets use a local provenance key, never provider credentials.
    // Create it during explicit init so ordinary inactive hooks store no text.
    crate::adapter::native_capture::initialize(&super::Workspace {
        root: root.clone(),
        control: control.clone(),
    })
    .map_err(|message| InitError::Invalid(format!("native capture initialization: {message}")))?;

    let search_path = control.join("index/source-search.sqlite");
    ensure_empty_search_database(&search_path, &mut report)?;

    let magnesiumignore_path = control.join("magnesiumignore");
    if magnesiumignore_path
        .try_exists()
        .map_err(|source| InitError::Io {
            action: "inspect magnesiumignore",
            path: magnesiumignore_path.clone(),
            source,
        })?
    {
        report.verified(&magnesiumignore_path);
    } else {
        write_private_atomic(&magnesiumignore_path, b"").map_err(|source| InitError::Io {
            action: "create magnesiumignore",
            path: magnesiumignore_path.clone(),
            source,
        })?;
        report.created(&magnesiumignore_path);
    }

    // Install the exact approved bundle and activate its version pointer.
    // Existing version directories containing edits are never overwritten.
    let instruction_bundle_dir = control.join("instructions/v2");
    let bundle_existed = instruction_bundle_dir
        .try_exists()
        .map_err(|source| InitError::Io {
            action: "inspect stage-specific instruction bundle",
            path: instruction_bundle_dir.clone(),
            source,
        })?;
    let installed = if upgrade_instructions {
        crate::workspace::instruction_bundle::upgrade(&control)
    } else {
        crate::workspace::instruction_bundle::install(&control)
    }
    .map_err(|message| {
        InitError::Invalid(format!("stage-specific instruction bundle: {message}"))
    })?;
    if let crate::workspace::instruction_bundle::InstallDisposition::Upgraded { previous } =
        installed
    {
        report.instructions_upgraded_from = Some(previous);
    }
    if bundle_existed {
        report.verified(&instruction_bundle_dir);
    } else {
        report.created(&instruction_bundle_dir);
    }

    let instructions_path = control.join(crate::adapter::INSTRUCTIONS_FILENAME);
    let instructions_bytes = crate::adapter::HARNESS_INSTRUCTIONS.as_bytes();
    if instructions_path
        .try_exists()
        .map_err(|source| InitError::Io {
            action: "inspect instructions",
            path: instructions_path.clone(),
            source,
        })?
    {
        let existing = fs::read(&instructions_path).map_err(|source| InitError::Io {
            action: "read instructions",
            path: instructions_path.clone(),
            source,
        })?;
        if existing == instructions_bytes {
            report.verified(&instructions_path);
        } else {
            write_private_atomic(&instructions_path, instructions_bytes).map_err(|source| {
                InitError::Io {
                    action: "update instructions",
                    path: instructions_path.clone(),
                    source,
                }
            })?;
            report.modified(&instructions_path);
        }
    } else {
        write_private_atomic(&instructions_path, instructions_bytes).map_err(|source| {
            InitError::Io {
                action: "create instructions",
                path: instructions_path.clone(),
                source,
            }
        })?;
        report.created(&instructions_path);
    }

    if let Some(exclude_path) = add_git_exclusion(&root)? {
        match exclude_path.change {
            Change::Created => report.created(&exclude_path.path),
            Change::Modified => report.modified(&exclude_path.path),
            Change::Verified => report.verified(&exclude_path.path),
        }
    }

    if let Some(adapter_name) = adapter {
        match adapter_name {
            "agents-md" => apply_agents_md_adapter(&root, &mut report)?,
            other => {
                return Err(InitError::Invalid(format!(
                    "unsupported adapter type `{other}`; supported values: `agents-md`"
                )));
            }
        }
    }

    let (plugin_status, needs_approval) = setup_codex_plugin(&root, &global_paths.marketplace());
    report.plugin = plugin_status;
    report.needs_hook_approval = needs_approval;
    report.opencode = Some(install_opencode_adapter());
    let claude = setup_claude_plugin(&root, &global_paths.claude_plugin(), &mut report.warnings);
    if matches!(claude, ClaudePluginStatus::Enabled { .. }) {
        match add_git_exclusion_for_relative(&root, Path::new(".claude/settings.local.json")) {
            Ok(Some(exclude)) => match exclude.change {
                Change::Created => report.created(&exclude.path),
                Change::Modified => report.modified(&exclude.path),
                Change::Verified => report.verified(&exclude.path),
            },
            Ok(None) => {}
            Err(error) => report.warnings.push(format!(
                "warning: could not keep .claude/settings.local.json out of Git: {error}"
            )),
        }
    }
    report.claude = Some(claude);
    Ok(report)
}

fn ensure_private_dir(path: &Path, report: &mut InitReport) -> Result<(), InitError> {
    let existed = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(InitError::Invalid(format!(
                "managed Magnesium directory {} is a symbolic link; replace it with a real directory and rerun initialization",
                path.display()
            )));
        }
        Ok(metadata) if !metadata.is_dir() => {
            return Err(InitError::Invalid(format!(
                "managed Magnesium directory {} is not a directory",
                path.display()
            )));
        }
        Ok(_) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(source) => {
            return Err(InitError::Io {
                action: "inspect private directory",
                path: path.to_path_buf(),
                source,
            });
        }
    };
    create_private_dir_all(path).map_err(|source| InitError::Io {
        action: "create private directory",
        path: path.to_path_buf(),
        source,
    })?;
    if existed {
        report.verified(path);
    } else {
        report.created(path);
    }
    Ok(())
}

fn reject_managed_file_symlinks(control: &Path) -> Result<(), InitError> {
    for relative in [
        "config.toml",
        "INSTRUCTIONS.md",
        "state.sqlite",
        "state.sqlite-wal",
        "state.sqlite-shm",
        "index/source-search.sqlite",
        "magnesiumignore",
    ] {
        let path = control.join(relative);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(InitError::Invalid(format!(
                    "managed Magnesium file {} is a symbolic link; replace it with a real file and rerun initialization",
                    path.display()
                )));
            }
            Ok(metadata) if !metadata.is_file() => {
                return Err(InitError::Invalid(format!(
                    "managed Magnesium file {} is not a regular file",
                    path.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(InitError::Io {
                    action: "inspect managed Magnesium file",
                    path,
                    source,
                });
            }
        }
    }
    Ok(())
}

fn ensure_empty_search_database(path: &Path, report: &mut InitReport) -> Result<(), InitError> {
    let existed = path.try_exists().map_err(|source| InitError::Io {
        action: "inspect source-search database",
        path: path.to_path_buf(),
        source,
    })?;
    use crate::retrieval::{
        INDEX_DIAGNOSTIC_BUDGET, INDEX_INITIALIZATION_BUDGET, initialize_index,
        validate_index_integrity,
    };
    let cancellation = crate::retrieval::OperationCancellation::none();
    drop(
        initialize_index(path, INDEX_INITIALIZATION_BUDGET, &cancellation)
            .map_err(|error| InitError::Invalid(error.to_string()))?,
    );
    if existed {
        validate_index_integrity(path, INDEX_DIAGNOSTIC_BUDGET, &cancellation)
            .map_err(|error| InitError::Invalid(error.to_string()))?;
        report.verified(path);
    } else {
        restrict_private_path(path).map_err(|source| InitError::Io {
            action: "restrict source-search database",
            path: path.to_path_buf(),
            source,
        })?;
        report.created(path);
        report_sqlite_sidecars(path, report)?;
    }
    Ok(())
}

fn report_sqlite_sidecars(path: &Path, report: &mut InitReport) -> Result<(), InitError> {
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = OsString::from(path.as_os_str());
        sidecar.push(suffix);
        let sidecar = PathBuf::from(sidecar);
        if sidecar.try_exists().map_err(|source| InitError::Io {
            action: "inspect SQLite sidecar",
            path: sidecar.clone(),
            source,
        })? {
            report.created(sidecar);
        }
    }
    Ok(())
}

fn generate_workspace_id() -> Result<String, InitError> {
    let random = os_random_hex(16).map_err(|source| InitError::Io {
        action: "generate workspace identity",
        path: PathBuf::from("operating-system random source"),
        source,
    })?;
    Ok(format!("ws_{:016x}{random}", now_unix_ms()?))
}

fn now_unix_ms() -> Result<i64, InitError> {
    let milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|source| {
            InitError::Invalid(format!("system clock precedes Unix epoch: {source}"))
        })?
        .as_millis();
    i64::try_from(milliseconds)
        .map_err(|_| InitError::Invalid("system time exceeds the supported range".to_owned()))
}

struct ExcludePath {
    path: PathBuf,
    change: Change,
}

enum Change {
    Created,
    Modified,
    Verified,
}

fn add_git_exclusion(root: &Path) -> Result<Option<ExcludePath>, InitError> {
    add_git_exclusion_for_relative(root, Path::new(CONTROL_DIR))
}

fn resolve_git_exclusion(
    root: &Path,
    managed: &Path,
) -> Result<Option<(PathBuf, Vec<u8>)>, InitError> {
    let root = fs::canonicalize(root).map_err(|source| InitError::Io {
        action: "resolve workspace path for Git exclusion",
        path: root.to_path_buf(),
        source,
    })?;
    let Some(repository) = git_path_output(&root, &["rev-parse", "--show-toplevel"])? else {
        return Ok(None);
    };
    let repository = fs::canonicalize(&repository).map_err(|source| InitError::Io {
        action: "resolve Git repository root",
        path: repository,
        source,
    })?;
    let relative = root.strip_prefix(&repository).map_err(|_| {
        InitError::Invalid(format!(
            "Git reported repository {} outside workspace {}",
            repository.display(),
            root.display()
        ))
    })?;
    let pattern = git_exclude_pattern(relative, managed)?;
    let exclude = git_path_output(&root, &["rev-parse", "--git-path", "info/exclude"])?
        .ok_or_else(|| InitError::Invalid("Git did not report an info/exclude path".to_owned()))?;
    Ok(Some((
        if exclude.is_absolute() {
            exclude
        } else {
            root.join(exclude)
        },
        pattern,
    )))
}

fn add_git_exclusion_for_relative(
    root: &Path,
    managed: &Path,
) -> Result<Option<ExcludePath>, InitError> {
    let Some((exclude, pattern)) = resolve_git_exclusion(root, managed)? else {
        return Ok(None);
    };
    let existed = match fs::symlink_metadata(&exclude) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(InitError::Invalid(format!(
                "Git private exclude {} is a symbolic link; replace it with a regular file before rerunning initialization",
                exclude.display()
            )));
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err(InitError::Invalid(format!(
                "Git private exclude {} is not a regular file",
                exclude.display()
            )));
        }
        Ok(_) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(source) => {
            return Err(InitError::Io {
                action: "inspect Git private exclude",
                path: exclude.clone(),
                source,
            });
        }
    };
    let current = if existed {
        fs::read(&exclude).map_err(|source| InitError::Io {
            action: "read Git private exclude",
            path: exclude.clone(),
            source,
        })?
    } else {
        Vec::new()
    };
    if current
        .split(|byte| *byte == b'\n')
        .any(|line| line.strip_suffix(b"\r").unwrap_or(line) == pattern)
    {
        return Ok(Some(ExcludePath {
            path: exclude,
            change: Change::Verified,
        }));
    }
    if let Some(parent) = exclude.parent() {
        fs::create_dir_all(parent).map_err(|source| InitError::Io {
            action: "create Git info directory",
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&exclude)
        .map_err(|source| InitError::Io {
            action: "open Git private exclude",
            path: exclude.clone(),
            source,
        })?;
    if !current.is_empty() && !current.ends_with(b"\n") {
        file.write_all(b"\n").map_err(|source| InitError::Io {
            action: "separate Git private exclusion",
            path: exclude.clone(),
            source,
        })?;
    }
    file.write_all(&pattern).map_err(|source| InitError::Io {
        action: "write Git private exclusion",
        path: exclude.clone(),
        source,
    })?;
    file.write_all(b"\n").map_err(|source| InitError::Io {
        action: "terminate Git private exclusion",
        path: exclude.clone(),
        source,
    })?;
    file.sync_all().map_err(|source| InitError::Io {
        action: "persist Git private exclusion",
        path: exclude.clone(),
        source,
    })?;
    Ok(Some(ExcludePath {
        path: exclude,
        change: if existed {
            Change::Modified
        } else {
            Change::Created
        },
    }))
}

fn git_path_output(root: &Path, arguments: &[&str]) -> Result<Option<PathBuf>, InitError> {
    let output = match Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    {
        Ok(output) => output,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(InitError::Io {
                action: "run Git",
                path: root.to_path_buf(),
                source,
            });
        }
    };
    if !output.status.success() {
        return Ok(None);
    }
    let bytes = output.stdout.strip_suffix(b"\n").unwrap_or(&output.stdout);
    let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
    decode_git_path(bytes).map(Some)
}

fn git_exclude_pattern(relative: &Path, managed: &Path) -> Result<Vec<u8>, InitError> {
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(value) => {
                let escaped = encode_git_pattern_component(value);
                parts.push(escaped);
            }
            Component::CurDir => {}
            _ => {
                return Err(InitError::Invalid(
                    "Git workspace path is not relative to its repository root".to_owned(),
                ));
            }
        }
    }
    let mut managed_parts = Vec::new();
    for component in managed.components() {
        match component {
            Component::Normal(value) => managed_parts.push(encode_git_pattern_component(value)),
            Component::CurDir => {}
            _ => {
                return Err(InitError::Invalid(
                    "Git exclusion path must be workspace-relative".to_owned(),
                ));
            }
        }
    }
    if managed_parts.is_empty() {
        return Err(InitError::Invalid("Git exclusion path is empty".to_owned()));
    }
    if parts.is_empty() {
        let mut pattern = Vec::new();
        for (index, part) in managed_parts.iter().enumerate() {
            if index > 0 {
                pattern.push(b'/');
            }
            pattern.extend_from_slice(part);
        }
        pattern.push(b'/');
        Ok(pattern)
    } else {
        let mut pattern = vec![b'/'];
        for (index, part) in parts.iter().enumerate() {
            if index > 0 {
                pattern.push(b'/');
            }
            pattern.extend_from_slice(part);
        }
        for part in managed_parts {
            pattern.push(b'/');
            pattern.extend_from_slice(&part);
        }
        pattern.push(b'/');
        Ok(pattern)
    }
}

#[cfg(unix)]
fn decode_git_path(bytes: &[u8]) -> Result<PathBuf, InitError> {
    use std::os::unix::ffi::OsStringExt;

    Ok(PathBuf::from(OsString::from_vec(bytes.to_vec())))
}

#[cfg(not(unix))]
fn decode_git_path(bytes: &[u8]) -> Result<PathBuf, InitError> {
    String::from_utf8(bytes.to_vec())
        .map(PathBuf::from)
        .map_err(|_| InitError::Invalid("Git returned a non-UTF-8 path".to_owned()))
}

#[cfg(unix)]
fn encode_git_pattern_component(value: &std::ffi::OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;

    escape_git_pattern(value.as_bytes())
}

#[cfg(not(unix))]
fn encode_git_pattern_component(value: &std::ffi::OsStr) -> Vec<u8> {
    escape_git_pattern(value.to_string_lossy().as_bytes())
}

fn escape_git_pattern(value: &[u8]) -> Vec<u8> {
    let mut escaped = Vec::with_capacity(value.len());
    for byte in value {
        if matches!(byte, b'\\' | b'[' | b'*' | b'?') {
            escaped.push(b'\\');
        }
        escaped.push(*byte);
    }
    escaped
}

fn codex_binary() -> OsString {
    std::env::var_os("CODEX_TEST_BINARY").unwrap_or_else(|| "codex".into())
}

fn non_empty_env(name: &str) -> Option<OsString> {
    std::env::var_os(name).filter(|value| !value.is_empty())
}

fn opencode_binary() -> OsString {
    non_empty_env("OPENCODE_TEST_BINARY").unwrap_or_else(|| "opencode".into())
}

fn detect_opencode() -> bool {
    Command::new(opencode_binary())
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .is_ok_and(|output| output.status.success())
}

fn opencode_config_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if let Some(app_data) = non_empty_env("APPDATA") {
            return Some(PathBuf::from(app_data).join("opencode"));
        }
    }
    if let Some(config_home) = non_empty_env("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(config_home).join("opencode"));
    }
    non_empty_env("HOME")
        .or_else(|| non_empty_env("USERPROFILE"))
        .map(|home| PathBuf::from(home).join(".config").join("opencode"))
}

fn escape_javascript_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\u{2028}' | '\u{2029}' => {
                escaped.push_str(&format!("\\u{:04x}", character as u32));
            }
            other => escaped.push(other),
        }
    }
    escaped
}

fn opencode_plugin_bytes(binary: &Path, version: &str) -> Vec<u8> {
    OPENCODE_PLUGIN_JS
        .replace("__MAGNESIUM_VERSION__", version)
        .replacen(
            "__MAGNESIUM_BINARY__",
            &escape_javascript_string(&binary.to_string_lossy()),
            1,
        )
        .into_bytes()
}

fn opencode_mcp_entry(binary: &Path, version: &str) -> Value {
    json!({
        "type": "local",
        "command": [
            binary.to_string_lossy(),
            "_internal",
            "mcp-server",
            "--plugin-version",
            version,
            "--adapter-protocol-version",
            OPENCODE_ADAPTER_PROTOCOL_VERSION,
            // OpenCode can run inside a Claude Code terminal, which exports
            // CLAUDE_CODE_SESSION_ID; only OpenCode's own session may bind.
            "--host-kind",
            "opencode",
        ],
        "enabled": true,
    })
}

enum OpenCodePluginChange {
    Present,
    Installed,
    Updated,
}

fn install_opencode_plugin(
    config_dir: &Path,
    binary: &Path,
    version: &str,
) -> Result<OpenCodePluginChange, String> {
    install_opencode_asset(
        config_dir,
        &config_dir.join("plugins").join("magnesium.js"),
        &opencode_plugin_bytes(binary, version),
    )
}

fn install_opencode_skill(config_dir: &Path) -> Result<OpenCodePluginChange, String> {
    install_opencode_asset(
        config_dir,
        &config_dir.join("skills").join("learn").join("SKILL.md"),
        OPENCODE_SKILL_MD.as_bytes(),
    )
}

/// Reject a symlink anywhere between the trusted config directory and the
/// asset parent, so `skills -> elsewhere` cannot redirect a Magnesium-owned write.
fn reject_symlinked_asset_components(config_dir: &Path, parent: &Path) -> Result<(), String> {
    let Ok(relative) = parent.strip_prefix(config_dir) else {
        return Ok(());
    };
    let mut current = config_dir.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "the OpenCode directory {} is a symbolic link; replace it with a real directory",
                    current.display()
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(format!(
                    "the OpenCode path {} is not a directory",
                    current.display()
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

fn install_opencode_asset(
    config_dir: &Path,
    asset_path: &Path,
    bytes: &[u8],
) -> Result<OpenCodePluginChange, String> {
    let Some(parent) = asset_path.parent() else {
        return Err(format!(
            "the OpenCode asset path {} has no parent directory",
            asset_path.display()
        ));
    };
    let parent = parent.to_path_buf();
    reject_symlinked_asset_components(config_dir, &parent)?;
    match fs::symlink_metadata(&parent) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(format!(
                "the OpenCode directory {} is a symbolic link; replace it with a real directory",
                parent.display()
            ));
        }
        Ok(metadata) if !metadata.is_dir() => {
            return Err(format!(
                "the OpenCode path {} is not a directory",
                parent.display()
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "could not inspect the OpenCode directory {}: {error}",
                parent.display()
            ));
        }
    }

    let existing = match fs::symlink_metadata(asset_path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(format!(
                "the installed OpenCode file {} is a symbolic link; replace it with a regular file",
                asset_path.display()
            ));
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err(format!(
                "the installed OpenCode file {} is not a regular file",
                asset_path.display()
            ));
        }
        Ok(_) => Some(
            fs::read(asset_path)
                .map_err(|error| format!("could not read the installed OpenCode file: {error}"))?,
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(format!(
                "could not inspect the installed OpenCode file: {error}"
            ));
        }
    };

    match existing {
        Some(current) if current == bytes => Ok(OpenCodePluginChange::Present),
        Some(_) => {
            write_private_atomic(asset_path, bytes).map_err(|error| {
                format!(
                    "could not update the OpenCode file {}: {error}",
                    asset_path.display()
                )
            })?;
            Ok(OpenCodePluginChange::Updated)
        }
        None => {
            write_private_atomic(asset_path, bytes).map_err(|error| {
                format!(
                    "could not install the OpenCode file {}: {error}",
                    asset_path.display()
                )
            })?;
            Ok(OpenCodePluginChange::Installed)
        }
    }
}

fn register_opencode_mcp(config_dir: &Path, binary: &Path, version: &str) -> OpenCodeMcpStatus {
    let entry = opencode_mcp_entry(binary, version);
    let entry_string = serde_json::to_string(&entry).unwrap_or_default();
    let manual = |path: PathBuf| OpenCodeMcpStatus::Manual {
        path,
        entry: entry_string.clone(),
    };

    let jsonc_path = config_dir.join("opencode.jsonc");
    if fs::symlink_metadata(&jsonc_path).is_ok() {
        return manual(jsonc_path);
    }

    let json_path = config_dir.join("opencode.json");
    match fs::symlink_metadata(&json_path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            manual(json_path)
        }
        Ok(_) => {
            let Ok(bytes) = fs::read(&json_path) else {
                return manual(json_path);
            };
            let Ok(mut value) = serde_json::from_slice::<Value>(&bytes) else {
                return manual(json_path);
            };
            let Some(root) = value.as_object_mut() else {
                return manual(json_path);
            };
            if !root.contains_key("mcp") {
                root.insert("mcp".to_owned(), Value::Object(serde_json::Map::new()));
            }
            let Some(mcp) = root.get_mut("mcp").and_then(Value::as_object_mut) else {
                return manual(json_path);
            };
            if mcp.get("magnesium") == Some(&entry) {
                return OpenCodeMcpStatus::Registered;
            }
            mcp.insert("magnesium".to_owned(), entry);
            let Ok(mut serialized) = serde_json::to_vec_pretty(&value) else {
                return manual(json_path);
            };
            serialized.push(b'\n');
            if crate::workspace::write_user_owned_atomic(&json_path, &serialized).is_err() {
                return manual(json_path);
            }
            OpenCodeMcpStatus::Registered
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let value = json!({
                "$schema": OPENCODE_SCHEMA,
                "mcp": {"magnesium": entry},
            });
            let Ok(mut serialized) = serde_json::to_vec_pretty(&value) else {
                return manual(json_path);
            };
            serialized.push(b'\n');
            if let Some(parent) = json_path.parent()
                && fs::create_dir_all(parent).is_err()
            {
                return manual(json_path);
            }
            if crate::workspace::write_user_owned_atomic(&json_path, &serialized).is_err() {
                return manual(json_path);
            }
            OpenCodeMcpStatus::Registered
        }
        Err(_) => manual(json_path),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum OpenCodeAdapterStatus {
    NotDetected,
    Installed { version: String },
    Updated { version: String },
    Rejected { reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum OpenCodeMcpStatus {
    Registered,
    Manual { path: PathBuf, entry: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OpenCodeAdapterOutcome {
    adapter: OpenCodeAdapterStatus,
    mcp: Option<OpenCodeMcpStatus>,
}

impl OpenCodeAdapterOutcome {
    pub(crate) fn print(&self) {
        match &self.adapter {
            OpenCodeAdapterStatus::NotDetected => println!("OpenCode adapter: not detected"),
            OpenCodeAdapterStatus::Installed { version } => {
                println!("OpenCode adapter: installed (version {version})");
            }
            OpenCodeAdapterStatus::Updated { version } => {
                println!("OpenCode adapter: updated (version {version})");
            }
            OpenCodeAdapterStatus::Rejected { reason } => {
                println!("OpenCode adapter: not installed; {reason}");
            }
        }
        match &self.mcp {
            Some(OpenCodeMcpStatus::Registered) => println!("OpenCode MCP: registered"),
            Some(OpenCodeMcpStatus::Manual { path, entry }) => {
                println!(
                    "OpenCode MCP: manual registration required ({})",
                    path.display()
                );
                println!("  merge into the file's \"mcp\" object: \"magnesium\": {entry}");
            }
            None => {}
        }
    }
}

pub(crate) fn install_opencode_adapter() -> OpenCodeAdapterOutcome {
    let version = env!("CARGO_PKG_VERSION");
    if !detect_opencode() {
        return OpenCodeAdapterOutcome {
            adapter: OpenCodeAdapterStatus::NotDetected,
            mcp: None,
        };
    }
    let Some(config_dir) = opencode_config_dir() else {
        return OpenCodeAdapterOutcome {
            adapter: OpenCodeAdapterStatus::Rejected {
                reason: "no per-user OpenCode config directory is available".to_owned(),
            },
            mcp: None,
        };
    };
    let executable = match std::env::current_exe() {
        Ok(executable) => executable,
        Err(error) => {
            return OpenCodeAdapterOutcome {
                adapter: OpenCodeAdapterStatus::Rejected {
                    reason: format!("could not resolve the running executable: {error}"),
                },
                mcp: None,
            };
        }
    };
    let adapter = match (
        install_opencode_plugin(&config_dir, &executable, version),
        install_opencode_skill(&config_dir),
    ) {
        (Err(reason), _) | (_, Err(reason)) => OpenCodeAdapterStatus::Rejected { reason },
        (Ok(OpenCodePluginChange::Updated), _) | (_, Ok(OpenCodePluginChange::Updated)) => {
            OpenCodeAdapterStatus::Updated {
                version: version.to_owned(),
            }
        }
        _ => OpenCodeAdapterStatus::Installed {
            version: version.to_owned(),
        },
    };
    let mcp = Some(register_opencode_mcp(&config_dir, &executable, version));
    OpenCodeAdapterOutcome { adapter, mcp }
}

const CLAUDE_PLUGIN_ID: &str = "magnesium@magnesium";
/// Claude Code's permission rule for every tool of the plugin's `magnesium` MCP server.
const CLAUDE_MCP_RULE: &str = "mcp__plugin_magnesium_magnesium__*";
const CLAUDE_PLUGIN_FILES: &[(&str, &str)] = &[
    (
        ".claude-plugin/marketplace.json",
        include_str!("../../claude-plugin/.claude-plugin/marketplace.json"),
    ),
    (
        ".claude-plugin/plugin.json",
        include_str!("../../claude-plugin/.claude-plugin/plugin.json"),
    ),
    (".mcp.json", include_str!("../../claude-plugin/.mcp.json")),
    (
        "hooks/hooks.json",
        include_str!("../../claude-plugin/hooks/hooks.json"),
    ),
    (
        "skills/learn/SKILL.md",
        include_str!("../../claude-plugin/skills/learn/SKILL.md"),
    ),
    (
        "skills/papers/SKILL.md",
        include_str!("../../claude-plugin/skills/papers/SKILL.md"),
    ),
    (
        "skills/papers/templates/plan.md",
        include_str!("../../claude-plugin/skills/papers/templates/plan.md"),
    ),
    (
        "skills/papers/templates/gap-log.md",
        include_str!("../../claude-plugin/skills/papers/templates/gap-log.md"),
    ),
    (
        "skills/papers/templates/graph.yaml",
        include_str!("../../claude-plugin/skills/papers/templates/graph.yaml"),
    ),
    (
        "skills/papers/templates/drills.md",
        include_str!("../../claude-plugin/skills/papers/templates/drills.md"),
    ),
    (
        "skills/papers/templates/past-papers-README.md",
        include_str!("../../claude-plugin/skills/papers/templates/past-papers-README.md"),
    ),
    (
        "skills/papers/templates/preamble.tex",
        include_str!("../../claude-plugin/skills/papers/templates/preamble.tex"),
    ),
    (
        "skills/papers/templates/build.sh",
        include_str!("../../claude-plugin/skills/papers/templates/build.sh"),
    ),
    (
        "skills/papers/templates/paper.md",
        include_str!("../../claude-plugin/skills/papers/templates/paper.md"),
    ),
    (
        "skills/help/SKILL.md",
        include_str!("../../claude-plugin/skills/help/SKILL.md"),
    ),
    (
        "skills/help/user-guide.md",
        include_str!("../../docs/user-guide.md"),
    ),
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ClaudePluginStatus {
    /// Declared in the user settings and enabled in this folder only.
    Enabled {
        settings: PathBuf,
        local: PathBuf,
        enabled_everywhere: bool,
        /// The permission rules this run added to `permissions.allow`.
        added_rules: Vec<String>,
    },
    /// This folder's settings turn magnesium@magnesium off; that choice is kept.
    Disabled {
        local: PathBuf,
    },
    Rejected {
        reason: String,
    },
}

/// Claude Code's user settings file, which its CLI and desktop app share.
fn claude_settings_path() -> Option<PathBuf> {
    non_empty_env("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| non_empty_env("HOME").map(|home| PathBuf::from(home).join(".claude")))
        .map(|dir| dir.join("settings.json"))
}

fn same_directory(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

/// Read a JSON settings object, or an empty one when the file is absent.
/// Symbolic links and non-object JSON are refused rather than rewritten.
fn read_settings_object(path: &Path) -> Result<serde_json::Map<String, Value>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(format!(
            "{} is a symbolic link; Magnesium does not edit it",
            path.display()
        )),
        Ok(metadata) if !metadata.is_file() => {
            Err(format!("{} is not a regular file", path.display()))
        }
        Ok(_) => {
            let bytes =
                fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
            match serde_json::from_slice::<Value>(&bytes) {
                Ok(Value::Object(object)) => Ok(object),
                _ => Err(format!(
                    "{} is not a JSON object; Magnesium did not change it",
                    path.display()
                )),
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(serde_json::Map::new()),
        Err(error) => Err(format!("could not inspect {}: {error}", path.display())),
    }
}

fn write_settings_object(
    path: &Path,
    object: serde_json::Map<String, Value>,
) -> Result<(), String> {
    let mut bytes = serde_json::to_vec_pretty(&Value::Object(object))
        .map_err(|e| format!("could not serialize {}: {e}", path.display()))?;
    bytes.push(b'\n');
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    crate::workspace::write_user_owned_atomic(path, &bytes)
        .map_err(|e| format!("could not write {}: {e}", path.display()))
}

/// Declare the plugin directory as the `magnesium` plugin source in the user's
/// Claude settings without enabling it anywhere; each study folder enables it.
/// A different `magnesium` entry is kept. Returns whether the user settings also
/// enable Magnesium for every folder.
fn declare_claude_marketplace(settings: &Path, plugin_dir: &Path) -> Result<bool, String> {
    let mut object = read_settings_object(settings)?;
    let enabled_everywhere = object
        .get("enabledPlugins")
        .and_then(|plugins| plugins.get(CLAUDE_PLUGIN_ID))
        == Some(&Value::Bool(true));
    let marketplaces = object
        .entry("extraKnownMarketplaces")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| {
            format!(
                "{}: extraKnownMarketplaces is not an object",
                settings.display()
            )
        })?;
    match marketplaces.get("magnesium") {
        Some(existing) => {
            let declared = existing
                .pointer("/source/path")
                .and_then(Value::as_str)
                .filter(|_| existing.pointer("/source/source") == Some(&json!("directory")));
            match declared {
                Some(path) if same_directory(Path::new(path), plugin_dir) => Ok(enabled_everywhere),
                _ => Err(format!(
                    "{} already declares a different `magnesium` plugin source ({existing}); it was kept, so point it at {} to use this Magnesium",
                    settings.display(),
                    plugin_dir.display()
                )),
            }
        }
        None => {
            marketplaces.insert(
                "magnesium".to_owned(),
                json!({"source": {"source": "directory", "path": plugin_dir}}),
            );
            write_settings_object(settings, object)?;
            Ok(enabled_everywhere)
        }
    }
}

/// The rules that let Magnesium run in a study folder without permission prompts:
/// every tool of the plugin's MCP server, reads of the copied plugin, where
/// the skills keep their templates, a launch of each shipped skill, and the
/// read-only `magnesium digest` that /magnesium:help runs. Claude Code launches Magnesium's
/// skills without a Skill rule today; the rules keep launches prompt-free if a
/// release starts to ask. A skill's own allowed-tools would block plain-words
/// launches, so /magnesium:help gets its command from here.
fn claude_permission_rules(plugin_dir: &Path) -> Vec<String> {
    let mut rules = vec![
        CLAUDE_MCP_RULE.to_owned(),
        format!("Read({}/**)", claude_absolute_path(plugin_dir)),
    ];
    rules.extend(
        CLAUDE_PLUGIN_FILES
            .iter()
            .filter_map(|(path, _)| path.strip_prefix("skills/")?.strip_suffix("/SKILL.md"))
            .map(|skill| format!("Skill(magnesium:{skill})")),
    );
    rules.push("Bash(magnesium digest)".to_owned());
    rules
}

/// An absolute path as a Claude Code permission rule writes it: `//` and the
/// path in POSIX form, so `C:\Users` on Windows becomes `//c/Users`.
fn claude_absolute_path(path: &Path) -> String {
    let mut rule = String::from("/");
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => match prefix.kind() {
                std::path::Prefix::Disk(drive) | std::path::Prefix::VerbatimDisk(drive) => {
                    rule.push('/');
                    rule.push(char::from(drive).to_ascii_lowercase());
                }
                _ => rule.push_str(&prefix.as_os_str().to_string_lossy().replace('\\', "/")),
            },
            Component::RootDir | Component::CurDir => {}
            part => {
                rule.push('/');
                rule.push_str(&part.as_os_str().to_string_lossy());
            }
        }
    }
    rule
}

/// Enable Magnesium for this folder only, in its personal (uncommitted) Claude
/// settings, and add `rules` to `permissions.allow` there. An explicit `false`
/// for the plugin is the user's choice and is kept. The user's own `ask` and
/// `deny` rules stay as they are: Claude Code applies them before `allow`.
/// When `permissions` has another shape, it is kept and a warning is added.
/// Returns the rules it added, or `None` when the folder turns Magnesium off.
fn enable_claude_plugin_here(
    root: &Path,
    rules: &[String],
    warnings: &mut Vec<String>,
) -> Result<Option<Vec<String>>, String> {
    let local = root.join(".claude/settings.local.json");
    crate::workspace::reject_symlink_components(&local, root)
        .map_err(|e| format!("{}: {e}", local.display()))?;
    let mut object = read_settings_object(&local)?;
    let plugins = object
        .entry("enabledPlugins")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| format!("{}: enabledPlugins is not an object", local.display()))?;
    let enabled_now = match plugins.get(CLAUDE_PLUGIN_ID) {
        Some(Value::Bool(true)) => false,
        Some(Value::Bool(false)) => return Ok(None),
        _ => {
            plugins.insert(CLAUDE_PLUGIN_ID.to_owned(), Value::Bool(true));
            true
        }
    };
    let allow = object
        .entry("permissions")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .map(|permissions| permissions.entry("allow").or_insert_with(|| json!([])))
        .and_then(Value::as_array_mut);
    let mut added = Vec::new();
    match allow {
        Some(allow) => {
            for rule in rules {
                if !allow.contains(&json!(rule)) {
                    allow.push(json!(rule));
                    added.push(rule.clone());
                }
            }
        }
        None => warnings.push(format!(
            "warning: {}: permissions is not an object with an allow list, so Magnesium added no permission rules there; Claude Code will ask before Magnesium's tools run",
            local.display()
        )),
    }
    if enabled_now || !added.is_empty() {
        write_settings_object(&local, object)?;
    }
    Ok(Some(added))
}

/// Copy the bundled plugin to a stable per-user directory, declare it in the
/// user's Claude settings, and enable it for this folder only, so Claude Code
/// loads Magnesium when opened here and nowhere else. Claude loads the directory
/// in place, so rerunning `magnesium init` after an upgrade refreshes it.
fn setup_claude_plugin(
    root: &Path,
    plugin_dir: &Path,
    warnings: &mut Vec<String>,
) -> ClaudePluginStatus {
    if let Err(reason) = write_claude_plugin_files(plugin_dir) {
        return ClaudePluginStatus::Rejected { reason };
    }
    let Some(settings) = claude_settings_path() else {
        return ClaudePluginStatus::Rejected {
            reason: "no Claude Code settings directory (set HOME or CLAUDE_CONFIG_DIR)".to_owned(),
        };
    };
    let enabled_everywhere = match declare_claude_marketplace(&settings, plugin_dir) {
        Ok(enabled) => enabled,
        Err(reason) => return ClaudePluginStatus::Rejected { reason },
    };
    let local = root.join(".claude/settings.local.json");
    match enable_claude_plugin_here(root, &claude_permission_rules(plugin_dir), warnings) {
        Ok(Some(added_rules)) => ClaudePluginStatus::Enabled {
            settings,
            local,
            enabled_everywhere,
            added_rules,
        },
        Ok(None) => ClaudePluginStatus::Disabled { local },
        Err(reason) => ClaudePluginStatus::Rejected { reason },
    }
}

/// Write the bundled Claude plugin into its managed directory, skipping files
/// that already match.
fn write_claude_plugin_files(plugin_dir: &Path) -> Result<(), String> {
    validate_claude_plugin_paths(plugin_dir)?;
    let mut writes = crate::workspace::AtomicWrites::default();
    for (relative, content) in CLAUDE_PLUGIN_FILES {
        let dest = plugin_dir.join(relative);
        dest.parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| match fs::read_to_string(&dest) {
                Ok(existing) if existing == *content => Ok(()),
                _ => writes.write_private(&dest, content.as_bytes()),
            })
            .map_err(|e| format!("could not write {}: {e}", dest.display()))?;
    }
    writes
        .finish()
        .map_err(|e| format!("could not flush {}: {e}", plugin_dir.display()))
}

/// After a runtime upgrade, rewrite an existing Claude plugin copy: its hook
/// and MCP command lines pin the plugin version, so the previous copy fails
/// every version check. Returns false when init never wrote a copy here;
/// folder settings stay as init left them.
pub(crate) fn refresh_claude_plugin(plugin_dir: &Path) -> Result<bool, String> {
    if !plugin_dir.exists() {
        return Ok(false);
    }
    write_claude_plugin_files(plugin_dir).map(|()| true)
}

/// Check the managed plugin root and every packaged destination before any
/// create, read or write: no component from the root down may be a symbolic
/// link or reparse point, directories must be directories and files files.
/// Aliases above the managed root (such as /var on macOS) stay allowed.
fn validate_claude_plugin_paths(plugin_dir: &Path) -> Result<(), String> {
    let refuse = |path: &Path, problem: &str| {
        format!(
            "the managed Claude plugin path {} {problem}; replace it with a real directory or file, then rerun magnesium init",
            path.display()
        )
    };
    for (relative, _) in CLAUDE_PLUGIN_FILES {
        let destination = plugin_dir.join(relative);
        crate::workspace::reject_symlink_components(&destination, plugin_dir)
            .map_err(|error| refuse(&destination, &format!("is unsafe ({error})")))?;
        let mut current = destination.as_path();
        loop {
            match fs::symlink_metadata(current) {
                Ok(metadata) => {
                    #[cfg(windows)]
                    {
                        use std::os::windows::fs::MetadataExt;
                        if metadata.file_attributes() & 0x0000_0400 != 0 {
                            return Err(refuse(current, "is a reparse point"));
                        }
                    }
                    let expect_file = current == destination;
                    if expect_file && !metadata.is_file() {
                        return Err(refuse(current, "is not a regular file"));
                    }
                    if !expect_file && !metadata.is_dir() {
                        return Err(refuse(current, "is not a directory"));
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(refuse(current, &format!("cannot be inspected ({error})")));
                }
            }
            if current == plugin_dir {
                break;
            }
            current = current
                .parent()
                .ok_or_else(|| refuse(&destination, "has no managed root"))?;
        }
    }
    Ok(())
}

impl ClaudePluginStatus {
    fn print(&self) {
        match self {
            Self::Enabled {
                local,
                enabled_everywhere,
                added_rules,
                ..
            } => {
                println!(
                    "Claude Code plugin: enabled for this folder ({}); start a new Claude Code session here",
                    local.display()
                );
                if !added_rules.is_empty() {
                    println!(
                        "Claude Code plugin: allowed Magnesium's tools, plugin files and skills in .claude/settings.local.json"
                    );
                }
                if *enabled_everywhere {
                    println!(
                        "Claude Code plugin: your user settings also enable Magnesium in every folder; remove enabledPlugins.\"{CLAUDE_PLUGIN_ID}\" there to limit it to Magnesium folders"
                    );
                }
            }
            Self::Disabled { local } => println!(
                "Claude Code plugin: disabled for this folder by {}; that choice was kept",
                local.display()
            ),
            Self::Rejected { reason } => println!("Claude Code plugin: not enabled; {reason}"),
        }
    }

    fn to_json(&self) -> Value {
        match self {
            Self::Enabled {
                settings,
                local,
                enabled_everywhere,
                added_rules,
            } => json!({
                "status": "enabled",
                "plugin": CLAUDE_PLUGIN_ID,
                "user_settings": settings,
                "folder_settings": local,
                "enabled_everywhere": enabled_everywhere,
                "added_rules": added_rules,
            }),
            Self::Disabled { local } => {
                json!({"status": "disabled", "plugin": CLAUDE_PLUGIN_ID, "folder_settings": local})
            }
            Self::Rejected { reason } => json!({"status": "rejected", "reason": reason}),
        }
    }
}

pub(crate) fn materialize_bundled_marketplace(marketplace_dir: &Path) -> Result<(), io::Error> {
    let files: &[(&str, &str)] = &[
        (".agents/plugins/marketplace.json", MARKETPLACE_MANIFEST),
        ("codex-plugin/.codex-plugin/plugin.json", PLUGIN_MANIFEST),
        ("codex-plugin/.mcp.json", MCP_MANIFEST),
        ("codex-plugin/hooks/hooks.json", HOOKS_MANIFEST),
        ("codex-plugin/skills/learn/SKILL.md", SKILL_MD),
    ];
    let mut writes = crate::workspace::AtomicWrites::default();
    for (rel_path, content) in files {
        let dest = marketplace_dir.join(rel_path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let needs_write = match fs::read_to_string(&dest) {
            Ok(existing) => existing != *content,
            Err(_) => true,
        };
        if needs_write {
            writes.write_private(&dest, content.as_bytes())?;
        }
    }
    writes.finish()
}

fn ensure_codex_marketplace_registered(root: &Path, marketplace_dir: &Path) -> Result<(), String> {
    let list_output = Command::new(codex_binary())
        .args(["plugin", "marketplace", "list", "--json"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("could not execute codex plugin marketplace list: {e}"))?;
    if !list_output.status.success() {
        return Err(format!(
            "codex plugin marketplace list failed: {}",
            String::from_utf8_lossy(&list_output.stderr).trim()
        ));
    }
    let listing: serde_json::Value = serde_json::from_slice(&list_output.stdout)
        .map_err(|e| format!("invalid Codex marketplace listing: {e}"))?;
    let marketplaces = listing["marketplaces"]
        .as_array()
        .ok_or("Codex marketplace listing has no marketplaces array")?;
    let registered = marketplaces.iter().find(|m| m["name"] == "magnesium");
    if let Some(marketplace) = registered {
        let actual = marketplace["root"]
            .as_str()
            .map(Path::new)
            .and_then(|path| fs::canonicalize(path).ok());
        let expected = fs::canonicalize(marketplace_dir).ok();
        if actual.is_none() || actual != expected {
            return Err(format!(
                "Codex marketplace magnesium points to another source; expected {}. Existing registration was preserved",
                marketplace_dir.display()
            ));
        }
    }
    let already_registered = registered.is_some();
    if !already_registered {
        let mut cmd = Command::new(codex_binary());
        cmd.arg("plugin")
            .arg("marketplace")
            .arg("add")
            .arg(marketplace_dir)
            .arg("--json")
            .current_dir(root)
            .stdin(Stdio::null())
            .stderr(Stdio::piped());
        let add_output = cmd
            .output()
            .map_err(|e| format!("could not execute codex plugin marketplace add: {e}"))?;
        if !add_output.status.success() {
            let stderr = String::from_utf8_lossy(&add_output.stderr);
            return Err(format!(
                "codex plugin marketplace add failed with exit code {:?}: {}",
                add_output.status.code(),
                stderr.trim()
            ));
        }
    }
    Ok(())
}

fn is_magnesium_codex_plugin(plugin: &serde_json::Value) -> bool {
    plugin.get("pluginId").and_then(serde_json::Value::as_str) == Some("magnesium@magnesium")
}

fn ensure_codex_plugin_installed(root: &Path) -> Result<(), String> {
    let listing = Command::new(codex_binary())
        .args(["plugin", "list", "--json"])
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("could not list Codex plugins: {e}"))?;
    if !listing.status.success() {
        return Err(format!(
            "codex plugin list failed: {}",
            String::from_utf8_lossy(&listing.stderr).trim()
        ));
    }
    let listing: serde_json::Value = serde_json::from_slice(&listing.stdout)
        .map_err(|e| format!("invalid Codex plugin listing: {e}"))?;
    let installed = listing["installed"]
        .as_array()
        .ok_or("Codex plugin listing has no installed array")?;
    if installed
        .iter()
        .any(|p| is_magnesium_codex_plugin(p) && p["installed"] == true && p["enabled"] != true)
    {
        return Err(
            "Magnesium plugin is disabled in Codex; its enabled setting was preserved".to_owned(),
        );
    }

    // The supported add operation also refreshes same-version development builds.
    // Version equality alone cannot establish that the cached instruction bytes match.
    let output = Command::new(codex_binary())
        .args(["plugin", "add", "magnesium@magnesium", "--json"])
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("could not execute codex plugin add: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "codex plugin add failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let receipt: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("invalid Codex plugin installation receipt: {e}"))?;
    if !is_magnesium_codex_plugin(&receipt) || receipt["version"] != env!("CARGO_PKG_VERSION") {
        return Err("Codex installed a different plugin identity or version".to_owned());
    }
    let installed_path = receipt["installedPath"]
        .as_str()
        .map(Path::new)
        .filter(|p| p.is_absolute())
        .ok_or("Codex did not report an absolute installed plugin path")?;
    for (relative, expected) in [
        (".codex-plugin/plugin.json", PLUGIN_MANIFEST),
        (".mcp.json", MCP_MANIFEST),
        ("hooks/hooks.json", HOOKS_MANIFEST),
        ("skills/learn/SKILL.md", SKILL_MD),
    ] {
        let path = installed_path.join(relative);
        let actual = fs::read(&path)
            .map_err(|e| format!("could not verify installed plugin {}: {e}", path.display()))?;
        if actual != expected.as_bytes() {
            return Err(format!(
                "installed plugin {} differs from the bundled version",
                path.display()
            ));
        }
    }
    Ok(())
}

fn setup_codex_plugin(root: &Path, marketplace_dir: &Path) -> (PluginStatus, bool) {
    if let Err(e) = materialize_bundled_marketplace(marketplace_dir) {
        eprintln!("warning: could not cache bundled plugin marketplace: {e}");
        return (inspect_codex_plugin(root), false);
    }

    match Command::new(codex_binary())
        .arg("--version")
        .current_dir(root)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    {
        Ok(output) if output.status.success() => {}
        Ok(_) => return (PluginStatus::Unavailable, false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return (PluginStatus::CodexMissing, false);
        }
        Err(_) => return (PluginStatus::Unavailable, false),
    }

    if let Err(e) = ensure_codex_marketplace_registered(root, marketplace_dir) {
        eprintln!("warning: could not register marketplace: {e}");
        return (PluginStatus::Unavailable, false);
    }
    if let Err(e) = ensure_codex_plugin_installed(root) {
        eprintln!("warning: could not install plugin: {e}");
        return (PluginStatus::Unavailable, false);
    }
    let status = inspect_codex_plugin(root);
    let needs_approval = matches!(
        status,
        PluginStatus::Registered {
            trusted: Some(false),
            ..
        }
    );
    (status, needs_approval)
}

fn apply_agents_md_adapter(root: &Path, report: &mut InitReport) -> Result<(), InitError> {
    let path = root.join("AGENTS.md");
    match fs::symlink_metadata(&path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(InitError::Invalid(format!(
                    "{} is a symlink; refusing to edit symlinked user file",
                    path.display()
                )));
            }
            if !meta.is_file() {
                return Err(InitError::Invalid(format!(
                    "{} is not a regular file; refusing to edit non-regular user file",
                    path.display()
                )));
            }
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            crate::workspace::write_user_owned_atomic(&path, agents_md_block("\n").as_bytes())
                .map_err(|source| InitError::Io {
                    action: "create AGENTS.md",
                    path: path.clone(),
                    source,
                })?;
            report.created(&path);
            report.adapter = Some(AdapterKind::AgentsMd);
            return Ok(());
        }
        Err(source) => {
            return Err(InitError::Io {
                action: "inspect AGENTS.md",
                path: path.clone(),
                source,
            });
        }
    }

    let existing = fs::read_to_string(&path).map_err(|source| InitError::Io {
        action: "read AGENTS.md",
        path: path.clone(),
        source,
    })?;

    let begin_count = existing.matches(AGENTS_MD_BEGIN).count();
    let end_count = existing.matches(AGENTS_MD_END).count();
    if begin_count > 1 || end_count > 1 || begin_count != end_count {
        return Err(InitError::Invalid(
            "AGENTS.md contains duplicate or mismatched Magnesium marker blocks; refusing to edit"
                .to_owned(),
        ));
    }
    if begin_count == 1 {
        let start = existing.find(AGENTS_MD_BEGIN).unwrap();
        let end = existing.find(AGENTS_MD_END).unwrap();
        if start > end {
            return Err(InitError::Invalid(
                "AGENTS.md contains malformed Magnesium marker blocks (END precedes BEGIN); refusing to edit"
                    .to_owned(),
            ));
        }
    }

    let newline = if existing.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let agents_block = agents_md_block(newline);

    if begin_count == 1 {
        let start = existing.find(AGENTS_MD_BEGIN).unwrap();
        let end = existing.find(AGENTS_MD_END).unwrap();
        let end_after = end + AGENTS_MD_END.len();
        let end_pos = if existing[end_after..].starts_with("\r\n") {
            end_after + 2
        } else if existing[end_after..].starts_with('\n') {
            end_after + 1
        } else {
            end_after
        };
        let mut updated = String::with_capacity(existing.len() + agents_block.len());
        updated.push_str(&existing[..start]);
        updated.push_str(&agents_block);
        updated.push_str(&existing[end_pos..]);
        if updated == existing {
            report.verified(&path);
        } else {
            crate::workspace::write_user_owned_atomic(&path, updated.as_bytes()).map_err(
                |source| InitError::Io {
                    action: "update AGENTS.md adapter block",
                    path: path.clone(),
                    source,
                },
            )?;
            report.modified(&path);
        }
        report.adapter = Some(AdapterKind::AgentsMd);
        return Ok(());
    }

    let mut updated = existing.clone();
    if !updated.is_empty() && !updated.ends_with(newline) {
        updated.push_str(newline);
    }
    if !updated.is_empty() {
        updated.push_str(newline);
    }
    updated.push_str(&agents_block);
    crate::workspace::write_user_owned_atomic(&path, updated.as_bytes()).map_err(|source| {
        InitError::Io {
            action: "append AGENTS.md adapter block",
            path: path.clone(),
            source,
        }
    })?;
    report.modified(&path);
    report.adapter = Some(AdapterKind::AgentsMd);
    Ok(())
}

pub(crate) fn inspect_codex_plugin_at(root: &Path, executable: Option<&str>) -> PluginStatus {
    let binary = executable.map_or_else(codex_binary, OsString::from);
    let output = match Command::new(binary)
        .args(["plugin", "list", "--json"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    {
        Ok(output) if output.status.success() => output,
        Ok(_) => return PluginStatus::Unavailable,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return PluginStatus::CodexMissing,
        Err(_) => return PluginStatus::Unavailable,
    };
    let listing = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
    let plugin = listing
        .as_ref()
        .and_then(|listing| listing.get("installed")?.as_array())
        .and_then(|plugins| {
            plugins.iter().find(|plugin| {
                is_magnesium_codex_plugin(plugin)
                    && plugin.get("installed").and_then(serde_json::Value::as_bool) == Some(true)
            })
        });
    let Some(plugin) = plugin else {
        return PluginStatus::Unavailable;
    };
    let version = plugin
        .get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let protocol_version = plugin
        .get("adapter_protocol_version")
        .or_else(|| plugin.get("protocol_version"))
        .and_then(serde_json::Value::as_u64);
    let hooks_registered = plugin
        .get("hooks_registered")
        .or_else(|| plugin.get("hooks"))
        .and_then(|v| v.as_bool().or_else(|| v.as_array().map(|a| !a.is_empty())));
    let is_trusted = plugin
        .get("trusted")
        .or_else(|| plugin.get("approved"))
        .and_then(serde_json::Value::as_bool);
    if plugin.get("enabled").and_then(serde_json::Value::as_bool) == Some(true) {
        PluginStatus::Registered {
            version,
            protocol_version,
            hooks_registered,
            trusted: is_trusted,
        }
    } else {
        PluginStatus::Disabled { version }
    }
}

pub(crate) fn inspect_codex_plugin(root: &Path) -> PluginStatus {
    inspect_codex_plugin_at(root, None)
}

pub(crate) fn refresh_and_verify_codex_plugin(
    root: &Path,
    marketplace_dir: &Path,
) -> Result<(), String> {
    materialize_bundled_marketplace(marketplace_dir)
        .map_err(|e| format!("could not cache bundled plugin marketplace: {e}"))?;

    match Command::new(codex_binary())
        .arg("--version")
        .current_dir(root)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    {
        Ok(output) if output.status.success() => {}
        Ok(_) => return Err("Codex CLI returned non-zero when checking version".to_owned()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            println!("Codex plugin: Codex CLI not found; skipping plugin refresh.");
            return Ok(());
        }
        Err(error) => return Err(format!("could not execute Codex CLI: {error}")),
    }

    ensure_codex_marketplace_registered(root, marketplace_dir)?;
    ensure_codex_plugin_installed(root)?;
    match inspect_codex_plugin(root) {
        PluginStatus::Registered {
            version,
            protocol_version,
            hooks_registered,
            trusted,
        } => {
            let ver = version.as_deref().unwrap_or("");
            if !crate::adapter::compatible_plugin_version(ver) {
                return Err(format!(
                    "Codex plugin: installed version {ver} is incompatible with expected {}",
                    env!("CARGO_PKG_VERSION")
                ));
            }
            if protocol_version.is_some_and(|v| v != crate::adapter::SUPPORTED_PROTOCOL_VERSION) {
                return Err(format!(
                    "Codex plugin: protocol version {:?} does not match supported {}",
                    protocol_version,
                    crate::adapter::SUPPORTED_PROTOCOL_VERSION
                ));
            }
            if hooks_registered == Some(false) {
                return Err("Codex plugin: hooks are not registered".to_owned());
            }
            if trusted == Some(false) {
                return Err("Codex plugin: plugin is not trusted or approved".to_owned());
            }
            println!("Codex plugin: verified installed version {ver} and bundled plugin files.");
            if protocol_version.is_none() || hooks_registered.is_none() || trusted.is_none() {
                println!(
                    "Codex does not report hook/protocol approval in its plugin listing; verify lifecycle hook approval in Codex before use."
                );
            }
            Ok(())
        }
        PluginStatus::Disabled { version } => {
            println!(
                "Codex plugin: installed version {} but disabled in Codex.",
                version.as_deref().unwrap_or("unknown")
            );
            Err("Codex plugin is installed but disabled in Codex".to_owned())
        }
        PluginStatus::CodexMissing => Ok(()),
        PluginStatus::Unavailable => {
            Err("Codex plugin is not installed or unavailable in Codex".to_owned())
        }
    }
}

#[derive(Debug)]
enum InitError {
    Io {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    Config(ConfigError),
    Invalid(String),
}

impl fmt::Display for InitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                action,
                path,
                source,
            } => write!(formatter, "cannot {action} {}: {source}", path.display()),
            Self::Config(error) => error.fmt(formatter),
            Self::Invalid(message) => formatter.write_str(message),
        }
    }
}

impl Error for InitError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Config(error) => Some(error),
            Self::Invalid(_) => None,
        }
    }
}

impl From<ConfigError> for InitError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PluginStatus {
    Registered {
        version: Option<String>,
        protocol_version: Option<u64>,
        hooks_registered: Option<bool>,
        trusted: Option<bool>,
    },
    Disabled {
        version: Option<String>,
    },
    Unavailable,
    CodexMissing,
}

enum LearnerState {
    Empty,
    Preserved,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AdapterKind {
    AgentsMd,
}

struct InitReport {
    warnings: Vec<String>,
    root: PathBuf,
    created: Vec<PathBuf>,
    modified: Vec<PathBuf>,
    verified: Vec<PathBuf>,
    adapter: Option<AdapterKind>,
    plugin: PluginStatus,
    needs_hook_approval: bool,
    opencode: Option<OpenCodeAdapterOutcome>,
    claude: Option<ClaudePluginStatus>,
    instructions_upgraded_from: Option<String>,
    learner_state: LearnerState,
}

impl InitReport {
    fn new(root: PathBuf) -> Self {
        Self {
            warnings: Vec::new(),
            root,
            created: Vec::new(),
            modified: Vec::new(),
            verified: Vec::new(),
            adapter: None,
            plugin: PluginStatus::Unavailable,
            needs_hook_approval: false,
            opencode: None,
            claude: None,
            instructions_upgraded_from: None,
            learner_state: LearnerState::Empty,
        }
    }

    fn created(&mut self, path: impl AsRef<Path>) {
        push_unique(&mut self.created, path.as_ref());
    }
    fn modified(&mut self, path: impl AsRef<Path>) {
        push_unique(&mut self.modified, path.as_ref());
    }
    fn verified(&mut self, path: impl AsRef<Path>) {
        push_unique(&mut self.verified, path.as_ref());
    }

    fn print(self) {
        for warning in &self.warnings {
            eprintln!("{warning}");
        }
        println!("Magnesium initialized at {}", self.root.display());
        for path in self.created {
            println!("created: {}", path.display());
        }
        for path in self.modified {
            println!("modified: {}", path.display());
        }
        for path in self.verified {
            println!("verified: {}", path.display());
        }
        if let Some(adapter) = self.adapter {
            match adapter {
                AdapterKind::AgentsMd => println!(
                    "adapter: AGENTS.md bootstrap configured (best-effort instruction adapter; requires host lifecycle events or turn attestation for write operations)"
                ),
            }
        }
        match self.plugin {
            PluginStatus::Registered { version, .. } => {
                println!(
                    "plugin: registered and enabled (version {})",
                    version.as_deref().unwrap_or("unknown")
                );
                if self.needs_hook_approval {
                    println!(
                        "host trust: approve Magnesium hooks on next Codex launch if prompted"
                    );
                }
            }
            PluginStatus::Disabled { version } => println!(
                "plugin: installed but disabled (version {}); enable the Magnesium plugin in Codex",
                version.as_deref().unwrap_or("unknown")
            ),
            PluginStatus::Unavailable => println!(
                "plugin: not registered; workspace files were not changed (install the packaged Magnesium plugin globally when available)"
            ),
            PluginStatus::CodexMissing => {
                println!(
                    "plugin: Codex CLI not found; workspace is ready for instruction-only or tool-only adapters"
                )
            }
        }
        if let Some(opencode) = &self.opencode {
            opencode.print();
        }
        if let Some(claude) = &self.claude {
            claude.print();
        }
        if let Some(previous) = &self.instructions_upgraded_from {
            println!(
                "instructions: upgraded to this version's teaching instructions (earlier bundle {previous} kept unchanged)"
            );
        }
        println!("tracking: enabled by default");
        match self.learner_state {
            LearnerState::Empty => println!("learner state: empty"),
            LearnerState::Preserved => println!("learner state: preserved"),
        }
    }

    fn to_json(&self) -> Value {
        let strings = |paths: &[PathBuf]| {
            paths
                .iter()
                .map(|path| Value::String(path.display().to_string()))
                .collect::<Vec<_>>()
        };
        let plugin = match &self.plugin {
            PluginStatus::Registered {
                version,
                protocol_version,
                hooks_registered,
                trusted,
            } => {
                let mut value = json!({"status": "registered"});
                if let Some(version) = version {
                    value["version"] = json!(version);
                }
                if let Some(protocol) = protocol_version {
                    value["protocol_version"] = json!(protocol);
                }
                if let Some(hooks) = hooks_registered {
                    value["hooks_registered"] = json!(hooks);
                }
                if let Some(trusted) = trusted {
                    value["trusted"] = json!(trusted);
                }
                value
            }
            PluginStatus::Disabled { version } => {
                let mut value = json!({"status": "disabled"});
                if let Some(version) = version {
                    value["version"] = json!(version);
                }
                value
            }
            PluginStatus::Unavailable => json!({"status": "unavailable"}),
            PluginStatus::CodexMissing => json!({"status": "codex_missing"}),
        };
        let mut value = json!({
            "root": self.root.display().to_string(),
            "warnings": self.warnings,
            "created": strings(&self.created),
            "modified": strings(&self.modified),
            "verified": strings(&self.verified),
            "adapter": self.adapter.map(|adapter| match adapter {
                AdapterKind::AgentsMd => "agents-md",
            }),
            "plugin": plugin,
            "needs_hook_approval": self.needs_hook_approval,
            "tracking": "enabled_by_default",
            "learner_state": match self.learner_state {
                LearnerState::Empty => "empty",
                LearnerState::Preserved => "preserved",
            },
        });
        if let Some(opencode) = &self.opencode {
            value["opencode"] = match &opencode.adapter {
                OpenCodeAdapterStatus::NotDetected => json!({"adapter": "not_detected"}),
                OpenCodeAdapterStatus::Installed { version } => {
                    json!({"adapter": "installed", "version": version})
                }
                OpenCodeAdapterStatus::Updated { version } => {
                    json!({"adapter": "updated", "version": version})
                }
                OpenCodeAdapterStatus::Rejected { reason } => {
                    json!({"adapter": "rejected", "reason": reason})
                }
            };
            if let Some(mcp) = &opencode.mcp {
                value["opencode"]["mcp"] = match mcp {
                    OpenCodeMcpStatus::Registered => json!("registered"),
                    OpenCodeMcpStatus::Manual { path, entry } => json!({
                        "status": "manual",
                        "path": path.display().to_string(),
                        "entry": entry,
                    }),
                };
            }
        }
        if let Some(claude) = &self.claude {
            value["claude"] = claude.to_json();
        }
        if let Some(previous) = &self.instructions_upgraded_from {
            value["instructions"] = json!({"status": "upgraded", "previous_bundle_hash": previous});
        }
        value
    }
}

fn push_unique(paths: &mut Vec<PathBuf>, path: &Path) {
    if !paths.iter().any(|existing| existing == path) {
        paths.push(path.to_path_buf());
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_CONFIG_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    fn temp_config_dir(label: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "magnesium-plugin-contract-{label}-{}-{}",
            std::process::id(),
            NEXT_CONFIG_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("create temporary OpenCode config directory");
        directory
    }

    fn write_executable(config_dir: &Path, name: &str) -> PathBuf {
        let path = config_dir.join(name);
        fs::write(&path, b"#!/bin/sh\nexit 0\n").expect("write executable fixture");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
                .expect("mark fixture executable");
        }
        path
    }

    #[test]
    fn opencode_plugin_bytes_preserve_template_markers_in_the_executable_path() {
        let plugin = String::from_utf8(super::opencode_plugin_bytes(
            Path::new("/tmp/__MAGNESIUM_VERSION__/__MAGNESIUM_BINARY__/magnesium"),
            "0.2.0",
        ))
        .unwrap();
        assert_eq!(
            plugin
                .lines()
                .find(|line| line.starts_with("const MAGNESIUM_BIN ="))
                .unwrap(),
            r#"const MAGNESIUM_BIN = "/tmp/__MAGNESIUM_VERSION__/__MAGNESIUM_BINARY__/magnesium";"#
        );
        assert_eq!(
            plugin
                .lines()
                .find(|line| line.starts_with("const MAGNESIUM_VERSION ="))
                .unwrap(),
            r#"const MAGNESIUM_VERSION = "0.2.0";"#
        );
    }

    #[test]
    fn codex_plugin_inspection_uses_the_selected_executable_and_query() {
        let directory = temp_config_dir("codex-selected");
        let root = directory.join("workspace");
        fs::create_dir_all(&root).expect("create workspace fixture");
        let record = directory.join("invocation.txt");
        let plugin = serde_json::json!({
            "installed": [{
                "pluginId": "magnesium@magnesium",
                "installed": true,
                "enabled": true,
                "version": env!("CARGO_PKG_VERSION"),
                "adapter_protocol_version": crate::adapter::SUPPORTED_PROTOCOL_VERSION,
                "hooks_registered": true,
                "trusted": true,
            }]
        });
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$PWD\" \"$@\" > '{}'\nprintf '%s' '{}'\n",
            record.display(),
            plugin,
        );
        let selected = write_executable(&directory, "selected-codex");
        fs::write(&selected, script).expect("write selected executable body");
        // A genuinely different second installation reports a disabled
        // plugin; if it were consulted the status would differ.
        let other_record = directory.join("other-invocation.txt");
        let other_plugin = serde_json::json!({
            "installed": [{
                "pluginId": "magnesium@magnesium",
                "installed": true,
                "enabled": false,
                "version": env!("CARGO_PKG_VERSION"),
            }]
        });
        let other = write_executable(&directory, "other-codex");
        fs::write(
            &other,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$PWD\" \"$@\" > '{}'\nprintf '%s' '{}'\n",
                other_record.display(),
                other_plugin,
            ),
        )
        .expect("write other executable body");

        let status =
            super::inspect_codex_plugin_at(&root, Some(selected.to_str().expect("utf8 path")));
        assert!(
            matches!(
                status,
                super::PluginStatus::Registered {
                    hooks_registered: Some(true),
                    trusted: Some(true),
                    ..
                }
            ),
            "the selected executable's report must drive the status: {status:?}"
        );
        assert!(
            !other_record.exists(),
            "the unselected installation must never be invoked"
        );
        let invocation = fs::read_to_string(&record).expect("selected executable was invoked");
        let canonical_root = root.canonicalize().expect("canonical workspace root");
        let mut lines = invocation.lines();
        assert_eq!(
            lines.next(),
            Some(canonical_root.to_string_lossy().as_ref())
        );
        assert_eq!(lines.next(), Some("plugin"));
        assert_eq!(lines.next(), Some("list"));
        assert_eq!(lines.next(), Some("--json"));

        // Selecting the other installation changes the report instead of
        // reusing the first executable's cached identity.
        let other_status =
            super::inspect_codex_plugin_at(&root, Some(other.to_str().expect("utf8 path")));
        assert_eq!(
            other_status,
            super::PluginStatus::Disabled {
                version: Some(env!("CARGO_PKG_VERSION").to_owned())
            }
        );
        assert!(other_record.exists(), "the selected other installation ran");

        // An explicitly selected path that cannot be executed reports the
        // missing provider instead of silently consulting another install.
        assert_eq!(
            super::inspect_codex_plugin_at(&root, Some("/nonexistent/selected-codex")),
            super::PluginStatus::CodexMissing
        );
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn claude_mcp_rule_names_the_bundled_plugin_and_its_server() {
        let bundled = |name: &str| -> serde_json::Value {
            let (_, content) = super::CLAUDE_PLUGIN_FILES
                .iter()
                .find(|(path, _)| *path == name)
                .expect("bundled Claude plugin file");
            serde_json::from_str(content).expect("bundled Claude plugin JSON")
        };
        let plugin = bundled(".claude-plugin/plugin.json");
        let mcp = bundled(".mcp.json");
        let servers = mcp["mcpServers"].as_object().expect("MCP servers");
        assert_eq!(servers.len(), 1, "{mcp}");
        let server = servers.keys().next().unwrap();
        let plugin = plugin["name"].as_str().expect("plugin name");
        assert_eq!(
            super::CLAUDE_MCP_RULE,
            format!("mcp__plugin_{plugin}_{server}__*")
        );
    }

    #[test]
    fn claude_read_rule_names_the_plugin_directory_from_the_filesystem_root() {
        #[cfg(unix)]
        let (directory, rule) = (
            "/Users/a b/Library/Application Support/magnesium/claude-plugin",
            "Read(//Users/a b/Library/Application Support/magnesium/claude-plugin/**)",
        );
        #[cfg(windows)]
        let (directory, rule) = (
            r"C:\Users\a b\AppData\Roaming\magnesium\claude-plugin",
            "Read(//c/Users/a b/AppData/Roaming/magnesium/claude-plugin/**)",
        );
        let rules = super::claude_permission_rules(Path::new(directory));
        assert!(rules.contains(&rule.to_owned()), "{rules:?}");
    }
}
