use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::time::SystemTime;

use rusqlite::Connection;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use support::TestEnvironment;
use support::mcp::McpClient;
use support::snapshot::DatabaseSnapshot;

const CURRENT_TABLES: &[&str] = &[
    "learning_sessions",
    "host_bindings",
    "learning_events",
    "observations",
    "learner_memories",
    "memory_changes",
    "operation_receipts",
    "review_items",
    "context_receipts",
];

fn initialize(environment: &TestEnvironment) {
    let output = environment
        .command()
        .arg("init")
        .output()
        .expect("initialize workspace");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn cli_instruction_request(environment: &TestEnvironment, request: &Value) -> Value {
    cli_instruction_request_in(environment, &environment.workspace, request)
}

fn cli_instruction_request_in(environment: &TestEnvironment, cwd: &Path, request: &Value) -> Value {
    let output = cli_instruction_output(environment, cwd, request);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("CLI instruction response JSON")
}

fn cli_instruction_output(
    environment: &TestEnvironment,
    cwd: &Path,
    request: &Value,
) -> std::process::Output {
    let mut child = environment
        .command_in(cwd)
        .args([
            "adapter",
            "call",
            "context",
            "--protocol-version",
            "2",
            "--json-input",
            "-",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("start CLI adapter call");
    serde_json::to_writer(child.stdin.as_mut().unwrap(), request).unwrap();
    child.stdin.as_mut().unwrap().flush().unwrap();
    drop(child.stdin.take());
    child.wait_with_output().expect("finish CLI adapter call")
}

fn mcp_instruction_request(
    environment: &TestEnvironment,
    cwd: &Path,
    bound_workspace: Option<&Path>,
    initialized: bool,
    request: Value,
) -> Value {
    let mut mcp = McpClient::spawn_bound(environment, cwd, bound_workspace, initialized);
    mcp.initialize();
    let response = mcp.request(
        2,
        "tools/call",
        json!({"name":"context","arguments":request}),
    );
    mcp.close_cleanly();
    response
}

fn event_count(database: &std::path::Path) -> i64 {
    let connection =
        Connection::open_with_flags(database, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("open learner store read-only");
    connection
        .query_row("SELECT COUNT(*) FROM learning_events", [], |row| row.get(0))
        .expect("count learner events")
}

fn workspace_file_snapshot(root: &Path) -> BTreeMap<String, (bool, u64, SystemTime, u32, String)> {
    fn visit(
        root: &Path,
        path: &Path,
        output: &mut BTreeMap<String, (bool, u64, SystemTime, u32, String)>,
    ) {
        let metadata = std::fs::symlink_metadata(path).expect("snapshot metadata");
        assert!(
            !metadata.file_type().is_symlink(),
            "workspace snapshot has no symlinks"
        );
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode()
        };
        #[cfg(not(unix))]
        let mode = u32::from(metadata.permissions().readonly());
        let relative = path
            .strip_prefix(root)
            .expect("snapshot path is below root");
        let key = if relative.as_os_str().is_empty() {
            ".".to_owned()
        } else {
            relative.to_string_lossy().replace('\\', "/")
        };
        let is_directory = metadata.is_dir();
        let digest = if is_directory {
            String::new()
        } else {
            Sha256::digest(std::fs::read(path).expect("snapshot file"))
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect()
        };
        output.insert(
            key,
            (
                is_directory,
                metadata.len(),
                metadata.modified().expect("modification time"),
                mode,
                digest,
            ),
        );
        if is_directory {
            for entry in std::fs::read_dir(path).expect("snapshot directory") {
                visit(root, &entry.expect("snapshot entry").path(), output);
            }
        }
    }

    let mut output = BTreeMap::new();
    visit(root, root, &mut output);
    output
}

fn assert_same_filesystem(
    before: &BTreeMap<String, (bool, u64, SystemTime, u32, String)>,
    after: &BTreeMap<String, (bool, u64, SystemTime, u32, String)>,
    label: &str,
) {
    let changed = before
        .keys()
        .chain(after.keys())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter(|path| before.get(*path) != after.get(*path))
        .map(|path| path.as_str())
        .collect::<Vec<_>>();
    let details = changed
        .iter()
        .map(|path| (*path, before.get(*path), after.get(*path)))
        .collect::<Vec<_>>();
    assert!(
        changed.is_empty(),
        "{label} changed Magnesium files: {details:?}"
    );
}

#[test]
fn fresh_initialization_installs_instructions_and_preserves_study_materials() {
    let environment = TestEnvironment::from_fixture("basic").expect("test environment");
    let materials = environment.workspace.join("materials");
    std::fs::create_dir(&materials).unwrap();
    std::fs::write(materials.join("notes.md"), "Personal study notes: λ ≠ 0\n").unwrap();
    std::fs::write(materials.join("worksheet.bin"), [0, 255, 1, 128, 0]).unwrap();
    let before = workspace_file_snapshot(&materials);
    let lesson_before = std::fs::read(environment.workspace.join("lesson.md")).unwrap();
    initialize(&environment);
    let response = cli_instruction_request(
        &environment,
        &json!({"action":"instructions","stage":"orient_and_probe","host":"codex"}),
    );
    assert_eq!(response["status"], "served");
    assert!(
        response["files"]
            .as_array()
            .unwrap()
            .contains(&json!("workflows/orient-and-probe.md"))
    );
    assert!(response.get("stage_metadata").is_none());
    assert!(
        environment
            .workspace
            .join(support::CONTROL_DIR)
            .join("state.sqlite")
            .is_file()
    );
    assert_same_filesystem(
        &before,
        &workspace_file_snapshot(&materials),
        "fresh initialization",
    );
    assert_eq!(
        std::fs::read(environment.workspace.join("lesson.md")).unwrap(),
        lesson_before
    );
}

#[test]
fn cli_and_mcp_serve_the_same_approved_bundle_without_mutation() {
    let environment = TestEnvironment::from_fixture("basic").expect("test environment");
    initialize(&environment);
    let database = environment
        .workspace
        .join(support::CONTROL_DIR)
        .join("state.sqlite");
    let events_before = event_count(&database);
    let state_before =
        DatabaseSnapshot::capture(&database, CURRENT_TABLES).expect("snapshot learner records");
    let filesystem_before =
        workspace_file_snapshot(&environment.workspace.join(support::CONTROL_DIR));
    let bridge = std::fs::read(
        environment
            .workspace
            .join(support::CONTROL_DIR)
            .join("INSTRUCTIONS.md"),
    )
    .expect("active v2 bridge");
    let installed_manifest: Value = serde_json::from_slice(
        &std::fs::read(
            environment
                .workspace
                .join(support::CONTROL_DIR)
                .join("instructions/v2")
                .join(installed_bundle_directory(&environment))
                .join("manifest.json"),
        )
        .expect("installed v2 manifest"),
    )
    .expect("installed manifest JSON");
    assert_eq!(installed_manifest["status"], "active");
    assert!(
        environment
            .workspace
            .join(support::CONTROL_DIR)
            .join("instructions/active.json")
            .exists()
    );
    let request = json!({
        "action": "instructions",
        "stage": "teach_and_practice",
        "host": "codex"
    });

    let cli = cli_instruction_request(&environment, &request);
    assert_same_filesystem(
        &filesystem_before,
        &workspace_file_snapshot(&environment.workspace.join(support::CONTROL_DIR)),
        "CLI instruction read",
    );
    assert_eq!(event_count(&database), events_before);
    assert_eq!(
        DatabaseSnapshot::capture(&database, CURRENT_TABLES,)
            .expect("snapshot learner records after CLI read"),
        state_before
    );
    let mut mcp = McpClient::spawn(&environment, true);
    mcp.initialize();
    // A request after notifications/initialized serializes after that
    // handshake notification on the server transport.
    mcp.request(2, "tools/list", json!({}));
    // MCP startup initializes its host and may update workspace metadata. Take
    // the action-level baseline only after startup so this proves that the
    // context request itself is read-only.
    let mcp_events_before = event_count(&database);
    let mcp_state_before = DatabaseSnapshot::capture(&database, CURRENT_TABLES)
        .expect("snapshot learner records after MCP startup");
    let mcp_filesystem_before =
        workspace_file_snapshot(&environment.workspace.join(support::CONTROL_DIR));
    let tool = mcp.call_value(3, "context", request);
    assert_same_filesystem(
        &mcp_filesystem_before,
        &workspace_file_snapshot(&environment.workspace.join(support::CONTROL_DIR)),
        "MCP instruction read",
    );
    assert_eq!(event_count(&database), mcp_events_before);
    assert_eq!(
        DatabaseSnapshot::capture(&database, CURRENT_TABLES,)
            .expect("snapshot learner records after MCP instruction read"),
        mcp_state_before
    );
    mcp.close_cleanly();

    assert_eq!(
        cli, tool,
        "CLI and MCP must return the same semantic response"
    );
    assert_eq!(cli["status"], "served");
    assert_eq!(cli["recommended_stage"], "teach_and_practice");
    assert!(cli.get("stage_metadata").is_none());
    assert!(
        cli["text"]
            .as_str()
            .is_some_and(|text| text.contains("# Magnesium")),
        "approved v2 text must be served"
    );
    assert_eq!(event_count(&database), events_before);
    assert_eq!(
        std::fs::read(
            environment
                .workspace
                .join(support::CONTROL_DIR)
                .join("INSTRUCTIONS.md")
        )
        .expect("active v2 bridge after"),
        bridge
    );
    assert_eq!(
        DatabaseSnapshot::capture(&database, CURRENT_TABLES,)
            .expect("snapshot learner records after"),
        state_before
    );
}

fn installed_bundle_directory(environment: &TestEnvironment) -> String {
    let root = environment
        .workspace
        .join(support::CONTROL_DIR)
        .join("instructions/v2");
    std::fs::read_dir(root)
        .expect("v2 asset directory")
        .map(|entry| entry.expect("v2 asset entry"))
        .find(|entry| entry.file_type().expect("v2 asset kind").is_dir())
        .expect("installed version directory")
        .file_name()
        .to_string_lossy()
        .into_owned()
}

#[test]
fn instruction_action_requires_bounded_stage_and_host_and_rejects_other_context_fields() {
    let environment = TestEnvironment::from_fixture("basic").expect("test environment");
    initialize(&environment);
    for request in [
        json!({"action":"instructions","host":"codex"}),
        json!({"action":"instructions","stage":"teach_and_practice"}),
        json!({"action":"instructions","stage":"anything","host":"codex"}),
        json!({"action":"instructions","stage":"teach_and_practice","host":"anything"}),
        json!({"action":"instructions","stage":"teach_and_practice","host":"codex","operation":"anything"}),
        json!({"action":"instructions","stage":"teach_and_practice","host":"codex","goal":"unexpected instruction field"}),
        json!({"action":"status","stage":"teach_and_practice","host":"codex"}),
    ] {
        let mut mcp = McpClient::spawn(&environment, true);
        mcp.initialize();
        let response = mcp.request(
            2,
            "tools/call",
            json!({"name":"context","arguments":request}),
        );
        assert!(
            response.get("error").is_none(),
            "MCP request stays a typed tool reply: {response}"
        );
        assert_eq!(response["result"]["isError"], true);
        mcp.close_cleanly();
    }
}

#[test]
fn instruction_action_does_not_install_outside_an_initialized_workspace() {
    let environment = TestEnvironment::from_fixture("basic").expect("test environment");
    let control = environment.workspace.join(support::CONTROL_DIR);
    assert!(!control.exists());
    let response = cli_instruction_request(
        &environment,
        &json!({
            "action": "instructions",
            "stage": "orient_and_probe",
            "host": "claude"
        }),
    );
    assert_eq!(response["error"]["code"], "inactive_workspace");
    assert!(!control.exists());
}

#[test]
fn read_only_authorization_uses_local_boundaries_without_global_routing() {
    let request = json!({
        "action": "instructions",
        "stage": "orient_and_probe",
        "host": "codex"
    });

    let environment = TestEnvironment::from_fixture("basic").expect("test environment");
    let inactive_cli = cli_instruction_request(&environment, &request);
    let inactive_mcp = mcp_instruction_request(
        &environment,
        &environment.workspace,
        None,
        false,
        request.clone(),
    );
    assert_eq!(inactive_cli["error"]["code"], "inactive_workspace");
    assert!(inactive_cli.get("stage_metadata").is_none());
    assert_eq!(
        inactive_mcp["result"]["content"][0]["text"],
        serde_json::to_string(&inactive_cli).unwrap()
    );
    assert!(!environment.workspace.join(support::CONTROL_DIR).exists());

    let environment = TestEnvironment::from_fixture("basic").expect("test environment");
    initialize(&environment);
    let foreign = environment.root.join("foreign-workspace");
    let foreign_control = foreign.join(support::CONTROL_DIR);
    std::fs::create_dir_all(&foreign_control).expect("create foreign control");
    std::fs::copy(
        environment
            .workspace
            .join(support::CONTROL_DIR)
            .join("config.toml"),
        foreign_control.join("config.toml"),
    )
    .expect("copy foreign workspace identity");
    let foreign_before = workspace_file_snapshot(&foreign_control);
    let primary_before = workspace_file_snapshot(&environment.workspace.join(support::CONTROL_DIR));
    let foreign_cli = cli_instruction_request_in(&environment, &foreign, &request);
    assert_ne!(
        foreign_cli["status"], "served",
        "copied config is not an installed instruction bundle"
    );
    assert!(foreign_cli.get("stage_metadata").is_none());
    assert_same_filesystem(
        &foreign_before,
        &workspace_file_snapshot(&foreign_control),
        "foreign workspace instruction request",
    );
    assert_same_filesystem(
        &primary_before,
        &workspace_file_snapshot(&environment.workspace.join(support::CONTROL_DIR)),
        "primary workspace after foreign instruction request",
    );

    let nested = environment.workspace.join("nested");
    let nested_control = nested.join(support::CONTROL_DIR);
    std::fs::create_dir_all(&nested_control).expect("create nested boundary");
    std::fs::copy(
        environment
            .workspace
            .join(support::CONTROL_DIR)
            .join("config.toml"),
        nested_control.join("config.toml"),
    )
    .expect("copy parent identity into nested boundary");
    let nested_before = workspace_file_snapshot(&nested_control);
    let nested_cli = cli_instruction_request_in(&environment, &nested, &request);
    assert_ne!(
        nested_cli["status"], "served",
        "copied config is not an installed instruction bundle"
    );
    assert!(nested_cli.get("stage_metadata").is_none());
    assert_same_filesystem(
        &nested_before,
        &workspace_file_snapshot(&nested_control),
        "nearer nested instruction boundary",
    );

    let bound_mcp = mcp_instruction_request(
        &environment,
        &foreign,
        Some(&environment.workspace),
        true,
        request.clone(),
    );
    assert_eq!(bound_mcp["result"]["isError"], true);
    let bound_error: Value = serde_json::from_str(
        bound_mcp["result"]["content"][0]["text"]
            .as_str()
            .expect("MCP routing error text"),
    )
    .expect("MCP routing error JSON");
    assert!(
        bound_error.get("reason").is_some() || bound_error.get("code").is_some(),
        "{bound_error}"
    );
}

#[cfg(unix)]
#[test]
fn instruction_authorization_rejects_symlinked_control_for_cli_and_mcp() {
    use std::os::unix::fs::symlink;

    let environment = TestEnvironment::from_fixture("basic").expect("test environment");
    initialize(&environment);
    let control = environment.workspace.join(support::CONTROL_DIR);
    let external_control = environment.root.join("external-control");
    std::fs::rename(&control, &external_control).expect("move control outside workspace");
    symlink(&external_control, &control).expect("link control outside workspace");
    let external_before = workspace_file_snapshot(&external_control);
    let request = json!({
        "action": "instructions",
        "stage": "orient_and_probe",
        "host": "codex"
    });

    let cli = cli_instruction_output(&environment, &environment.workspace, &request);
    assert!(!cli.status.success());
    assert!(String::from_utf8_lossy(&cli.stderr).contains("real directory"));
    let mcp = mcp_instruction_request(&environment, &environment.workspace, None, true, request);
    assert_eq!(mcp["result"]["isError"], true);
    assert!(
        mcp["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("real directory")
    );
    assert!(
        std::fs::symlink_metadata(&control)
            .expect("symlink remains")
            .file_type()
            .is_symlink()
    );
    assert_same_filesystem(
        &external_before,
        &workspace_file_snapshot(&external_control),
        "external control target after symlinked instruction request",
    );
}
