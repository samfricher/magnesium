use std::fs;
use std::io::Write;
use std::process::Stdio;

use serde_json::{Value, json};
use support::{TestEnvironment, codex_command, copy_tree, test_binary};

#[test]
fn repository_marketplace_and_plugin_commands_are_exact_and_native() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let marketplace: Value = serde_json::from_slice(
        &fs::read(root.join(".agents/plugins/marketplace.json")).expect("marketplace manifest"),
    )
    .expect("marketplace JSON");
    assert_eq!(marketplace["name"], "magnesium");
    assert_eq!(marketplace["interface"]["displayName"], "Magnesium");
    assert_eq!(
        marketplace["plugins"]
            .as_array()
            .expect("plugin list")
            .len(),
        1
    );
    let plugin = &marketplace["plugins"][0];
    assert_eq!(plugin["name"], "magnesium");
    assert_eq!(
        plugin["source"],
        json!({"source": "local", "path": "./codex-plugin"})
    );
    assert_eq!(
        plugin["policy"],
        json!({"installation": "AVAILABLE", "authentication": "ON_INSTALL"})
    );
    assert_eq!(plugin["category"], "Productivity");

    let mcp: Value = serde_json::from_slice(
        &fs::read(root.join("codex-plugin/.mcp.json")).expect("MCP manifest"),
    )
    .expect("MCP JSON");
    assert_eq!(
        mcp["mcpServers"]["magnesium"]["args"],
        json!([
            "_internal",
            "mcp-server",
            "--plugin-version",
            "0.2.0",
            "--adapter-protocol-version",
            "2",
            "--host-kind",
            "codex"
        ]),
        "the Codex MCP server must name its launching host"
    );
    let hooks: Value = serde_json::from_slice(
        &fs::read(root.join("codex-plugin/hooks/hooks.json")).expect("hooks manifest"),
    )
    .expect("hooks JSON");
    for hooks in hooks["hooks"].as_object().expect("hook events").values() {
        for entry in hooks.as_array().expect("hook groups") {
            for hook in entry["hooks"].as_array().expect("hook commands") {
                for field in ["command", "commandWindows"] {
                    let command = hook[field].as_str().expect("native hook command");
                    assert!(
                        command.contains("--plugin-version 0.2.0 --adapter-protocol-version 2")
                    );
                    assert!(!command.contains("python") && !command.contains("node"));
                }
            }
        }
    }
}

