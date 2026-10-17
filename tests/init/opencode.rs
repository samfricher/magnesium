use std::fs;
use std::path::Path;

use serde_json::Value;
use support::{TestEnvironment, test_binary};

use super::assert_success;

fn expected_opencode_plugin(opencode_binary: &Path, version: &str) -> Vec<u8> {
    let template = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("opencode-plugin/magnesium.js"),
    )
    .expect("read bundled OpenCode plugin");
    let escaped = opencode_binary
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    template
        .replace("__MAGNESIUM_VERSION__", version)
        .replacen("__MAGNESIUM_BINARY__", &escaped, 1)
        .into_bytes()
}

fn opencode_mcp_command(opencode_binary: &Path) -> Value {
    serde_json::json!([
        opencode_binary.display().to_string(),
        "_internal",
        "mcp-server",
        "--plugin-version",
        env!("CARGO_PKG_VERSION"),
        "--adapter-protocol-version",
        "2",
        "--host-kind",
        "opencode"
    ])
}

#[test]
fn init_installs_and_refreshes_the_opencode_adapter_and_mcp_registration() {
    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    let opencode_binary = test_binary();
    let config_dir = environment.opencode_config_dir();
    let plugin_path = config_dir.join("plugins/magnesium.js");
    let skill_path = config_dir.join("skills/learn/SKILL.md");
    let config_path = config_dir.join("opencode.json");

    let first = environment
        .command()
        .env("OPENCODE_TEST_BINARY", &opencode_binary)
        .arg("init")
        .output()
        .expect("run magnesium init with OpenCode detected");
    assert_success(&first);
    let stdout = String::from_utf8(first.stdout).expect("init output is UTF-8");
    assert!(stdout.contains("OpenCode adapter: installed (version "));
    assert!(stdout.contains("OpenCode MCP: registered"));
    assert_eq!(
        fs::read(&plugin_path).expect("installed plugin"),
        expected_opencode_plugin(&opencode_binary, env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(
        fs::read(&skill_path).expect("installed skill"),
        fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("opencode-plugin/skills/learn/SKILL.md")
        )
        .expect("read bundled OpenCode skill")
    );
    let installed_plugin = fs::read(&plugin_path).unwrap();
    let installed_skill = fs::read(&skill_path).unwrap();

    let config: Value = serde_json::from_slice(&fs::read(&config_path).expect("created config"))
        .expect("OpenCode config is strict JSON");
    assert_eq!(config["$schema"], "https://opencode.ai/config.json");
    assert_eq!(config["mcp"]["magnesium"]["type"], "local");
    assert_eq!(config["mcp"]["magnesium"]["enabled"], true);
    assert_eq!(
        config["mcp"]["magnesium"]["command"],
        opencode_mcp_command(&opencode_binary)
    );
    assert!(config["mcp"]["magnesium"].get("cwd").is_none());
    let installed_config = fs::read(&config_path).unwrap();

    let second = environment
        .command()
        .env("OPENCODE_TEST_BINARY", &opencode_binary)
        .arg("init")
        .output()
        .expect("rerun magnesium init");
    assert_success(&second);
    let second_stdout = String::from_utf8(second.stdout).unwrap();
    assert!(second_stdout.contains("OpenCode adapter: installed (version "));
    assert_eq!(fs::read(&plugin_path).unwrap(), installed_plugin);
    assert_eq!(fs::read(&skill_path).unwrap(), installed_skill);
    assert_eq!(fs::read(&config_path).unwrap(), installed_config);

    fs::write(&plugin_path, b"stale plugin\n").unwrap();
    fs::write(&skill_path, b"stale skill\n").unwrap();
    let third = environment
        .command()
        .env("OPENCODE_TEST_BINARY", &opencode_binary)
        .arg("init")
        .output()
        .expect("refresh magnesium init");
    assert_success(&third);
    assert!(
        String::from_utf8(third.stdout)
            .unwrap()
            .contains("OpenCode adapter: updated (version ")
    );
    assert_eq!(
        fs::read(&plugin_path).unwrap(),
        expected_opencode_plugin(&opencode_binary, env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(fs::read(&skill_path).unwrap(), installed_skill);
}

#[test]
fn init_preserves_unrelated_existing_opencode_config_keys() {
    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    let opencode_binary = test_binary();
    let config_dir = environment.opencode_config_dir();
    fs::create_dir_all(&config_dir).unwrap();
    let config_path = config_dir.join("opencode.json");
    let original = serde_json::json!({
        "$schema": "https://opencode.ai/config.json",
        "theme": "system",
        "mcp": {
            "other": {"type": "local", "command": ["other-server"], "enabled": true}
        },
        "permission": {"bash": "ask"}
    });
    let mut original_bytes = serde_json::to_vec_pretty(&original).unwrap();
    original_bytes.push(b'\n');
    fs::write(&config_path, &original_bytes).unwrap();

    let output = environment
        .command()
        .env("OPENCODE_TEST_BINARY", &opencode_binary)
        .arg("init")
        .output()
        .expect("run magnesium init with an existing config");
    assert_success(&output);
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("OpenCode MCP: registered")
    );

    let updated: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    assert_eq!(updated["theme"], "system");
    assert_eq!(updated["permission"]["bash"], "ask");
    assert_eq!(updated["mcp"]["other"], original["mcp"]["other"]);
    assert_eq!(
        updated["mcp"]["magnesium"]["command"],
        opencode_mcp_command(&opencode_binary)
    );
}

#[test]
fn init_leaves_opencode_jsonc_untouched_and_reports_manual_registration() {
    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    let opencode_binary = test_binary();
    let config_dir = environment.opencode_config_dir();
    fs::create_dir_all(&config_dir).unwrap();
    let jsonc_path = config_dir.join("opencode.jsonc");
    let jsonc =
        b"{\n  // keep the comment\n  \"$schema\": \"https://opencode.ai/config.json\"\n}\n";
    fs::write(&jsonc_path, jsonc).unwrap();

    let output = environment
        .command()
        .env("OPENCODE_TEST_BINARY", &opencode_binary)
        .arg("init")
        .output()
        .expect("run magnesium init with a JSONC config");
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("OpenCode adapter: installed (version "));
    assert!(stdout.contains("OpenCode MCP: manual registration required"));
    assert!(stdout.contains(&jsonc_path.display().to_string()));
    assert_eq!(fs::read(&jsonc_path).unwrap(), jsonc);
    assert!(!config_dir.join("opencode.json").exists());
}

#[test]
fn init_leaves_unparseable_opencode_json_untouched_and_reports_manual_registration() {
    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    let opencode_binary = test_binary();
    let config_dir = environment.opencode_config_dir();
    fs::create_dir_all(&config_dir).unwrap();
    let config_path = config_dir.join("opencode.json");
    let invalid = b"{\n  // comments are not strict JSON\n  \"mcp\": {}\n}\n";
    fs::write(&config_path, invalid).unwrap();

    let output = environment
        .command()
        .env("OPENCODE_TEST_BINARY", &opencode_binary)
        .arg("init")
        .output()
        .expect("run magnesium init with an unparseable config");
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("OpenCode MCP: manual registration required"));
    assert!(stdout.contains(&config_path.display().to_string()));
    assert_eq!(fs::read(&config_path).unwrap(), invalid);
}

