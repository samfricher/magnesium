use std::fs;

use support::TestEnvironment;

#[cfg(unix)]
#[test]
fn init_with_an_unusable_codex_does_not_claim_workspace_files_were_untouched() {
    use std::os::unix::fs::PermissionsExt;

    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    let failing = environment.root.join("codex-failing");
    fs::write(&failing, "#!/bin/sh\nexit 3\n").unwrap();
    fs::set_permissions(&failing, fs::Permissions::from_mode(0o755)).unwrap();

    let output = environment
        .command()
        .env("CODEX_TEST_BINARY", &failing)
        .arg("init")
        .output()
        .expect("run magnesium init with a failing Codex probe");
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("plugin: not registered"), "{stdout}");
    assert!(
        !stdout.contains("workspace files were not changed"),
        "{stdout}"
    );
    assert!(
        fs::canonicalize(&environment.workspace)
            .unwrap()
            .join(support::CONTROL_DIR)
            .join("INSTRUCTIONS.md")
            .is_file()
    );
}

#[test]
fn failed_agents_md_adapter_does_not_claim_no_workspace_files_were_replaced() {
    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    let control = fs::canonicalize(&environment.workspace)
        .unwrap()
        .join(support::CONTROL_DIR);
    let first = environment
        .command()
        .arg("init")
        .output()
        .expect("run magnesium init");
    assert!(first.status.success(), "{first:?}");
    let instructions = control.join("INSTRUCTIONS.md");
    let installed = fs::read(&instructions).unwrap();
    fs::write(&instructions, b"stale instructions\n").unwrap();
    fs::write(
        environment.workspace.join("AGENTS.md"),
        "<!-- magnesium:begin -->\nunterminated\n",
    )
    .unwrap();

    let output = environment
        .command()
        .args(["init", "--adapter", "agents-md"])
        .output()
        .expect("run magnesium init with mismatched AGENTS.md markers");
    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("mismatched Magnesium marker blocks"),
        "{stderr}"
    );
    assert!(
        !stderr.contains("No existing workspace files were replaced"),
        "{stderr}"
    );
    // The failure came after init had already rewritten INSTRUCTIONS.md.
    assert_eq!(fs::read(&instructions).unwrap(), installed);
}