#[test]
fn claude_plugin_is_exact_and_native() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    // Plugins ship with the runtime, so they carry its version and protocol.
    let plugin_version = env!("CARGO_PKG_VERSION");
    let protocol = "2".to_owned();
    let codex_plugin: Value = serde_json::from_slice(
        &fs::read(root.join("codex-plugin/.codex-plugin/plugin.json")).expect("codex manifest"),
    )
    .expect("codex manifest JSON");

    // The Claude Code plugin mirrors the Codex skeleton: manifest, real MCP
    // server declaration, and the six lifecycle hooks bound to the native
    // executable. The installed command line supplies `--host-kind claude`
    // because the official Claude Code event JSON carries session, cwd, and
    // prompt but no host-kind field.
    let manifest: Value = serde_json::from_slice(
        &fs::read(root.join("claude-plugin/.claude-plugin/plugin.json")).expect("claude manifest"),
    )
    .expect("claude manifest JSON");
    assert_eq!(manifest["name"], "magnesium");
    assert_eq!(manifest["version"], plugin_version);
    assert_eq!(manifest["version"], codex_plugin["version"]);
    assert!(
        manifest["description"]
            .as_str()
            .is_some_and(|d| !d.is_empty())
    );

    let mcp: Value = serde_json::from_slice(
        &fs::read(root.join("claude-plugin/.mcp.json")).expect("claude MCP manifest"),
    )
    .expect("claude MCP JSON");
    assert_eq!(
        mcp["mcpServers"]["magnesium"]["command"],
        json!("magnesium"),
        "the Claude MCP server must invoke the real native executable"
    );
    assert_eq!(
        mcp["mcpServers"]["magnesium"]["args"],
        json!([
            "_internal",
            "mcp-server",
            "--plugin-version",
            plugin_version,
            "--adapter-protocol-version",
            protocol,
            "--host-kind",
            "claude",
        ]),
        "the Claude MCP server must name its launching host"
    );

    let hooks: Value = serde_json::from_slice(
        &fs::read(root.join("claude-plugin/hooks/hooks.json")).expect("claude hooks manifest"),
    )
    .expect("claude hooks JSON");
    let codex_hooks: Value = serde_json::from_slice(
        &fs::read(root.join("codex-plugin/hooks/hooks.json")).expect("codex hooks manifest"),
    )
    .expect("codex hooks JSON");
    let events: Vec<String> = hooks["hooks"]
        .as_object()
        .expect("hook events")
        .keys()
        .cloned()
        .collect();
    let codex_events: Vec<String> = codex_hooks["hooks"]
        .as_object()
        .expect("codex hook events")
        .keys()
        .cloned()
        .collect();
    assert_eq!(
        events, codex_events,
        "Claude must cover the same lifecycle events"
    );
    for (event, groups) in hooks["hooks"].as_object().expect("hook events") {
        // Claude also ends a conversation on /clear and in-process /resume.
        let expected = if event == "SessionEnd" {
            Some(json!("other|clear|resume"))
        } else {
            codex_hooks["hooks"][event][0].get("matcher").cloned()
        };
        assert_eq!(
            groups[0].get("matcher").cloned(),
            expected,
            "{event} matcher must mirror the Codex declaration"
        );
        for hook in groups[0]["hooks"].as_array().expect("hook commands") {
            let command = hook["command"].as_str().expect("native hook command");
            assert!(
                command.starts_with("magnesium _internal hook "),
                "{event} must invoke the native runtime: {command}"
            );
            assert!(command.contains(&format!("--plugin-version {plugin_version}")));
            assert!(command.contains(&format!("--adapter-protocol-version {protocol}")));
            assert!(
                command.contains("--host-kind claude"),
                "{event} must bind the Claude host kind: {command}"
            );
            assert!(!command.contains("python") && !command.contains("node"));
        }
    }
    // The UserPromptSubmit hook must fit Claude Code's 30-second command-hook
    // budget for that event.
    assert!(
        hooks["hooks"]["UserPromptSubmit"][0]["hooks"][0]["timeout"]
            .as_i64()
            .is_some_and(|timeout| timeout <= 30)
    );

    // The plugin directory is its own one-plugin marketplace, so Claude Code
    // can load it in place without copying it into a separate layout.
    let marketplace: Value = serde_json::from_slice(
        &fs::read(root.join("claude-plugin/.claude-plugin/marketplace.json"))
            .expect("claude marketplace"),
    )
    .expect("claude marketplace JSON");
    assert_eq!(marketplace["name"], "magnesium");
    assert_eq!(
        marketplace["plugins"],
        json!([{"name": "magnesium", "source": "./", "description": marketplace["plugins"][0]["description"]}])
    );

    let skill =
        fs::read_to_string(root.join("claude-plugin/skills/learn/SKILL.md")).expect("claude skill");
    let instructions = format!("{}/INSTRUCTIONS.md", support::CONTROL_DIR);
    for required in [
        "teaching-policy/v2",
        instructions.as_str(),
        "learning_step",
        "learner-model/v2",
    ] {
        assert!(
            skill.contains(required),
            "Claude skill must reference the shared contract: {required}"
        );
    }
}

fn init_json(environment: &TestEnvironment) -> Value {
    let output = environment
        .command()
        .args(["init", "--json"])
        .output()
        .expect("run init");
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).expect("init JSON")
}

fn read_json(path: &std::path::Path) -> Value {
    serde_json::from_slice(&fs::read(path).expect("settings file")).expect("settings JSON")
}

