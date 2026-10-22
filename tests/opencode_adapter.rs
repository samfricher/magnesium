//! OpenCode adapter boundary tests (LEARN-213).
//!
//! - The deterministic Node harness (`opencode-plugin/tests.mjs`) pins the
//!   plugin's capture/skip decisions and non-blocking hook execution. It runs
//!   on Unix because its executable hook stub uses /bin/sh. Node is required
//!   in that lane; a missing Node installation fails explicitly.
//! - The native checks cover the session-scoped post-compaction contract the
//!   plugin emits: no turn identity, accepted without capture, and the next
//!   prompt's hook context still tells the tutor to reread context.prepare.

#[cfg(unix)]
use std::path::Path;
#[cfg(unix)]
use std::process::Command;

#[cfg(unix)]
use rusqlite::Connection;
use serde_json::{Value, json};
use support::{TestEnvironment, run_hook};

fn init_workspace(environment: &TestEnvironment) {
    let init = environment
        .command()
        .arg("init")
        .output()
        .expect("initialize workspace");
    assert!(
        init.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );
}

#[cfg(unix)]
fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

#[cfg(unix)]
#[test]
fn opencode_adapter_plugin_behaviors() {
    assert!(
        node_available(),
        "Node is required for the OpenCode plugin integration test"
    );
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("node")
        .arg(root.join("opencode-plugin/tests.mjs"))
        .current_dir(root)
        .output()
        .expect("run OpenCode adapter plugin harness");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "OpenCode adapter plugin harness failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("ALL PLUGIN CASES PASSED"),
        "plugin harness did not report success:\n{stdout}"
    );
}

#[cfg(unix)]
#[test]
fn installed_opencode_plugin_runs_its_bound_runtime_without_path_lookup() {
    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    let bound_directory = environment.root.join("__MAGNESIUM_VERSION__");
    std::fs::create_dir(&bound_directory).expect("create executable fixture directory");
    let bound_binary = bound_directory.join("magnesium");
    std::fs::copy(support::test_binary(), &bound_binary)
        .expect("copy ordinary Magnesium executable");
    let install = environment
        .command_with(&bound_binary, &environment.workspace)
        .env("OPENCODE_TEST_BINARY", &bound_binary)
        .arg("init")
        .output()
        .expect("install OpenCode plugin");
    assert!(
        install.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&install.stderr)
    );
    let script = r#"
import { readFileSync } from "node:fs";
if (process.env.MAGNESIUM_BIN !== undefined) throw new Error("unexpected runtime override");
process.env.PATH = "";
const source = readFileSync(process.argv[1]);
const { Magnesium } = await import("data:text/javascript;base64," + source.toString("base64"));
const warnings = [];
const hooks = await Magnesium({
    directory: process.cwd(),
    client: {
        app: { log: async (entry) => warnings.push(entry) },
        session: { get: async () => ({ data: { id: "installed-plugin-session" } }) },
    },
});
const turn = {
    message: { id: "installed-plugin-turn" },
    parts: [{
        id: "installed-original-part",
        sessionID: "installed-plugin-session",
        messageID: "installed-plugin-turn",
        type: "text",
        text: "What did Magnesium retain for this workspace?",
    }],
};
await hooks["chat.message"]({ sessionID: "installed-plugin-session" }, turn);
console.log(JSON.stringify({ parts: turn.parts, warnings }));
"#;
    let output = environment
        .command_with("node", &environment.workspace)
        .args(["--input-type=module", "--eval", script])
        .arg(
            environment
                .opencode_config_dir()
                .join("plugins/magnesium.js"),
        )
        .output()
        .expect("run installed OpenCode plugin with Node");
    assert!(
        output.status.success(),
        "installed plugin failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).expect("installed plugin result");
    assert_eq!(result["warnings"], json!([]));
    let parts = result["parts"].as_array().expect("plugin message parts");
    assert_eq!(parts.len(), 2);
    assert_eq!(
        parts[0],
        json!({
            "id": "installed-original-part",
            "sessionID": "installed-plugin-session",
            "messageID": "installed-plugin-turn",
            "type": "text",
            "text": "What did Magnesium retain for this workspace?",
        })
    );
    assert_eq!(parts[1]["id"], "prt_magnesium_installed-plugin-turn");
    assert_eq!(parts[1]["synthetic"], true);
    assert_eq!(parts[1]["sessionID"], "installed-plugin-session");
    assert_eq!(parts[1]["messageID"], "installed-plugin-turn");
    let context = parts[1]["text"].as_str().expect("native runtime context");
    assert!(context.contains("learner-model/v2"));
    assert!(context.contains("context.prepare"));
    assert_eq!(support::v2::count(&environment, "learning_events"), 0);
}