#[cfg(unix)]
#[test]
fn init_never_follows_symlinked_opencode_plugin_paths() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    let opencode_binary = test_binary();
    let config_dir = environment.opencode_config_dir();
    fs::create_dir_all(&config_dir).unwrap();
    let external = environment.root.join("external-opencode-plugins");
    fs::create_dir(&external).unwrap();
    fs::set_permissions(&external, fs::Permissions::from_mode(0o755)).unwrap();
    let sentinel = external.join("sentinel.bin");
    fs::write(&sentinel, b"external plugin bytes").unwrap();
    symlink(&external, config_dir.join("plugins")).unwrap();

    let output = environment
        .command()
        .env("OPENCODE_TEST_BINARY", &opencode_binary)
        .arg("init")
        .output()
        .expect("run magnesium init with a symlinked plugins directory");
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("OpenCode adapter: not installed;"));
    assert!(stdout.contains("symbolic link"));
    assert_eq!(fs::read(&sentinel).unwrap(), b"external plugin bytes");
    assert_eq!(fs::read_dir(&external).unwrap().count(), 1);

    fs::remove_file(config_dir.join("plugins")).unwrap();
    fs::create_dir(config_dir.join("plugins")).unwrap();
    symlink(&sentinel, config_dir.join("plugins/magnesium.js")).unwrap();
    let linked_file = environment
        .command()
        .env("OPENCODE_TEST_BINARY", &opencode_binary)
        .arg("init")
        .output()
        .expect("run magnesium init with a symlinked plugin file");
    assert_success(&linked_file);
    let linked_stdout = String::from_utf8(linked_file.stdout).unwrap();
    assert!(linked_stdout.contains("OpenCode adapter: not installed;"));
    assert!(linked_stdout.contains("symbolic link"));
    assert_eq!(fs::read(&sentinel).unwrap(), b"external plugin bytes");
    assert!(
        fs::symlink_metadata(config_dir.join("plugins/magnesium.js"))
            .unwrap()
            .file_type()
            .is_symlink()
    );

    fs::remove_file(config_dir.join("plugins/magnesium.js")).unwrap();
    fs::create_dir_all(config_dir.join("plugins/magnesium.js")).unwrap();
    let second = environment
        .command()
        .env("OPENCODE_TEST_BINARY", &opencode_binary)
        .arg("init")
        .output()
        .expect("run magnesium init with a non-regular plugin path");
    assert_success(&second);
    let second_stdout = String::from_utf8(second.stdout).unwrap();
    assert!(second_stdout.contains("OpenCode adapter: not installed;"));
    assert!(second_stdout.contains("not a regular file"));
    assert!(config_dir.join("plugins/magnesium.js").is_dir());

    // A symlinked skills grandparent must not redirect the skill install.
    fs::remove_dir_all(config_dir.join("plugins")).unwrap();
    if config_dir.join("skills").exists() {
        fs::remove_dir_all(config_dir.join("skills")).unwrap();
    }
    let external_skills = environment.root.join("external-opencode-skills");
    fs::create_dir_all(&external_skills).unwrap();
    let skill_sentinel = external_skills.join("sentinel.bin");
    fs::write(&skill_sentinel, b"external skill bytes").unwrap();
    symlink(&external_skills, config_dir.join("skills")).unwrap();
    let third = environment
        .command()
        .env("OPENCODE_TEST_BINARY", &opencode_binary)
        .arg("init")
        .output()
        .expect("run magnesium init with a symlinked skills directory");
    assert_success(&third);
    let third_stdout = String::from_utf8(third.stdout).unwrap();
    assert!(third_stdout.contains("OpenCode adapter: not installed;"));
    assert!(third_stdout.contains("symbolic link"));
    assert_eq!(fs::read(&skill_sentinel).unwrap(), b"external skill bytes");
    assert_eq!(fs::read_dir(&external_skills).unwrap().count(), 1);
}