/// The rules init allows in a study folder: Magnesium's MCP server, reads of the
/// copied plugin, and a launch of each shipped skill. Claude Code rules name an absolute path with `//` and
/// match Windows paths in POSIX form (`C:\Users` is `//c/Users`).
fn magnesium_rules(report: &Value) -> Vec<Value> {
    let settings = read_json(std::path::Path::new(
        report["claude"]["user_settings"].as_str().unwrap(),
    ));
    let plugin_dir = settings["extraKnownMarketplaces"]["magnesium"]["source"]["path"]
        .as_str()
        .unwrap()
        .replace('\\', "/");
    let plugin_dir = match plugin_dir.split_once(':') {
        Some((drive, rest)) if cfg!(windows) => format!("/{}{rest}", drive.to_ascii_lowercase()),
        _ => plugin_dir,
    };
    vec![
        json!("mcp__plugin_magnesium_magnesium__*"),
        json!(format!("Read(/{plugin_dir}/**)")),
        json!("Skill(magnesium:learn)"),
        json!("Skill(magnesium:papers)"),
        json!("Skill(magnesium:help)"),
        json!("Bash(magnesium digest)"),
    ]
}

#[test]
fn init_enables_the_claude_plugin_for_this_folder_only() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let environment = TestEnvironment::from_fixture("basic").expect("test environment");
    let local = environment.workspace.join(".claude/settings.local.json");
    fs::create_dir_all(local.parent().unwrap()).unwrap();
    fs::write(&local, br#"{"permissions":{"allow":["Read"]}}"#).unwrap();

    let report = init_json(&environment);
    let claude = &report["claude"];
    assert_eq!(claude["status"], "enabled", "{report}");
    assert_eq!(claude["enabled_everywhere"], false);
    let settings_path = std::path::PathBuf::from(claude["user_settings"].as_str().unwrap());
    let settings = read_json(&settings_path);
    // The user settings only declare where the plugin is; nothing is enabled there.
    assert_eq!(settings.as_object().unwrap().len(), 1, "{settings}");
    let plugin_dir = std::path::PathBuf::from(
        settings["extraKnownMarketplaces"]["magnesium"]["source"]["path"]
            .as_str()
            .unwrap(),
    );
    assert_eq!(
        settings["extraKnownMarketplaces"]["magnesium"]["source"]["source"],
        "directory"
    );
    for relative in [
        ".claude-plugin/marketplace.json",
        ".claude-plugin/plugin.json",
        ".mcp.json",
        "hooks/hooks.json",
        "skills/learn/SKILL.md",
        "skills/papers/SKILL.md",
        "skills/papers/templates/plan.md",
        "skills/papers/templates/gap-log.md",
        "skills/papers/templates/graph.yaml",
        "skills/papers/templates/drills.md",
        "skills/papers/templates/past-papers-README.md",
        "skills/papers/templates/preamble.tex",
        "skills/papers/templates/build.sh",
        "skills/papers/templates/paper.md",
        "skills/help/SKILL.md",
    ] {
        assert_eq!(
            fs::read(plugin_dir.join(relative)).expect("copied plugin file"),
            fs::read(root.join("claude-plugin").join(relative)).expect("bundled plugin file"),
            "{relative} must match the bundled plugin"
        );
    }
    assert_eq!(
        fs::read(plugin_dir.join("skills/help/user-guide.md")).expect("copied user guide"),
        fs::read(root.join("docs/user-guide.md")).expect("repository user guide"),
        "the help skill must install the repository user guide unchanged"
    );
    let papers = fs::read_to_string(plugin_dir.join("skills/papers/SKILL.md")).unwrap();
    let (frontmatter, _) = papers
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n"))
        .expect("the installed papers skill opens with frontmatter");
    assert!(
        frontmatter.lines().any(|line| line == "name: papers"),
        "{frontmatter}"
    );
    let instructions = format!("{}/INSTRUCTIONS.md", support::CONTROL_DIR);
    for required in [
        "stage:\"papers\"",
        "stage:\"papers_loop\"",
        instructions.as_str(),
    ] {
        assert!(papers.contains(required), "papers skill misses {required}");
    }
    let help = fs::read_to_string(plugin_dir.join("skills/help/SKILL.md")).unwrap();
    let (frontmatter, _) = help
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n"))
        .expect("the installed help skill opens with frontmatter");
    assert!(
        frontmatter.lines().any(|line| line == "name: help"),
        "{frontmatter}"
    );
    // The help skill grants nothing itself: Magnesium's folder settings allow `magnesium digest`.
    assert!(
        !frontmatter
            .lines()
            .any(|line| line.starts_with("allowed-tools:")),
        "{frontmatter}"
    );
    assert!(help.contains("Run `magnesium digest` once"), "{help}");
    // Magnesium's rules are allowed next to the user's own, so Magnesium does not ask.
    let mut allow = vec![json!("Read")];
    allow.extend(magnesium_rules(&report));
    // /magnesium:help runs `magnesium digest`; the folder settings, not the skill, allow it.
    assert!(
        read_json(&local)["permissions"]["allow"]
            .as_array()
            .unwrap()
            .contains(&json!("Bash(magnesium digest)")),
        "{local:?}"
    );
    assert_eq!(
        read_json(&local),
        json!({"permissions":{"allow":allow},"enabledPlugins":{"magnesium@magnesium":true}})
    );
    assert_eq!(claude["added_rules"], json!(magnesium_rules(&report)));
    assert_eq!(report["warnings"], json!([]), "{report}");

    // Rerunning changes nothing.
    let before = (fs::read(&settings_path).unwrap(), fs::read(&local).unwrap());
    let rerun = init_json(&environment);
    assert_eq!(rerun["claude"]["status"], "enabled");
    assert_eq!(rerun["claude"]["added_rules"], json!([]));
    assert_eq!(
        (fs::read(&settings_path).unwrap(), fs::read(&local).unwrap()),
        before
    );
}