#[cfg(unix)]
#[test]
fn installed_opencode_plugin_captures_json_looking_learner_text_exactly() {
    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    let install = environment
        .command()
        .env("OPENCODE_TEST_BINARY", support::test_binary())
        .arg("init")
        .output()
        .expect("install OpenCode plugin");
    assert!(
        install.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&install.stderr)
    );
    support::v2::start(&environment, "opencode", "json-looking-session");
    let learner_text = "  {\"sources\":\"[]\",\"enabled\":\"true\"}\nJSON-looking learner text  ";
    let script = r#"
import { readFileSync } from "node:fs";
if (process.env.MAGNESIUM_BIN !== undefined) throw new Error("unexpected runtime override");
process.env.PATH = "";
const source = readFileSync(process.argv[1]);
const { Magnesium } = await import("data:text/javascript;base64," + source.toString("base64"));
const warnings = [];
const hooks = await Magnesium({
    directory: process.cwd(),
    client: {
        app: { log: async (entry) => warnings.push(entry) },
        session: { get: async () => ({ data: { id: "json-looking-session" } }) },
    },
});
const turn = {
    message: { id: "json-looking-turn" },
    parts: [{
        id: "json-looking-original-part",
        sessionID: "json-looking-session",
        messageID: "json-looking-turn",
        type: "text",
        text: process.argv[2],
    }],
};
await hooks["chat.message"]({ sessionID: "json-looking-session" }, turn);
console.log(JSON.stringify({ parts: turn.parts, warnings }));
"#;
    let output = environment
        .command_with("node", &environment.workspace)
        .args(["--input-type=module", "--eval", script])
        .arg(
            environment
                .opencode_config_dir()
                .join("plugins/magnesium.js"),
        )
        .arg(learner_text)
        .output()
        .expect("run installed OpenCode plugin with Node");
    assert!(
        output.status.success(),
        "installed plugin failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).expect("installed plugin result");
    assert_eq!(result["warnings"], json!([]));
    let parts = result["parts"].as_array().expect("plugin message parts");
    assert_eq!(parts.len(), 2);
    assert_eq!(
        parts[0],
        json!({
            "id": "json-looking-original-part",
            "sessionID": "json-looking-session",
            "messageID": "json-looking-turn",
            "type": "text",
            "text": learner_text,
        })
    );
    assert_eq!(parts[1]["id"], "prt_magnesium_json-looking-turn");
    assert_eq!(parts[1]["synthetic"], true);
    assert_eq!(parts[1]["sessionID"], "json-looking-session");
    assert_eq!(parts[1]["messageID"], "json-looking-turn");
    assert!(
        parts[1]["text"]
            .as_str()
            .unwrap()
            .contains("learner-model/v2")
    );
    let connection = Connection::open(
        environment
            .workspace
            .join(support::CONTROL_DIR)
            .join("state.sqlite"),
    )
    .unwrap();
    let stored: (String, String, String, String) = connection
        .query_row(
            "SELECT text,host_kind,host_thread_id,origin FROM learning_events",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        stored,
        (
            learner_text.to_owned(),
            "opencode".to_owned(),
            "magnesium-h1:opencode:json-looking-session".to_owned(),
            "host_observed".to_owned(),
        )
    );
    assert_eq!(support::v2::count(&environment, "learning_events"), 1);
}

#[test]
fn post_compact_without_a_turn_returns_empty_output_and_captures_nothing() {
    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    init_workspace(&environment);

    // The OpenCode adapter emits PostCompact with no turn in flight.
    let compact = run_hook(
        &environment,
        "post-compact",
        json!({
            "session_id": "opencode-compact-session",
            "transcript_path": "/not/read/transcript.jsonl",
            "cwd": environment.workspace,
            "hook_event_name": "PostCompact",
            "host_kind": "opencode",
            "trigger": "auto"
        }),
    );
    assert!(compact.status.success());
    assert_eq!(compact.stdout, b"{}");
    assert_eq!(support::v2::count(&environment, "learning_events"), 0);
}
