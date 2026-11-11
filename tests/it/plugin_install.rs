use std::fs;
use std::io::Write;
use std::process::Stdio;

use crate::support::{TestEnvironment, test_binary};
use serde_json::{Value, json};

#[test]
fn claude_plugin_is_exact_and_native() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    // Plugins ship with the runtime, so they carry its version and protocol.
    let plugin_version = env!("CARGO_PKG_VERSION");
    let protocol = "2".to_owned();

    // The Claude Code plugin is a manifest, a real MCP server declaration,
    // and the four lifecycle hooks bound to the native executable. The
    // installed command line supplies `--host-kind claude` because the
    // official Claude Code event JSON carries session, cwd, and prompt but no
    // host-kind field.
    let manifest: Value = serde_json::from_slice(
        &fs::read(root.join("claude-plugin/.claude-plugin/plugin.json")).expect("claude manifest"),
    )
    .expect("claude manifest JSON");
    assert_eq!(manifest["name"], "magnesium");
    assert_eq!(manifest["version"], plugin_version);
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
    // Claude Code restates the core rules through SessionStart after a
    // compaction, so the plugin registers no compaction hooks.
    let mut events: Vec<&str> = hooks["hooks"]
        .as_object()
        .expect("hook events")
        .keys()
        .map(String::as_str)
        .collect();
    events.sort_unstable();
    assert_eq!(
        events,
        [
            "PostToolUse",
            "SessionEnd",
            "SessionStart",
            "Stop",
            "UserPromptSubmit"
        ]
    );
    for (event, groups) in hooks["hooks"].as_object().expect("hook events") {
        // SessionEnd also ends a conversation on /clear and in-process /resume.
        let expected = match event.as_str() {
            "SessionStart" => Some(json!("startup|resume|clear|compact")),
            "SessionEnd" => Some(json!("other|clear|resume")),
            // Only the window's reply tool ends a turn.
            "PostToolUse" => Some(json!("mcp__plugin_magnesium_magnesium__reply")),
            _ => None,
        };
        assert_eq!(
            groups[0].get("matcher").cloned(),
            expected,
            "{event} matcher"
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
    let instructions = format!("{}/INSTRUCTIONS.md", crate::support::CONTROL_DIR);
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
    let instructions = format!("{}/INSTRUCTIONS.md", crate::support::CONTROL_DIR);
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
            env!("CARGO_PKG_VERSION"),
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

/// A plugin copy from before the compaction hooks were removed, or a Claude
/// Code session started before the program was replaced, still runs them.
/// Exit code 2 from a PreCompact hook blocks compaction, so both retired
/// commands succeed silently and record nothing, inside a study folder or
/// outside one, whatever their input holds.
#[test]
fn retired_compaction_hooks_from_an_older_plugin_copy_exit_cleanly() {
    let environment = TestEnvironment::from_fixture("basic").expect("test environment");
    let init = environment
        .command()
        .arg("init")
        .output()
        .expect("initialize");
    assert!(init.status.success());
    let outside = environment.root.join("outside");
    fs::create_dir(&outside).expect("folder outside any study folder");
    let before = snapshot(&environment);
    for (event, name) in [
        ("pre-compact", "PreCompact"),
        ("post-compact", "PostCompact"),
    ] {
        let payload = json!({
            "session_id": "older-plugin", "cwd": environment.workspace,
            "hook_event_name": name, "trigger": "auto", "custom_instructions": ""
        })
        .to_string();
        for folder in [&environment.workspace, &outside] {
            for input in [payload.as_bytes(), b"", b"not json"] {
                // The exact command line an older plugin copy registered.
                let mut child = environment
                    .command_with(test_binary(), folder)
                    .args([
                        "_internal",
                        "hook",
                        event,
                        "--plugin-version",
                        env!("CARGO_PKG_VERSION"),
                        "--adapter-protocol-version",
                        "2",
                        "--host-kind",
                        "claude",
                    ])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .expect("spawn retired hook");
                // The hook may exit before it reads its input.
                let _ = child.stdin.take().expect("hook stdin").write_all(input);
                let output = child.wait_with_output().expect("hook output");
                assert_eq!(output.status.code(), Some(0), "{event}: {output:?}");
                assert!(output.stdout.is_empty(), "{event}: {output:?}");
                assert!(output.stderr.is_empty(), "{event}: {output:?}");
            }
        }
    }
    assert_eq!(before, snapshot(&environment));
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
}

fn snapshot(environment: &TestEnvironment) -> Vec<(String, Vec<u8>)> {
    let mut result = Vec::new();
    collect(
        &environment.workspace.join(crate::support::CONTROL_DIR),
        &environment.workspace.join(crate::support::CONTROL_DIR),
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