#[test]
fn init_says_when_it_allows_magnesium_in_claude_code() {
    let environment = TestEnvironment::from_fixture("basic").expect("test environment");
    let init = || {
        let output = environment
            .command()
            .arg("init")
            .output()
            .expect("run init");
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).expect("init output is UTF-8")
    };
    let allowed = "Claude Code plugin: allowed Magnesium's tools, plugin files and skills in .claude/settings.local.json\n";
    let first = init();
    assert!(first.contains(allowed), "{first}");

    // A rerun adds no rule, so it prints no permission line.
    let second = init();
    assert!(
        second.contains("Claude Code plugin: enabled for this folder"),
        "{second}"
    );
    assert!(!second.contains("allowed Magnesium's"), "{second}");
}

#[test]
fn init_allows_magnesium_in_an_enabled_folder_and_keeps_the_learners_ask_and_deny_rules() {
    // A folder enabled by an earlier Magnesium gets the rules on the next init.
    let environment = TestEnvironment::from_fixture("basic").expect("test environment");
    let local = environment.workspace.join(".claude/settings.local.json");
    fs::create_dir_all(local.parent().unwrap()).unwrap();
    fs::write(
        &local,
        br#"{"enabledPlugins":{"magnesium@magnesium":true}}"#,
    )
    .unwrap();
    let report = init_json(&environment);
    assert_eq!(
        read_json(&local),
        json!({"enabledPlugins":{"magnesium@magnesium":true},"permissions":{"allow":magnesium_rules(&report)}})
    );

    // The learner's own ask and deny rules stay exactly as they are; Claude
    // Code applies them before allow, so they still win.
    let environment = TestEnvironment::from_fixture("basic").expect("test environment");
    let local = environment.workspace.join(".claude/settings.local.json");
    fs::create_dir_all(local.parent().unwrap()).unwrap();
    let ask = json!(["mcp__plugin_magnesium_magnesium__memory"]);
    let deny = json!(["mcp__plugin_magnesium_magnesium__*", "Read(//etc/**)"]);
    let choices = json!({"permissions":{"ask":ask,"deny":deny}});
    fs::write(&local, choices.to_string()).unwrap();
    let report = init_json(&environment);
    assert_eq!(report["claude"]["status"], "enabled", "{report}");
    assert_eq!(
        read_json(&local),
        json!({
            "permissions":{"ask":ask,"deny":deny,"allow":magnesium_rules(&report)},
            "enabledPlugins":{"magnesium@magnesium":true}
        })
    );
}