#[test]
fn init_ignores_an_opencode_binary_that_fails_its_version_probe() {
    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    let failing = environment.root.join("opencode-failing");
    fs::write(&failing, "#!/bin/sh\nexit 3\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&failing, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let output = environment
        .command()
        .env("OPENCODE_TEST_BINARY", &failing)
        .arg("init")
        .output()
        .expect("run magnesium init with a failing OpenCode probe");
    assert_success(&output);
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("OpenCode adapter: not detected")
    );
    assert!(!environment.opencode_config_dir().exists());
}

#[test]
fn init_reports_opencode_not_detected_and_leaves_config_absent() {
    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    let output = environment
        .command()
        .arg("init")
        .output()
        .expect("run magnesium init without OpenCode");
    assert_success(&output);
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("OpenCode adapter: not detected")
    );
    assert!(!environment.opencode_config_dir().exists());
}

#[test]
fn update_refreshes_installed_opencode_adapter_without_touching_mcp() {
    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    let opencode_binary = test_binary();
    let config_dir = environment.opencode_config_dir();
    let plugin_path = config_dir.join("plugins/magnesium.js");
    let config_path = config_dir.join("opencode.json");

    let init = environment
        .command()
        .env("OPENCODE_TEST_BINARY", &opencode_binary)
        .arg("init")
        .output()
        .expect("initialize with OpenCode detected");
    assert_success(&init);
    fs::write(&plugin_path, b"stale plugin\n").unwrap();
    let config_before = fs::read(&config_path).unwrap();

    let update = environment
        .command()
        .env("OPENCODE_TEST_BINARY", &opencode_binary)
        .arg("update")
        .output()
        .expect("run update");
    assert_success(&update);
    let stdout = String::from_utf8(update.stdout).expect("update output is UTF-8");
    assert!(stdout.contains("OpenCode adapter: updated (version "));
    assert_eq!(
        fs::read(&plugin_path).unwrap(),
        expected_opencode_plugin(&opencode_binary, env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(fs::read(&config_path).unwrap(), config_before);
}

/// OpenCode can be started from a Claude Code terminal, which exports
/// CLAUDE_CODE_SESSION_ID to every child. The MCP server that init registers
/// for OpenCode must still bind tool calls to the OpenCode conversation,
/// like the OpenCode hooks do.
#[test]
fn registered_opencode_mcp_server_binds_as_opencode_inside_a_claude_terminal() {
    use std::io::BufReader;
    use std::process::Stdio;
    use support::mcp::{McpClient, write_message};

    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    let opencode_binary = test_binary();
    let init = environment
        .command()
        .env("OPENCODE_TEST_BINARY", &opencode_binary)
        .arg("init")
        .output()
        .expect("initialize with OpenCode detected");
    assert_success(&init);
    let config: Value = serde_json::from_slice(
        &fs::read(environment.opencode_config_dir().join("opencode.json")).unwrap(),
    )
    .unwrap();
    let command: Vec<String> = config["mcp"]["magnesium"]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|part| part.as_str().unwrap().to_owned())
        .collect();

    let mut child = environment
        .command_with(&command[0], &environment.workspace)
        .args(&command[1..])
        .env("CLAUDE_CODE_SESSION_ID", "claude-terminal-session")
        .env("OPENCODE_SESSION_ID", "opencode-session")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start the registered OpenCode MCP command");
    let stdin = child.stdin.take().unwrap();
    let stdout = BufReader::new(child.stdout.take().unwrap());
    let mut client = McpClient {
        child,
        stdin,
        stdout,
    };
    client.request(
        1,
        "initialize",
        serde_json::json!({
            "protocolVersion":"2025-11-25", "capabilities":{},
            "clientInfo":{"name":"opencode","version":"test"}
        }),
    );
    write_message(
        &mut client.stdin,
        &serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    );
    let status = client.call_value(
        2,
        "context",
        serde_json::json!({"schema":"learner-model/v2","action":"status"}),
    );
    assert_eq!(status["host_binding"]["host_kind"], "opencode", "{status}");
    assert!(
        status["host_binding"]["host_thread_id"]
            .as_str()
            .is_some_and(|thread| thread.contains("opencode-session")),
        "{status}"
    );
    client.close_cleanly();
}