#[test]
fn init_enables_magnesium_but_adds_no_rules_when_permissions_has_another_shape() {
    for permissions in [json!("allow everything"), json!({"allow":"Read"})] {
        let environment = TestEnvironment::from_fixture("basic").expect("test environment");
        let local = environment.workspace.join(".claude/settings.local.json");
        fs::create_dir_all(local.parent().unwrap()).unwrap();
        fs::write(&local, json!({"permissions":permissions}).to_string()).unwrap();
        let report = init_json(&environment);
        assert_eq!(report["claude"]["status"], "enabled", "{report}");
        assert_eq!(
            read_json(&local),
            json!({"permissions":permissions,"enabledPlugins":{"magnesium@magnesium":true}})
        );
        assert_eq!(report["claude"]["added_rules"], json!([]), "{report}");
        let warnings = report["warnings"].as_array().unwrap();
        assert!(
            warnings.iter().any(|warning| warning
                .as_str()
                .unwrap()
                .contains("Magnesium added no permission rules")),
            "{report}"
        );
    }
}

#[test]
fn init_keeps_a_folder_opt_out_and_a_different_magnesium_plugin_source() {
    let environment = TestEnvironment::from_fixture("basic").expect("test environment");
    let local = environment.workspace.join(".claude/settings.local.json");
    fs::create_dir_all(local.parent().unwrap()).unwrap();
    fs::write(
        &local,
        br#"{"enabledPlugins":{"magnesium@magnesium":false}}"#,
    )
    .unwrap();
    let report = init_json(&environment);
    assert_eq!(report["claude"]["status"], "disabled", "{report}");
    assert_eq!(
        read_json(&local),
        json!({"enabledPlugins":{"magnesium@magnesium":false}})
    );

    assert!(
        report["claude"]["folder_settings"]
            .as_str()
            .unwrap()
            .ends_with(".claude/settings.local.json")
    );

    let other = TestEnvironment::from_fixture("basic").expect("second environment");
    let first = init_json(&other);
    let user_settings =
        std::path::PathBuf::from(first["claude"]["user_settings"].as_str().unwrap());
    let foreign = br#"{"extraKnownMarketplaces":{"magnesium":{"source":{"source":"github","repo":"someone/magnesium"}}}}"#;
    fs::write(&user_settings, foreign).unwrap();
    let rejected = init_json(&other);
    assert_eq!(rejected["claude"]["status"], "rejected", "{rejected}");
    assert!(
        rejected["claude"]["reason"]
            .as_str()
            .unwrap()
            .contains("different `magnesium` plugin source"),
        "{rejected}"
    );
    assert_eq!(fs::read(&user_settings).unwrap(), foreign);
}

#[test]
fn incompatible_hook_fails_open_without_workspace_mutation() {
    let environment = TestEnvironment::from_fixture("basic").expect("test environment");
    let init = environment
        .command()
        .arg("init")
        .output()
        .expect("initialize");
    assert!(init.status.success());
    let before = snapshot(&environment);
    let mut child = environment
        .command_with(test_binary(), &environment.workspace)
        .args([
            "_internal",
            "hook",
            "session-start",
            "--plugin-version",
            "0.2.0",
            "--adapter-protocol-version",
            "1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn incompatible hook");
    child
        .stdin
        .take()
        .expect("hook stdin")
        .write_all(
            serde_json::to_string(&json!({
                "session_id": "plugin-install", "cwd": environment.workspace,
                "hook_event_name": "SessionStart", "source": "startup"
            }))
            .expect("hook input")
            .as_bytes(),
        )
        .expect("write hook input");
    let output = child.wait_with_output().expect("hook output");
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert_eq!(before, snapshot(&environment));
}

// Native proofs explicitly opt into the selected host executable. Ordinary test
// commands keep the absent-host default even when a real Codex is on PATH.
#[cfg(unix)]
fn native_runtime_command(environment: &TestEnvironment) -> std::process::Command {
    let executable = codex_command(environment).get_program().to_owned();
    let mut command = environment.command();
    command.env("CODEX_TEST_BINARY", executable);
    command
}

#[cfg(unix)]
#[test]
#[ignore = "requires an authorized installed Codex CLI with plugin marketplace commands"]
fn isolated_codex_install_caches_the_plugin_source() {
    let environment = TestEnvironment::from_fixture("basic").expect("test environment");
    let probe = codex_command(&environment)
        .arg("--version")
        .output()
        .expect("opt-in native test requires an installed Codex CLI");
    assert!(
        probe.status.success(),
        "Codex version probe failed: {probe:?}"
    );
    let marketplace = environment.root.join("marketplace");
    fs::create_dir_all(marketplace.join(".agents/plugins")).expect("marketplace metadata");
    copy_tree(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("codex-plugin"),
        &marketplace.join("codex-plugin"),
    )
    .expect("copy plugin bundle");
    fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".agents/plugins/marketplace.json"),
        marketplace.join(".agents/plugins/marketplace.json"),
    )
    .expect("copy marketplace");
    for (operation, args) in [
        (
            "marketplace",
            vec![
                "plugin",
                "marketplace",
                "add",
                marketplace.to_str().expect("marketplace path"),
                "--json",
            ],
        ),
        (
            "plugin",
            vec!["plugin", "add", "magnesium@magnesium", "--json"],
        ),
    ] {
        let output = codex_command(&environment)
            .args(args)
            .output()
            .expect("Codex plugin command");
        if !output.status.success()
            && String::from_utf8_lossy(&output.stderr).contains("unrecognized subcommand")
        {
            panic!("opt-in native test requires Codex plugin marketplace commands");
        }
        assert!(
            output.status.success(),
            "Codex {operation} failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let listing = codex_command(&environment)
        .args(["plugin", "list", "--json"])
        .output()
        .expect("list plugins");
    assert!(listing.status.success());
    let value: Value = serde_json::from_slice(&listing.stdout).expect("plugin list JSON");
    assert!(
        value["installed"]
            .as_array()
            .is_some_and(|plugins| { plugins.iter().any(|plugin| plugin["name"] == "magnesium") })
    );
    let config_before = fs::read(environment.codex_home.join("config.toml")).unwrap();
    let init = native_runtime_command(&environment)
        .arg("init")
        .output()
        .unwrap();
    assert!(init.status.success());
    assert!(String::from_utf8_lossy(&init.stderr).contains("points to another source"));
    assert_eq!(
        fs::read(environment.codex_home.join("config.toml")).unwrap(),
        config_before
    );
}

#[cfg(unix)]
#[test]
#[ignore = "requires an authorized installed Codex CLI and native plugin approval workflow"]
fn public_init_and_update_refresh_the_qualified_plugin_without_changing_trust() {
    use support::codex_app_server_request;

    let environment = TestEnvironment::from_fixture("basic").expect("test environment");
    let probe = codex_command(&environment)
        .arg("--version")
        .output()
        .expect("opt-in native test requires an installed Codex CLI");
    assert!(
        probe.status.success(),
        "Codex version probe failed: {probe:?}"
    );
    let run_codex = |args: &[&str]| -> Value {
        let output = codex_command(&environment)
            .args(args)
            .output()
            .expect("Codex command");
        assert!(
            output.status.success(),
            "Codex {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("Codex JSON")
    };
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let other_market = environment.root.join("other-market");
    fs::create_dir_all(other_market.join(".agents/plugins")).unwrap();
    copy_tree(
        &source.join("codex-plugin"),
        &other_market.join("codex-plugin"),
    )
    .unwrap();
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(source.join(".agents/plugins/marketplace.json")).unwrap())
            .unwrap();
    manifest["name"] = json!("magnesium-other");
    fs::write(
        other_market.join(".agents/plugins/marketplace.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    run_codex(&[
        "plugin",
        "marketplace",
        "add",
        other_market.to_str().unwrap(),
        "--json",
    ]);
    let other = run_codex(&["plugin", "add", "magnesium@magnesium-other", "--json"]);
    let other_skill = std::path::Path::new(other["installedPath"].as_str().unwrap())
        .join("skills/learn/SKILL.md");
    let other_before = fs::read(&other_skill).unwrap();

    let init = native_runtime_command(&environment)
        .arg("init")
        .output()
        .unwrap();
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    assert!(!String::from_utf8_lossy(&init.stderr).contains("could not install plugin"));
    let listing = run_codex(&["plugin", "list", "--json"]);
    let installed = listing["installed"].as_array().unwrap();
    assert!(
        installed
            .iter()
            .any(|p| p["pluginId"] == "magnesium@magnesium" && p["enabled"] == true)
    );
    assert!(
        installed
            .iter()
            .any(|p| p["pluginId"] == "magnesium@magnesium-other" && p["enabled"] == true)
    );
    let market = environment.data_dir().join("marketplace");
    let skill_source = market.join("codex-plugin/skills/learn/SKILL.md");
    let expected = fs::read(source.join("codex-plugin/skills/learn/SKILL.md")).unwrap();

    // Record actual hook approval in the disposable host, then ensure refresh
    // preserves those choices rather than obtaining a pass by approving hooks.
    let response = codex_app_server_request(
        &environment,
        json!({"method":"hooks/list", "id":2, "params":{"cwds":[environment.workspace]}}),
    );
    let hooks = response["result"]["data"][0]["hooks"].as_array().unwrap();
    let selected: Vec<_> = hooks
        .iter()
        .filter(|h| h["pluginId"] == "magnesium@magnesium")
        .collect();
    assert_eq!(selected.len(), 6);
    let mut config = fs::OpenOptions::new()
        .append(true)
        .open(environment.codex_home.join("config.toml"))
        .unwrap();
    for hook in &selected {
        writeln!(
            config,
            "\n[hooks.state.\"{}\"]\ntrusted_hash = \"{}\"",
            hook["key"].as_str().unwrap(),
            hook["currentHash"].as_str().unwrap()
        )
        .unwrap();
    }
    drop(config);
    let config_before = fs::read(environment.codex_home.join("config.toml")).unwrap();

    // Install stale same-version bytes through Codex, then repair through public init.
    fs::write(&skill_source, b"stale same-version skill\n").unwrap();
    let receipt = run_codex(&["plugin", "add", "magnesium@magnesium", "--json"]);
    let installed_skill = std::path::Path::new(receipt["installedPath"].as_str().unwrap())
        .join("skills/learn/SKILL.md");
    assert_eq!(
        fs::read(&installed_skill).unwrap(),
        b"stale same-version skill\n"
    );
    let repeated = native_runtime_command(&environment)
        .arg("init")
        .output()
        .unwrap();
    assert!(repeated.status.success());
    assert_eq!(fs::read(&installed_skill).unwrap(), expected);
    assert_eq!(
        fs::read(environment.codex_home.join("config.toml")).unwrap(),
        config_before
    );

    fs::write(&skill_source, b"stale before public update\n").unwrap();
    run_codex(&["plugin", "add", "magnesium@magnesium", "--json"]);
    let update = environment
        .command()
        .env(
            "CODEX_TEST_BINARY",
            codex_command(&environment).get_program(),
        )
        .arg("update")
        .output()
        .unwrap();
    assert!(
        update.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&update.stdout),
        String::from_utf8_lossy(&update.stderr)
    );
    assert_eq!(fs::read(&installed_skill).unwrap(), expected);
    assert_eq!(fs::read(&other_skill).unwrap(), other_before);
    assert_eq!(
        fs::read(environment.codex_home.join("config.toml")).unwrap(),
        config_before
    );
    assert!(String::from_utf8_lossy(&update.stdout).contains("verified installed version"));
    assert!(
        String::from_utf8_lossy(&update.stdout).contains("does not report hook/protocol approval")
    );
    let info = environment
        .command()
        .args(["info", "--json"])
        .output()
        .unwrap();
    let info: Value = serde_json::from_slice(&info.stdout).unwrap();
    assert_eq!(info["instructions"]["status"], "current");

    // An explicitly disabled plugin stays disabled; initialization must not
    // re-enable it merely to refresh its files.
    let config_path = environment.codex_home.join("config.toml");
    let content = fs::read_to_string(&config_path).unwrap();
    let mut config: toml_edit::DocumentMut = content.parse().unwrap();
    config["plugins"]["magnesium@magnesium"]["enabled"] = toml_edit::value(false);
    fs::write(&config_path, config.to_string()).unwrap();
    let disabled_before = fs::read(&config_path).unwrap();
    let disabled = native_runtime_command(&environment)
        .arg("init")
        .output()
        .unwrap();
    assert!(disabled.status.success());
    assert!(String::from_utf8_lossy(&disabled.stderr).contains("disabled"));
    assert_eq!(fs::read(&config_path).unwrap(), disabled_before);
}

fn snapshot(environment: &TestEnvironment) -> Vec<(String, Vec<u8>)> {
    let mut result = Vec::new();
    collect(
        &environment.workspace.join(support::CONTROL_DIR),
        &environment.workspace.join(support::CONTROL_DIR),
        &mut result,
    );
    result
}

fn collect(root: &std::path::Path, path: &std::path::Path, result: &mut Vec<(String, Vec<u8>)>) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .strip_prefix(root)
            .ok()
            .is_some_and(|relative| relative.starts_with("diagnostics"))
        {
            continue;
        }
        if path.is_dir() {
            collect(root, &path, result);
        } else if let Ok(bytes) = fs::read(&path) {
            result.push((
                path.strip_prefix(root)
                    .expect("snapshot path")
                    .display()
                    .to_string(),
                bytes,
            ));
        }
    }
    result.sort();
}

#[cfg(unix)]
#[test]
fn claude_plugin_install_refuses_a_symlinked_managed_directory_without_writing_outside() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let environment = TestEnvironment::from_fixture("basic").expect("test environment");
    let init = || {
        let output = environment
            .command()
            .args(["init", "--json"])
            .output()
            .expect("run init");
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice::<Value>(&output.stdout).expect("init JSON")
    };
    let first = init();
    let settings = std::path::PathBuf::from(first["claude"]["user_settings"].as_str().unwrap());
    let settings: Value = serde_json::from_slice(&fs::read(settings).unwrap()).unwrap();
    let plugin_dir = std::path::PathBuf::from(
        settings["extraKnownMarketplaces"]["magnesium"]["source"]["path"]
            .as_str()
            .unwrap(),
    );

    let outside = environment.root.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("plugin.json"), b"unrelated\n").unwrap();
    fs::set_permissions(&outside, fs::Permissions::from_mode(0o755)).unwrap();
    let managed = plugin_dir.join(".claude-plugin");
    fs::remove_dir_all(&managed).unwrap();
    symlink(&outside, &managed).unwrap();

    let second = init();
    assert_eq!(second["claude"]["status"], "rejected", "{second}");
    assert!(
        second["claude"]["reason"]
            .as_str()
            .unwrap()
            .contains(".claude-plugin"),
        "{second}"
    );
    assert_eq!(
        fs::read(outside.join("plugin.json")).unwrap(),
        b"unrelated\n"
    );
    assert!(!outside.join("marketplace.json").exists());
    assert_eq!(
        fs::metadata(&outside).unwrap().permissions().mode() & 0o777,
        0o755
    );
    assert!(
        fs::symlink_metadata(&managed)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

/// `update` runs after a runtime upgrade. The Claude plugin copy
/// pins the plugin version in its hook and MCP command lines, so a copy left
/// from the previous release fails every hook and tool call with a version
/// mismatch until it is rewritten.
#[test]
fn update_refreshes_an_existing_claude_plugin_copy() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let environment = TestEnvironment::from_fixture("basic").expect("test environment");
    let report = init_json(&environment);
    assert_eq!(report["claude"]["status"], "enabled", "{report}");
    let plugin_dir = environment.data_dir().join("claude-plugin");
    for relative in [".mcp.json", "hooks/hooks.json"] {
        let path = plugin_dir.join(relative);
        let current = fs::read_to_string(&path).expect("copied plugin file");
        let previous = current.replace(env!("CARGO_PKG_VERSION"), "0.0.1");
        assert_ne!(previous, current, "{relative} pins the plugin version");
        fs::write(&path, previous).unwrap();
    }

    let update = environment
        .command()
        .arg("update")
        .output()
        .expect("run update");
    assert!(update.status.success(), "{update:?}");
    for relative in [".mcp.json", "hooks/hooks.json"] {
        assert_eq!(
            fs::read(plugin_dir.join(relative)).expect("refreshed plugin file"),
            fs::read(root.join("claude-plugin").join(relative)).expect("bundled plugin file"),
            "{relative} must match the bundled plugin after update"
        );
    }

    // A machine where init never ran gets no Claude plugin copy from update.
    let fresh = TestEnvironment::from_fixture("basic").expect("test environment");
    let update = fresh.command().arg("update").output().expect("run update");
    assert!(update.status.success(), "{update:?}");
    assert!(!fresh.data_dir().join("claude-plugin").exists());
}
